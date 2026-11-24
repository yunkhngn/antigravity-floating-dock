use std::collections::VecDeque;
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};
use tauri_plugin_global_shortcut::Shortcut;

use crate::diagnostics::DiagnosticLogger;
use crate::hooks;
use crate::models::{
    AgentActivityState, ConnectionStatus, DockEvent, DockSettings, DockSnapshot, TranscriptMessage,
};
use crate::paths;
use crate::session::{AntigravitySessionAdapter, SessionAdapter};

const MAX_RECENT_EVENTS: usize = 50;
const DEDUPE_WINDOW_MS: u64 = 1500;

struct InnerState {
    settings: DockSettings,
    recent_events: VecDeque<DockEvent>,
    transcript_tail: Vec<TranscriptMessage>,
    transcript_error: Option<String>,
    ipc_listening: bool,
    ipc_socket_path: String,
    vibrancy_mode: String,
    shortcut_errors: Vec<String>,
    toggle_chat_shortcut_id: Option<u32>,
    toggle_bubble_shortcut_id: Option<u32>,
    bubble_pinned: bool,
    last_event_at_ms: Option<u64>,
    auto_hide_gen: u64,
    last_running_flag: Option<bool>,
}

pub struct AppState {
    inner: RwLock<InnerState>,
    seq: AtomicU64,
    pub allow_exit: AtomicBool,
    pub logger: DiagnosticLogger,
    pub session_adapter: AntigravitySessionAdapter,
}

