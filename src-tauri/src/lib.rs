mod adapters;
mod diagnostics;
mod hooks;
mod ipc;
mod models;
mod paths;
mod redact;
mod session;
mod state;
mod vibrancy;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, LogicalSize, Manager, RunEvent, Size, State, WindowEvent};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::adapters::desktop_interaction::{
    self, AxUiInspection, DesktopSendRequest, DesktopSendResult, PermissionStatus,
    SendOutcomeStatus,
};
use crate::hooks::{HookMutationResult, HookStatus};
use crate::models::{
    normalize_hook_payload, AttachmentInfo, ConversationSummary, DockSettings, DockSnapshot,
};
use crate::session::SessionAdapter;
use crate::state::{
    parse_shortcut, position_bubble_top_right, position_chat_top_right, show_chat_focused,
    toggle_chat_window, AppState,
};

#[tauri::command]
fn get_snapshot(state: State<'_, Arc<AppState>>) -> DockSnapshot {
    state.snapshot()
}

#[tauri::command]
fn show_chat(app: AppHandle, state: State<'_, Arc<AppState>>) {
    show_chat_focused(&app);
    let state_ref = Arc::clone(state.inner());
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = state_ref.refresh_conversations(Some(&app_clone));
    });
}

#[tauri::command]
fn hide_chat(app: AppHandle) {
    if let Some(win) = app.get_webview_window("chat") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn toggle_chat(app: AppHandle) {
    toggle_chat_window(&app);
}

#[tauri::command]
fn set_chat_expanded(app: AppHandle, expanded: bool) {
    if let Some(win) = app.get_webview_window("chat") {
        let (w, h) = if expanded { (560.0, 700.0) } else { (472.0, 584.0) };
        let _ = win.set_size(Size::Logical(LogicalSize::new(w, h)));
    }
}

#[tauri::command]
fn hide_bubble(app: AppHandle, state: State<'_, Arc<AppState>>) {
    state.set_bubble_pinned(false, Some(&app));
    if let Some(win) = app.get_webview_window("bubble") {
        let _ = win.hide();
    }
}

#[tauri::command]
fn set_bubble_pinned(app: AppHandle, state: State<'_, Arc<AppState>>, pinned: bool) {
    state.set_bubble_pinned(pinned, Some(&app));
    if !pinned {
        if let Some(win) = app.get_webview_window("bubble") {
            let _ = win.hide();
        }
    }
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: DockSettings,
) -> Result<DockSnapshot, String> {
    let style = settings.glass_style.clone();
    state.update_settings(settings)?;
    apply_global_shortcuts(&app, &state);
    apply_surface_to_windows(&app, &state, &style);
    state.emit_snapshot(&app);
    Ok(state.snapshot())
}

fn apply_surface_to_windows(app: &AppHandle, state: &Arc<AppState>, glass_style: &str) {
    let mut active_mode = "css-fallback".to_string();
    if let Some(bubble) = app.get_webview_window("bubble") {
        active_mode = vibrancy::apply_native_surface(&bubble, 28.0, glass_style);
    }
    if let Some(chat) = app.get_webview_window("chat") {
        let chat_mode = vibrancy::apply_native_surface(&chat, 24.0, glass_style);
        if active_mode == "css-fallback" {
            active_mode = chat_mode;
        }
    }
    state.set_vibrancy_mode(active_mode);
}

#[tauri::command]
fn verify_hooks() -> HookStatus {
    hooks::inspect_hook_status()
}

#[tauri::command]
fn install_hooks(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<HookMutationResult, String> {
    let result = hooks::install_hook()?;
    state.logger.log("INFO", "Installed Antigravity hook configuration");
    state.emit_snapshot(&app);
    Ok(result)
}

#[tauri::command]
fn uninstall_hooks(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<HookMutationResult, String> {
    let result = hooks::uninstall_hook()?;
    state.logger.log("INFO", "Removed Antigravity Floating Dock hook entry");
    state.emit_snapshot(&app);
    Ok(result)
}

#[tauri::command]
fn open_antigravity(
    state: State<'_, Arc<AppState>>,
    source_product: Option<String>,
    conversation_id: Option<String>,
) -> Result<(), String> {
    state.session_adapter.open_antigravity(
        source_product.as_deref().unwrap_or("antigravity"),
        conversation_id.as_deref(),
    )
}

#[tauri::command]
async fn send_message_to_antigravity(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request: DesktopSendRequest,
) -> Result<DesktopSendResult, String> {
    if state.should_queue_for_target(request.expected_conversation_id.as_deref()) {
        let item = state.enqueue_message(&request, Some(&app))?;
        state.logger.log(
            "INFO",
            &format!("Queued message id={} chars={}", item.id, request.message.chars().count()),
        );
        return Ok(DesktopSendResult {
            status: SendOutcomeStatus::Queued,
            strategy_used: "queued".to_string(),
            message: "Message queued; will send automatically when the current turn finishes."
                .to_string(),
            verified_submitted: false,
            target_window_title: request.expected_conversation_title.clone(),
            permission_status: desktop_interaction::check_permissions(),
        });
    }

    let state_ref = Arc::clone(state.inner());
    let msg_chars = request.message.chars().count();
    let result = tokio::task::spawn_blocking(move || state_ref.desktop_adapter.send_message(&request))
        .await
        .map_err(|e| format!("Desktop automation task failed: {e}"))?;

    state.logger.log(
        "INFO",
        &format!(
            "Desktop send status={:?} strategy={} chars={} verified={}",
            result.status, result.strategy_used, msg_chars, result.verified_submitted
        ),
    );
    state.emit_snapshot(&app);
    Ok(result)
}

#[tauri::command]
async fn flush_queue_item(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    queue_id: String,
) -> Result<DesktopSendResult, String> {
    let state_ref = Arc::clone(state.inner());
    state_ref.flush_queued_item(&queue_id, Some(&app)).await
}

#[tauri::command]
fn remove_queue_item(app: AppHandle, state: State<'_, Arc<AppState>>, queue_id: String) -> bool {
    state.remove_queued_item(&queue_id, Some(&app))
}

#[tauri::command]
fn clear_queue(app: AppHandle, state: State<'_, Arc<AppState>>) {
    state.clear_queue(Some(&app));
}

#[tauri::command]
fn check_desktop_permissions() -> PermissionStatus {
    desktop_interaction::check_permissions()
}

#[tauri::command]
fn request_accessibility_permission(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> PermissionStatus {
    let perms = desktop_interaction::request_accessibility_permission();
    state.emit_snapshot(&app);
    perms
}

#[tauri::command]
async fn inspect_antigravity_ui(source_product: Option<String>) -> Result<AxUiInspection, String> {
    tokio::task::spawn_blocking(move || desktop_interaction::inspect_ui(source_product.as_deref()))
        .await
        .map_err(|e| format!("UI inspection failed: {e}"))
}

#[tauri::command]
async fn refresh_conversations(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<ConversationSummary>, String> {
    let state_ref = Arc::clone(state.inner());
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || state_ref.refresh_conversations(Some(&app_clone)))
        .await
        .map_err(|e| format!("Failed to list conversations: {e}"))?
}

#[tauri::command]
fn select_conversation(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    conversation_id: Option<String>,
) -> DockSnapshot {
    state.select_conversation(conversation_id, Some(&app))
}

#[tauri::command]
fn save_attachment_image(filename: String, base64_data: String) -> Result<AttachmentInfo, String> {
    const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
    const MAX_STORED_ATTACHMENTS: usize = 30;

    let clean_b64 = base64_data
        .split_once(",")
        .map(|(_, rest)| rest)
        .unwrap_or(&base64_data);
    let bytes = decode_base64(clean_b64)?;
    if bytes.is_empty() {
        return Err("Image attachment is empty.".to_string());
    }
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("Image exceeds 10 MiB limit.".to_string());
    }

    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif"))
        .unwrap_or_else(|| "png".to_string());

    let stem: String = std::path::Path::new(&filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(36)
        .collect();
    let safe_stem = if stem.trim_matches('-').is_empty() {
        "image"
    } else {
        stem.trim_matches('-')
    };

    let dir = paths::attachments_dir();
    paths::ensure_dir_with_mode(&dir, 0o700)
        .map_err(|e| format!("Failed to create attachments directory: {e}"))?;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let stored_name = format!("{ts}-{safe_stem}.{ext}");
    let dest = dir.join(&stored_name);

    std::fs::write(&dest, &bytes).map_err(|e| format!("Failed to save attachment: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600));
    }

    // Prune older attachments to keep disk bounded.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                let m = e.metadata().ok()?;
                if m.is_file() {
                    Some((p, m.modified().ok()))
                } else {
                    None
                }
            })
            .collect();
        if files.len() > MAX_STORED_ATTACHMENTS {
            files.sort_by(|a, b| a.1.cmp(&b.1));
            for (old_path, _) in files.iter().take(files.len() - MAX_STORED_ATTACHMENTS) {
                let _ = std::fs::remove_file(old_path);
            }
        }
    }

    Ok(AttachmentInfo {
        path: dest.display().to_string(),
        name: stored_name,
        size_bytes: bytes.len(),
    })
}

fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity((input.len() / 4) * 3);
    let mut buf = 0u32;
    let mut bits = 0u8;

    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let val = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("Invalid base64 image data.".to_string()),
        };
        buf = (buf << 6) | u32::from(val);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::decode_base64;

    #[test]
    fn decodes_valid_base64_and_rejects_invalid() {
        assert_eq!(decode_base64("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(decode_base64("YWJj\n").unwrap(), b"abc");
        assert!(decode_base64("invalid!@#").is_err());
    }
}

#[tauri::command]
fn emit_sample_event(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    kind: String,
) -> Result<(), String> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let payload = match kind.as_str() {
        "working" => serde_json::json!({
            "hookEventName": "PostToolUse",
            "conversationId": "sample-session",
            "modelName": "gemini-preview",
            "toolCall": { "id": "sample-1", "name": "edit_file" },
            "workspacePaths": [paths::home_dir().display().to_string()],
            "transcriptPath": format!("{}/.gemini/antigravity/sample-missing.jsonl", paths::home_dir().display())
        }),
        "error" => serde_json::json!({
            "hookEventName": "Stop",
            "conversationId": "sample-session",
            "terminationReason": "ERROR",
            "error": "Sample simulated tool/agent error"
        }),
        _ => serde_json::json!({
            "hookEventName": "Stop",
            "conversationId": "sample-session",
            "terminationReason": "NO_TOOL_CALL",
            "fullyIdle": true
        }),
    };

    let event = normalize_hook_payload(&payload, ts, state.next_seq())?;
    state.ingest_event(event, Some(&app));
    Ok(())
}

#[tauri::command]
fn get_diagnostics(state: State<'_, Arc<AppState>>) -> Vec<String> {
    state.logger.recent_lines()
}

#[tauri::command]
fn quit_app(app: AppHandle, state: State<'_, Arc<AppState>>) {
    state.allow_exit.store(true, Ordering::SeqCst);
    ipc::cleanup_ipc_files();
    app.exit(0);
}

fn apply_global_shortcuts(app: &AppHandle, state: &Arc<AppState>) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    let settings = state.settings();
    let mut errors = Vec::new();
    let mut chat_id = None;
    let mut bubble_id = None;

    match parse_shortcut(&settings.toggle_chat_shortcut) {
        Ok(shortcut) => {
            if let Err(err) = gs.register(shortcut) {
                let msg = format!(
                    "Could not register chat shortcut '{}': {err}",
                    settings.toggle_chat_shortcut
                );
                state.logger.log("WARN", &msg);
                errors.push(msg);
            } else {
                chat_id = Some(shortcut.id());
            }
        }
        Err(err) => errors.push(err),
    }

    if !settings.toggle_bubble_shortcut.trim().is_empty() {
        match parse_shortcut(&settings.toggle_bubble_shortcut) {
            Ok(shortcut) => {
                if let Err(err) = gs.register(shortcut) {
                    let msg = format!(
                        "Could not register bubble shortcut '{}': {err}",
                        settings.toggle_bubble_shortcut
                    );
                    state.logger.log("WARN", &msg);
                    errors.push(msg);
                } else {
                    bubble_id = Some(shortcut.id());
                }
            }
            Err(err) => errors.push(err),
        }
    }

    state.set_shortcut_registration(chat_id, bubble_id, errors);
}

