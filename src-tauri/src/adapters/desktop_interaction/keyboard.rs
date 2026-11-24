use core_graphics::event::CGEventFlags;
use std::thread;
use std::time::Duration;

use crate::adapters::desktop_interaction::accessibility::{
    verify_submission, LocatedTarget,
};
use crate::adapters::desktop_interaction::ax_ffi::{
    activate_pid, frontmost_app_pid, pasteboard_read_string, pasteboard_write_string,
    post_key_combo, post_mouse_click,
};
use crate::adapters::desktop_interaction::{DesktopSendResult, PermissionStatus, SendOutcomeStatus};

const VK_ANSI_V: u16 = 0x09;
const VK_RETURN: u16 = 0x24;

pub fn try_send_via_keyboard(
    target: &LocatedTarget,
    message: &str,
    perms: PermissionStatus,
) -> DesktopSendResult {
    let previous_pid = frontmost_app_pid();
    let previous_clipboard = pasteboard_read_string();

    if let Err(err) = pasteboard_write_string(message) {
        return DesktopSendResult {
            status: SendOutcomeStatus::SubmissionFailed,
            strategy_used: "keyboard".to_string(),
            message: err,
            verified_submitted: false,
            target_window_title: Some(target.window_title.clone()),
            permission_status: perms,
        };
    }

    let was_minimized = target.window.get_bool("AXMinimized").unwrap_or(false);
    if was_minimized {
        let _ = target.window.set_bool_attr("AXMinimized", false);
        thread::sleep(Duration::from_millis(220));
    }

    let _ = activate_pid(target.pid);
    let _ = target.window.perform_action("AXRaise");
    thread::sleep(Duration::from_millis(140));

    // Focus the composer via AX, falling back to clicking inside its bounds if needed.
    let _ = target.composer.set_bool_attr("AXFocused", true);
    if target.composer.get_bool("AXFocused") != Some(true) {
        if let Some(bounds) = target.composer.get_bounds() {
            let cx = bounds.x + bounds.width * 0.5;
            let cy = bounds.y + bounds.height * 0.5;
            let _ = post_mouse_click(cx, cy);
            thread::sleep(Duration::from_millis(60));
        }
    } else {
        thread::sleep(Duration::from_millis(50));
    }

    // Paste via Cmd+V to preserve Unicode, Vietnamese diacritics, multiline text, and code blocks.
    if let Err(err) = post_key_combo(VK_ANSI_V, CGEventFlags::CGEventFlagCommand) {
        restore_context(previous_pid, previous_clipboard);
        return DesktopSendResult {
            status: SendOutcomeStatus::SubmissionFailed,
            strategy_used: "keyboard".to_string(),
            message: format!("Failed to paste message into Antigravity: {err}"),
            verified_submitted: false,
            target_window_title: Some(target.window_title.clone()),
            permission_status: perms,
        };
    }

    thread::sleep(Duration::from_millis(160));

    // Prefer triggering the Send button via AXPress so Enter never inserts a stray newline.
    let mut submit_triggered = false;
    if let Some(btn) = &target.send_button {
        if btn.get_bool("AXEnabled").unwrap_or(true) && btn.perform_action("AXPress").is_ok() {
            submit_triggered = true;
        } else if let Some(btn_bounds) = btn.get_bounds() {
            let bx = btn_bounds.x + btn_bounds.width * 0.5;
            let by = btn_bounds.y + btn_bounds.height * 0.5;
            if post_mouse_click(bx, by).is_ok() {
                submit_triggered = true;
            }
        }
    }

    // Fallback to Return only if no clickable Send button was available.
    if !submit_triggered {
        let _ = post_key_combo(VK_RETURN, CGEventFlags::empty());
    }

    let verified = verify_submission(&target.composer, target.send_button.as_ref(), message);

    thread::sleep(Duration::from_millis(100));
    restore_context(previous_pid, previous_clipboard);

    if verified {
        DesktopSendResult {
            status: SendOutcomeStatus::Success,
            strategy_used: "keyboard".to_string(),
            message: "Message pasted and submitted to active Antigravity conversation."
                .to_string(),
            verified_submitted: true,
            target_window_title: Some(target.window_title.clone()),
            permission_status: perms,
        }
    } else {
        DesktopSendResult {
            status: SendOutcomeStatus::SubmissionFailed,
            strategy_used: "keyboard".to_string(),
            message: "Message was pasted, but submission could not be verified. Check Antigravity."
                .to_string(),
            verified_submitted: false,
            target_window_title: Some(target.window_title.clone()),
            permission_status: perms,
        }
    }
}

fn restore_context(previous_pid: Option<i32>, previous_clipboard: Option<String>) {
    if let Some(prev_clip) = previous_clipboard {
        let _ = pasteboard_write_string(&prev_clip);
    }
    if let Some(pid) = previous_pid {
        let _ = activate_pid(pid);
    }
}