impl AppState {
    pub fn new() -> Self {
        let settings = load_settings();
        let bubble_pinned = settings.pin_bubble;
        Self {
            inner: RwLock::new(InnerState {
                settings,
                recent_events: VecDeque::with_capacity(MAX_RECENT_EVENTS),
                transcript_tail: Vec::new(),
                transcript_error: None,
                ipc_listening: false,
                ipc_socket_path: paths::socket_path().display().to_string(),
                vibrancy_mode: "pending".to_string(),
                shortcut_errors: Vec::new(),
                toggle_chat_shortcut_id: None,
                toggle_bubble_shortcut_id: None,
                bubble_pinned,
                last_event_at_ms: None,
                auto_hide_gen: 0,
                last_running_flag: None,
            }),
            seq: AtomicU64::new(1),
            allow_exit: AtomicBool::new(false),
            logger: DiagnosticLogger::new(),
            session_adapter: AntigravitySessionAdapter,
        }
    }

    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }

    pub fn set_ipc_listening(&self, listening: bool, socket_path: String) {
        if let Ok(mut g) = self.inner.write() {
            g.ipc_listening = listening;
            g.ipc_socket_path = socket_path;
        }
    }

    pub fn set_vibrancy_mode(&self, mode: String) {
        if let Ok(mut g) = self.inner.write() {
            g.vibrancy_mode = mode;
        }
    }

    pub fn set_shortcut_registration(
        &self,
        chat_id: Option<u32>,
        bubble_id: Option<u32>,
        errors: Vec<String>,
    ) {
        if let Ok(mut g) = self.inner.write() {
            g.toggle_chat_shortcut_id = chat_id;
            g.toggle_bubble_shortcut_id = bubble_id;
            g.shortcut_errors = errors;
        }
    }

    pub fn shortcut_ids(&self) -> (Option<u32>, Option<u32>) {
        self.inner
            .read()
            .map(|g| (g.toggle_chat_shortcut_id, g.toggle_bubble_shortcut_id))
            .unwrap_or((None, None))
    }

    pub fn update_settings(&self, new_settings: DockSettings) -> Result<(), String> {
        save_settings(&new_settings)?;
        if let Ok(mut g) = self.inner.write() {
            g.bubble_pinned = new_settings.pin_bubble;
            g.settings = new_settings;
        }
        Ok(())
    }

    pub fn settings(&self) -> DockSettings {
        self.inner
            .read()
            .map(|g| g.settings.clone())
            .unwrap_or_default()
    }

    pub fn set_bubble_pinned(&self, pinned: bool, app_handle: Option<&AppHandle>) {
        let updated_settings = if let Ok(mut g) = self.inner.write() {
            g.bubble_pinned = pinned;
            g.settings.pin_bubble = pinned;
            Some(g.settings.clone())
        } else {
            None
        };

        if let Some(s) = updated_settings {
            let _ = save_settings(&s);
        }

        if let Some(app) = app_handle {
            if pinned {
                show_bubble_without_focus(app);
            }
            self.emit_snapshot(app);
        }
    }

    pub fn ingest_event(self: &Arc<Self>, event: DockEvent, app_handle: Option<&AppHandle>) {
        let mut is_duplicate = false;
        let mut hide_after_ms: Option<u64> = None;
        let mut gen_for_hide: u64 = 0;

        let transcript_result = event
            .transcript_path
            .as_deref()
            .map(|p| self.session_adapter.read_transcript_tail(p, 12));

        if let Ok(mut g) = self.inner.write() {
            if let Some(latest) = g.recent_events.front() {
                if latest.dedupe_key == event.dedupe_key
                    && event.timestamp_ms.saturating_sub(latest.timestamp_ms) <= DEDUPE_WINDOW_MS
                {
                    is_duplicate = true;
                }
            }

            if is_duplicate {
                if let Some(front) = g.recent_events.front_mut() {
                    front.timestamp_ms = event.timestamp_ms;
                }
                g.last_event_at_ms = Some(event.timestamp_ms);
            } else {
                if g.recent_events.len() >= MAX_RECENT_EVENTS {
                    g.recent_events.pop_back();
                }
                g.last_event_at_ms = Some(event.timestamp_ms);

                match transcript_result {
                    Some(Ok(items)) => {
                        g.transcript_tail = items;
                        g.transcript_error = None;
                    }
                    Some(Err(err)) => {
                        g.transcript_error = Some(err);
                    }
                    None => {}
                }

                g.auto_hide_gen = g.auto_hide_gen.wrapping_add(1);
                gen_for_hide = g.auto_hide_gen;

                let should_auto_hide = !g.bubble_pinned
                    && matches!(
                        event.activity_state,
                        AgentActivityState::Idle | AgentActivityState::Unknown
                    );
                if should_auto_hide && g.settings.auto_hide_idle_ms > 0 {
                    hide_after_ms = Some(g.settings.auto_hide_idle_ms);
                }

                g.recent_events.push_front(event);
            }
        }

        if let Some(app) = app_handle {
            if !is_duplicate {
                show_bubble_without_focus(app);
                if let Some(delay) = hide_after_ms {
                    let state_ref = Arc::clone(self);
                    let app_clone = app.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        let should_hide = state_ref
                            .inner
                            .read()
                            .map(|g| g.auto_hide_gen == gen_for_hide && !g.bubble_pinned)
                            .unwrap_or(false);
                        if should_hide {
                            if let Some(win) = app_clone.get_webview_window("bubble") {
                                let _ = win.hide();
                            }
                        }
                    });
                }
            }
            self.emit_snapshot(app);
        }
    }

    pub fn refresh_running_status_if_changed(&self, app_handle: &AppHandle) {
        let running = self.session_adapter.is_antigravity_running();
        let changed = if let Ok(mut g) = self.inner.write() {
            if g.last_running_flag != Some(running) {
                g.last_running_flag = Some(running);
                true
            } else {
                false
            }
        } else {
            false
        };
        if changed {
            self.emit_snapshot(app_handle);
        }
    }

    pub fn snapshot(&self) -> DockSnapshot {
        let hook_status = hooks::inspect_hook_status();
        let running = self.session_adapter.is_antigravity_running();

        let guard = self.inner.read().expect("state lock poisoned");
        let active_event = guard.recent_events.front().cloned();
        let source_product = active_event
            .as_ref()
            .map(|e| e.source_product.as_str())
            .unwrap_or("antigravity");
        let session_capabilities = self.session_adapter.capabilities(source_product);

        DockSnapshot {
            status: ConnectionStatus {
                antigravity_running: running,
                hook_installed: hook_status.hook_installed && hook_status.hook_enabled,
                relay_installed: hook_status.relay_installed,
                ipc_listening: guard.ipc_listening,
                ipc_socket_path: guard.ipc_socket_path.clone(),
                last_event_at_ms: guard.last_event_at_ms,
                vibrancy_mode: guard.vibrancy_mode.clone(),
                shortcut_errors: guard.shortcut_errors.clone(),
            },
            settings: guard.settings.clone(),
            active_event,
            recent_events: guard.recent_events.iter().cloned().collect(),
            transcript_tail: guard.transcript_tail.clone(),
            transcript_error: guard.transcript_error.clone(),
            session_capabilities,
            bubble_pinned: guard.bubble_pinned,
        }
    }

    pub fn emit_snapshot(&self, app_handle: &AppHandle) {
        let snap = self.snapshot();
        let _ = app_handle.emit("dock://state-updated", snap);
    }
}

