use std::collections::{HashMap, VecDeque};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};
use tauri_plugin_global_shortcut::Shortcut;

use crate::adapters::desktop_interaction::{
    DesktopInteractionAdapter, DesktopSendRequest, DesktopSendResult, SendOutcomeStatus,
};
use crate::adapters::sidecar_bridge;
use crate::diagnostics::DiagnosticLogger;
use crate::hooks;
use crate::models::{
    AgentActivityState, ConnectionStatus, ConversationSummary, DockEvent, DockEventType,
    DockSettings, DockSnapshot, QueuedMessage, QueuedMessageStatus, TranscriptMessage,
};
use crate::paths;
use crate::redact::redact_string;
use crate::session::{AntigravitySessionAdapter, SessionAdapter};

const MAX_RECENT_EVENTS: usize = 50;
const MAX_QUEUED_MESSAGES: usize = 20;
const DEDUPE_WINDOW_MS: u64 = 1500;

struct InnerState {
    settings: DockSettings,
    recent_events: VecDeque<DockEvent>,
    selected_conversation_id: Option<String>,
    conversations: Vec<ConversationSummary>,
    queued_messages: Vec<QueuedMessage>,
    queue_flushing: bool,
    transcript_paths_by_cid: HashMap<String, String>,
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
    pub desktop_adapter: DesktopInteractionAdapter,
}

