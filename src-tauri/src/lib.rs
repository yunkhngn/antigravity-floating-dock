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

use crate::hooks::{HookMutationResult, HookStatus};
use crate::models::{normalize_hook_payload, DockSettings, DockSnapshot};
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
fn show_chat(app: AppHandle) {
    show_chat_focused(&app);
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
        let (w, h) = if expanded { (520.0, 660.0) } else { (440.0, 540.0) };
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
    state.update_settings(settings)?;
    apply_global_shortcuts(&app, &state);
    state.emit_snapshot(&app);
    Ok(state.snapshot())
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
fn open_antigravity(state: State<'_, Arc<AppState>>, source_product: Option<String>) -> Result<(), String> {
    state
        .session_adapter
        .open_antigravity(source_product.as_deref().unwrap_or("antigravity"))
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
                let _ = state_for_menu.session_adapter.open_antigravity("antigravity");
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

                // Sync relay binary into Application Support if available next to executable.
                if let Err(err) = hooks::sync_relay_binary() {
                    state.logger.log("INFO", &format!("Relay sync skipped: {err}"));
                }

                // Configure native macOS vibrancy / Liquid Glass on both floating windows.
                let mut active_mode = "css-fallback".to_string();
                if let Some(bubble) = handle.get_webview_window("bubble") {
                    position_bubble_top_right(&bubble);
                    let _ = bubble.set_visible_on_all_workspaces(true);
                    active_mode = vibrancy::apply_native_surface(&bubble, 24.0);
                    if state.snapshot().bubble_pinned {
                        let _ = bubble.show();
                    }
                }
                if let Some(chat) = handle.get_webview_window("chat") {
                    position_chat_top_right(&chat);
                    let _ = chat.set_visible_on_all_workspaces(true);
                    let chat_mode = vibrancy::apply_native_surface(&chat, 20.0);
                    if active_mode == "css-fallback" {
                        active_mode = chat_mode;
                    }
                }
                state.set_vibrancy_mode(active_mode);

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

                // Low-frequency status monitor (every 10s) to reflect Antigravity open/quit transitions.
                let monitor_state = Arc::clone(&state);
                let monitor_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        monitor_state.refresh_running_status_if_changed(&monitor_handle);
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