pub fn load_settings() -> DockSettings {
    let path = paths::settings_path();
    if let Ok(raw) = fs::read_to_string(path) {
        if let Ok(parsed) = serde_json::from_str::<DockSettings>(&raw) {
            return parsed;
        }
    }
    DockSettings::default()
}

pub fn save_settings(settings: &DockSettings) -> Result<(), String> {
    let dir = paths::app_support_dir();
    paths::ensure_dir_with_mode(&dir, 0o700)
        .map_err(|e| format!("Failed to create settings directory: {e}"))?;
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    fs::write(paths::settings_path(), format!("{json}\n"))
        .map_err(|e| format!("Failed to write settings: {e}"))
}

pub fn position_bubble_top_right(win: &WebviewWindow) {
    if let Ok(Some(monitor)) = win.primary_monitor() {
        let size = monitor.size();
        let pos = monitor.position();
        let scale = monitor.scale_factor();
        let win_w = (332.0 * scale) as i32;
        let margin_x = (18.0 * scale) as i32;
        let margin_y = (42.0 * scale) as i32;
        let x = pos.x + size.width as i32 - win_w - margin_x;
        let y = pos.y + margin_y;
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

pub fn position_chat_top_right(win: &WebviewWindow) {
    if let Ok(Some(monitor)) = win.primary_monitor() {
        let size = monitor.size();
        let pos = monitor.position();
        let scale = monitor.scale_factor();
        let win_w = win
            .outer_size()
            .map(|s| s.width as i32)
            .unwrap_or((440.0 * scale) as i32);
        let margin_x = (18.0 * scale) as i32;
        let margin_y = (112.0 * scale) as i32;
        let x = pos.x + size.width as i32 - win_w - margin_x;
        let y = pos.y + margin_y;
        let _ = win.set_position(PhysicalPosition::new(x, y));
    }
}

pub fn show_bubble_without_focus(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("bubble") {
        let already_visible = win.is_visible().unwrap_or(false);
        if !already_visible {
            position_bubble_top_right(&win);
            let _ = win.show();
        }
    }
}

pub fn show_chat_focused(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("chat") {
        if !win.is_visible().unwrap_or(false) {
            position_chat_top_right(&win);
            let _ = win.show();
        }
        let _ = win.set_focus();
    }
}

pub fn toggle_chat_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("chat") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            show_chat_focused(app);
        }
    }
}

pub fn parse_shortcut(raw: &str) -> Result<Shortcut, String> {
    raw.trim()
        .parse::<Shortcut>()
        .map_err(|e| format!("Invalid shortcut '{raw}': {e}"))
}