impl AppState {
    pub fn new() -> Self {
        let settings = load_settings();
        let bubble_pinned = settings.pin_bubble;
        Self {
            inner: RwLock::new(InnerState {
                settings,
                recent_events: VecDeque::with_capacity(MAX_RECENT_EVENTS),
                selected_conversation_id: None,
                conversations: Vec::new(),
                queued_messages: Vec::new(),
                queue_flushing: false,
                transcript_paths_by_cid: HashMap::new(),
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
            desktop_adapter: DesktopInteractionAdapter::new(),
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
        let mut flush_candidate_id: Option<String> = None;

        let resolved_transcript_path = event.transcript_path.clone().or_else(|| {
            event.conversation_id.as_deref().and_then(|cid| {
                self.inner
                    .read()
                    .ok()
                    .and_then(|g| g.transcript_paths_by_cid.get(cid).cloned())
                    .or_else(|| self.session_adapter.resolve_transcript_for_conversation(cid))
            })
        });

        let transcript_result = resolved_transcript_path
            .as_deref()
            .map(|p| self.session_adapter.read_transcript_tail(p, 24));

        let is_clean_stop = event.event_type == DockEventType::Stop
            && matches!(event.termination_reason.as_deref(), Some("NO_TOOL_CALL") | None)
            && event.activity_state != AgentActivityState::Error;
        let event_cid = event.conversation_id.clone();

        if let Ok(mut g) = self.inner.write() {
            if let (Some(cid), Some(tpath)) = (
                event.conversation_id.as_ref(),
                event.transcript_path.as_ref(),
            ) {
                if g.transcript_paths_by_cid.len() >= MAX_RECENT_EVENTS {
                    g.transcript_paths_by_cid.clear();
                }
                g.transcript_paths_by_cid
                    .insert(cid.clone(), tpath.clone());
            }

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

                let should_update_transcript = g.selected_conversation_id.is_none()
                    || g.selected_conversation_id.as_deref() == event.conversation_id.as_deref();
                if should_update_transcript {
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

                if is_clean_stop && !g.queue_flushing {
                    flush_candidate_id = g
                        .queued_messages
                        .iter()
                        .find(|m| {
                            m.status == QueuedMessageStatus::Pending
                                && (m.conversation_id.is_none()
                                    || event_cid.is_none()
                                    || m.conversation_id == event_cid)
                        })
                        .map(|m| m.id.clone());
                }
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
                if let Some(qid) = flush_candidate_id {
                    let state_ref = Arc::clone(self);
                    let app_clone = app.clone();
                    tauri::async_runtime::spawn(async move {
                        // Brief settle delay so Antigravity finishes transitioning turn state.
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        let _ = state_ref.flush_queued_item(&qid, Some(&app_clone)).await;
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

    pub fn poll_active_transcript_if_working(&self, app_handle: &AppHandle) {
        let target_cid = {
            let Ok(g) = self.inner.read() else {
                return;
            };
            let is_working = g
                .recent_events
                .front()
                .map(|e| e.activity_state == AgentActivityState::Working)
                .unwrap_or(false)
                || g.conversations
                    .iter()
                    .any(|c| c.run_state == "RUN_STATE_RUNNING");
            if !is_working {
                return;
            }
            g.selected_conversation_id.clone().or_else(|| {
                g.recent_events
                    .front()
                    .and_then(|e| e.conversation_id.clone())
                    .or_else(|| g.conversations.first().map(|c| c.id.clone()))
            })
        };

        let Some(cid) = target_cid else {
            return;
        };
        let path_opt = self
            .inner
            .read()
            .ok()
            .and_then(|g| g.transcript_paths_by_cid.get(&cid).cloned())
            .or_else(|| self.session_adapter.resolve_transcript_for_conversation(&cid));
        let Some(path) = path_opt else {
            return;
        };
        let Ok(items) = self.session_adapter.read_transcript_tail(&path, 24) else {
            return;
        };

        let changed = if let Ok(mut g) = self.inner.write() {
            let prev_len = g.transcript_tail.len();
            let prev_last_text = g.transcript_tail.last().map(|m| m.text.clone());
            let prev_last_steps = g.transcript_tail.last().map(|m| m.steps.len());
            let next_last_text = items.last().map(|m| m.text.clone());
            let next_last_steps = items.last().map(|m| m.steps.len());
            let differs = prev_len != items.len()
                || prev_last_text != next_last_text
                || prev_last_steps != next_last_steps;
            if differs {
                g.transcript_tail = items;
                g.transcript_error = None;
            }
            differs
        } else {
            false
        };

        if changed {
            self.emit_snapshot(app_handle);
        }
    }

    pub fn refresh_conversations(
        &self,
        app_handle: Option<&AppHandle>,
    ) -> Result<Vec<ConversationSummary>, String> {
        let local_entries = self.session_adapter.list_local_conversations(30);
        let remote_list = sidecar_bridge::list_conversations().ok();

        let mut merged: Vec<ConversationSummary> = Vec::new();
        let mut by_id: HashMap<String, ConversationSummary> = HashMap::new();
        let mut ordered_ids: Vec<String> = Vec::new();

        if let Some(remote) = remote_list {
            for item in remote {
                if !by_id.contains_key(&item.id) {
                    ordered_ids.push(item.id.clone());
                }
                by_id.insert(item.id.clone(), item);
            }
        }

        for (local, tpath) in &local_entries {
            if let Ok(mut g) = self.inner.write() {
                if g.transcript_paths_by_cid.len() >= MAX_RECENT_EVENTS {
                    g.transcript_paths_by_cid.clear();
                }
                g.transcript_paths_by_cid
                    .entry(local.id.clone())
                    .or_insert_with(|| tpath.clone());
            }
            if !by_id.contains_key(&local.id) {
                ordered_ids.push(local.id.clone());
                by_id.insert(local.id.clone(), local.clone());
            }
        }

        for id in ordered_ids {
            if let Some(item) = by_id.remove(&id) {
                merged.push(item);
            }
        }

        let initial_target_path = {
            let guard = self.inner.read().expect("state lock poisoned");
            if guard.selected_conversation_id.is_none()
                && guard.transcript_tail.is_empty()
                && guard.recent_events.is_empty()
            {
                merged.first().and_then(|c| {
                    guard
                        .transcript_paths_by_cid
                        .get(&c.id)
                        .cloned()
                        .or_else(|| self.session_adapter.resolve_transcript_for_conversation(&c.id))
                })
            } else {
                None
            }
        };

        let initial_transcript = initial_target_path
            .as_deref()
            .and_then(|p| self.session_adapter.read_transcript_tail(p, 16).ok());

        if let Ok(mut g) = self.inner.write() {
            g.conversations = merged.clone();
            if g.transcript_tail.is_empty() {
                if let Some(items) = initial_transcript {
                    g.transcript_tail = items;
                    g.transcript_error = None;
                }
            }
        }
        if let Some(app) = app_handle {
            self.emit_snapshot(app);
        }
        Ok(merged)
    }

    pub fn select_conversation(
        &self,
        conversation_id: Option<String>,
        app_handle: Option<&AppHandle>,
    ) -> DockSnapshot {
        let cleaned = conversation_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let target_cid = cleaned.clone().or_else(|| {
            self.inner.read().ok().and_then(|g| {
                g.recent_events
                    .front()
                    .and_then(|e| e.conversation_id.clone())
                    .or_else(|| g.conversations.first().map(|c| c.id.clone()))
            })
        });

        let known_path = target_cid.as_deref().and_then(|cid| {
            self.inner
                .read()
                .ok()
                .and_then(|g| g.transcript_paths_by_cid.get(cid).cloned())
                .or_else(|| self.session_adapter.resolve_transcript_for_conversation(cid))
        });

        let transcript_result = known_path
            .as_deref()
            .map(|p| self.session_adapter.read_transcript_tail(p, 16));

        if let Ok(mut g) = self.inner.write() {
            g.selected_conversation_id = cleaned;
            match transcript_result {
                Some(Ok(items)) => {
                    g.transcript_tail = items;
                    g.transcript_error = None;
                }
                Some(Err(err)) => {
                    g.transcript_tail.clear();
                    g.transcript_error = Some(err);
                }
                None => {
                    g.transcript_tail.clear();
                    g.transcript_error = if target_cid.is_some() {
                        Some("No JSONL transcript file found for this session yet.".to_string())
                    } else {
                        None
                    };
                }
            }
        }

        if let Some(app) = app_handle {
            self.emit_snapshot(app);
        }
        self.snapshot()
    }

    pub fn snapshot(&self) -> DockSnapshot {
        let hook_status = hooks::inspect_hook_status();
        let sidecar_status = sidecar_bridge::inspect_sidecar_status();
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
                sidecar_installed: sidecar_status.installed,
                sidecar_connected: sidecar_status.connected,
                ipc_listening: guard.ipc_listening,
                ipc_socket_path: guard.ipc_socket_path.clone(),
                last_event_at_ms: guard.last_event_at_ms,
                vibrancy_mode: guard.vibrancy_mode.clone(),
                shortcut_errors: guard.shortcut_errors.clone(),
            },
            settings: guard.settings.clone(),
            active_event,
            recent_events: guard.recent_events.iter().cloned().collect(),
            selected_conversation_id: guard.selected_conversation_id.clone(),
            conversations: guard.conversations.clone(),
            queued_messages: guard.queued_messages.clone(),
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

    pub fn should_queue_for_target(&self, target_cid: Option<&str>) -> bool {
        let Ok(g) = self.inner.read() else {
            return false;
        };
        if g.queue_flushing {
            return true;
        }
        let has_pending_for_target = g.queued_messages.iter().any(|m| {
            matches!(
                m.status,
                QueuedMessageStatus::Pending | QueuedMessageStatus::Sending
            ) && (m.conversation_id.as_deref() == target_cid
                || m.conversation_id.is_none()
                || target_cid.is_none())
        });
        if has_pending_for_target {
            return true;
        }
        if let Some(cid) = target_cid {
            if g.conversations
                .iter()
                .any(|c| c.id == cid && c.run_state == "RUN_STATE_RUNNING")
            {
                return true;
            }
            if let Some(latest) = g.recent_events.front() {
                if latest.conversation_id.as_deref() == Some(cid)
                    && latest.activity_state == AgentActivityState::Working
                {
                    return true;
                }
            }
        } else if let Some(latest) = g.recent_events.front() {
            if latest.activity_state == AgentActivityState::Working {
                return true;
            }
        }
        false
    }

    pub fn enqueue_message(
        &self,
        req: &DesktopSendRequest,
        app_handle: Option<&AppHandle>,
    ) -> Result<QueuedMessage, String> {
        let trimmed = req.message.trim();
        if trimmed.is_empty() {
            return Err("Cannot queue an empty message.".to_string());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let seq = self.next_seq();
        let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
        let preview = redact_string(&if first_line.chars().count() > 72 {
            let cut: String = first_line.chars().take(72).collect();
            format!("{cut}…")
        } else {
            first_line.to_string()
        });

        let item = QueuedMessage {
            id: format!("q-{now}-{seq}"),
            conversation_id: req.expected_conversation_id.clone(),
            conversation_title: req.expected_conversation_title.clone(),
            workspace_path: req.expected_workspace_path.clone(),
            source_product: req.source_product.clone(),
            message: req.message.clone(),
            preview,
            created_at_ms: now,
            status: QueuedMessageStatus::Pending,
            error: None,
        };

        {
            let mut g = self.inner.write().expect("state lock poisoned");
            if g.queued_messages.len() >= MAX_QUEUED_MESSAGES {
                return Err(format!(
                    "Queue limit reached ({MAX_QUEUED_MESSAGES} messages)."
                ));
            }
            g.queued_messages.push(item.clone());
        }

        if let Some(app) = app_handle {
            self.emit_snapshot(app);
        }
        Ok(item)
    }

    pub fn remove_queued_item(&self, queue_id: &str, app_handle: Option<&AppHandle>) -> bool {
        let removed = if let Ok(mut g) = self.inner.write() {
            let before = g.queued_messages.len();
            g.queued_messages
                .retain(|m| m.id != queue_id || m.status == QueuedMessageStatus::Sending);
            g.queued_messages.len() != before
        } else {
            false
        };
        if removed {
            if let Some(app) = app_handle {
                self.emit_snapshot(app);
            }
        }
        removed
    }

    pub fn clear_queue(&self, app_handle: Option<&AppHandle>) {
        if let Ok(mut g) = self.inner.write() {
            g.queued_messages
                .retain(|m| m.status == QueuedMessageStatus::Sending);
        }
        if let Some(app) = app_handle {
            self.emit_snapshot(app);
        }
    }

    pub async fn flush_queued_item(
        self: &Arc<Self>,
        queue_id: &str,
        app_handle: Option<&AppHandle>,
    ) -> Result<DesktopSendResult, String> {
        let req_opt = {
            let mut g = self.inner.write().expect("state lock poisoned");
            if g.queue_flushing {
                return Err("Queue flush already in progress.".to_string());
            }
            let Some(item) = g.queued_messages.iter_mut().find(|m| m.id == queue_id) else {
                return Err("Queued message no longer exists.".to_string());
            };
            if item.status == QueuedMessageStatus::Sending {
                return Err("Message is already sending.".to_string());
            }
            item.status = QueuedMessageStatus::Sending;
            item.error = None;
            let req = DesktopSendRequest {
                message: item.message.clone(),
                expected_conversation_id: item.conversation_id.clone(),
                expected_conversation_title: item.conversation_title.clone(),
                expected_workspace_path: item.workspace_path.clone(),
                source_product: item.source_product.clone(),
                idempotency_key: Some(item.id.clone()),
                allow_keyboard_fallback: true,
                allow_coordinate_fallback: true,
            };
            g.queue_flushing = true;
            req
        };

        if let Some(app) = app_handle {
            self.emit_snapshot(app);
        }

        let state_ref = Arc::clone(self);
        let msg_chars = req_opt.message.chars().count();
        let result = tokio::task::spawn_blocking(move || {
            state_ref.desktop_adapter.send_message(&req_opt)
        })
        .await
        .map_err(|e| {
            if let Ok(mut g) = self.inner.write() {
                g.queue_flushing = false;
                if let Some(item) = g.queued_messages.iter_mut().find(|m| m.id == queue_id) {
                    item.status = QueuedMessageStatus::Failed;
                    item.error = Some(e.to_string());
                }
            }
            format!("Queue send failed: {e}")
        })?;

        self.logger.log(
            "INFO",
            &format!(
                "Queue send status={:?} strategy={} chars={} verified={}",
                result.status, result.strategy_used, msg_chars, result.verified_submitted
            ),
        );

        {
            let mut g = self.inner.write().expect("state lock poisoned");
            g.queue_flushing = false;
            if result.status == SendOutcomeStatus::Success && result.verified_submitted {
                g.queued_messages.retain(|m| m.id != queue_id);
            } else if let Some(item) = g.queued_messages.iter_mut().find(|m| m.id == queue_id) {
                item.status = QueuedMessageStatus::Failed;
                item.error = Some(result.message.clone());
            }
        }

        if let Some(app) = app_handle {
            self.emit_snapshot(app);
        }

        Ok(result)
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
        let win_w = (364.0 * scale) as i32;
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
            .unwrap_or((472.0 * scale) as i32);
        let margin_x = (18.0 * scale) as i32;
        let margin_y = (118.0 * scale) as i32;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_enqueues_removes_and_clears_in_fifo_order() {
        let state = AppState::new();
        let req1 = DesktopSendRequest {
            message: "first queued prompt".to_string(),
            expected_conversation_id: Some("conv-1".to_string()),
            expected_conversation_title: None,
            expected_workspace_path: None,
            source_product: Some("antigravity".to_string()),
            idempotency_key: None,
            allow_keyboard_fallback: true,
            allow_coordinate_fallback: true,
        };
        let req2 = DesktopSendRequest {
            message: "second queued prompt".to_string(),
            ..req1.clone()
        };

        let item1 = state.enqueue_message(&req1, None).unwrap();
        let item2 = state.enqueue_message(&req2, None).unwrap();
        assert_eq!(state.snapshot().queued_messages.len(), 2);
        assert_eq!(state.snapshot().queued_messages[0].id, item1.id);
        assert_eq!(state.snapshot().queued_messages[1].id, item2.id);

        assert!(state.remove_queued_item(&item1.id, None));
        assert_eq!(state.snapshot().queued_messages.len(), 1);
        assert_eq!(state.snapshot().queued_messages[0].id, item2.id);

        state.clear_queue(None);
        assert!(state.snapshot().queued_messages.is_empty());
    }
}