fn setup_tray(app: &AppHandle, state: &Arc<AppState>) -> tauri::Result<()> {
    let toggle_chat_item = MenuItem::with_id(app, "toggle_chat", "Toggle Floating Chat", true, None::<&str>)?;
    let toggle_bubble_item =
        MenuItem::with_id(app, "toggle_bubble", "Pin / Unpin Status Bubble", true, None::<&str>)?;
    let open_agy_item = MenuItem::with_id(app, "open_agy", "Open Antigravity", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Antigravity Floating Dock", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&toggle_chat_item, &toggle_bubble_item, &open_agy_item, &quit_item],
    )?;

    let icon = tauri::include_image!("icons/tray-template.png");

    let state_for_menu = Arc::clone(state);
    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Antigravity Floating Dock")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "toggle_chat" => toggle_chat_window(app),
            "toggle_bubble" => {
                let currently_pinned = state_for_menu.snapshot().bubble_pinned;
                let next = !currently_pinned;
                state_for_menu.set_bubble_pinned(next, Some(app));
                if !next {
                    if let Some(win) = app.get_webview_window("bubble") {
                        let _ = win.hide();
                    }
                }
            }
            "open_agy" => {
                let cid = state_for_menu
                    .snapshot()
                    .active_event
                    .and_then(|e| e.conversation_id);
                let _ = state_for_menu
                    .session_adapter
                    .open_antigravity("antigravity", cid.as_deref());
            }
            "quit" => {
                state_for_menu.allow_exit.store(true, Ordering::SeqCst);
                ipc::cleanup_ipc_files();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shared_state = Arc::new(AppState::new());
    let state_for_shortcut = Arc::clone(&shared_state);

    let app = tauri::Builder::default()
        .manage(Arc::clone(&shared_state))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    if event.state() != tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        return;
                    }
                    let (chat_id, bubble_id) = state_for_shortcut.shortcut_ids();
                    if Some(shortcut.id()) == chat_id {
                        toggle_chat_window(app);
                    } else if Some(shortcut.id()) == bubble_id {
                        let currently_pinned = state_for_shortcut.snapshot().bubble_pinned;
                        let next = !currently_pinned;
                        state_for_shortcut.set_bubble_pinned(next, Some(app));
                        if !next {
                            if let Some(win) = app.get_webview_window("bubble") {
                                let _ = win.hide();
                            }
                        }
                    }
                })
                .build(),
        )
        .on_window_event(|window, event| {
            // Closing a floating window hides it instead of terminating the background companion.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            show_chat,
            hide_chat,
            toggle_chat,
            set_chat_expanded,
            hide_bubble,
            set_bubble_pinned,
            update_settings,
            verify_hooks,
            install_hooks,
            uninstall_hooks,
            open_antigravity,
            send_message_to_antigravity,
            flush_queue_item,
            remove_queue_item,
            clear_queue,
            check_desktop_permissions,
            request_accessibility_permission,
            inspect_antigravity_ui,
            refresh_conversations,
            select_conversation,
            save_attachment_image,
            emit_sample_event,
            get_diagnostics,
            quit_app,
        ])
        .setup({
            let state = Arc::clone(&shared_state);
            move |app| {
                #[cfg(target_os = "macos")]
                {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }

                let handle = app.handle().clone();

                // Sync relay binary and existing sidecar script (only if plugin was already installed by user).
                if let Err(err) = hooks::sync_relay_binary() {
                    state.logger.log("INFO", &format!("Relay sync skipped: {err}"));
                }
                if adapters::sidecar_bridge::plugin_manifest_path().is_file() {
                    let _ = adapters::sidecar_bridge::install_sidecar();
                }

                // Configure native macOS vibrancy / Liquid Glass on both floating windows.
                let glass_style = state.settings().glass_style;
                if let Some(bubble) = handle.get_webview_window("bubble") {
                    position_bubble_top_right(&bubble);
                    let _ = bubble.set_visible_on_all_workspaces(true);
                    if state.snapshot().bubble_pinned {
                        let _ = bubble.show();
                    }
                }
                if let Some(chat) = handle.get_webview_window("chat") {
                    position_chat_top_right(&chat);
                    let _ = chat.set_visible_on_all_workspaces(true);
                }
                apply_surface_to_windows(&handle, &state, &glass_style);

                // Preload Antigravity sessions and latest transcript immediately on launch.
                let _ = state.refresh_conversations(None);

                setup_tray(&handle, &state)?;
                apply_global_shortcuts(&handle, &state);

                // Start local Unix socket IPC server.
                let ipc_state = Arc::clone(&state);
                let ipc_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = ipc::start_ipc_server(ipc_handle.clone(), ipc_state.clone()).await {
                        ipc_state.logger.log("ERROR", &err);
                    }
                    ipc_state.emit_snapshot(&ipc_handle);
                });

                // Low-frequency status & live-transcript monitor.
                let monitor_state = Arc::clone(&state);
                let monitor_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let mut tick: u32 = 0;
                    loop {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        tick = tick.wrapping_add(1);
                        let chat_visible = monitor_handle
                            .get_webview_window("chat")
                            .and_then(|w| w.is_visible().ok())
                            .unwrap_or(false);
                        if chat_visible {
                            monitor_state.poll_active_transcript_if_working(&monitor_handle);
                        }
                        if tick % 5 == 0 {
                            monitor_state.refresh_running_status_if_changed(&monitor_handle);
                        }
                    }
                });

                Ok(())
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Antigravity Floating Dock");

    app.run(move |_app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            if !shared_state.allow_exit.load(Ordering::SeqCst) {
                api.prevent_exit();
            } else {
                ipc::cleanup_ipc_files();
            }
        }
    });
}
