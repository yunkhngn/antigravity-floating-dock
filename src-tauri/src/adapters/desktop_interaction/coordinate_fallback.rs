use core_graphics::event::CGEventFlags;
use std::thread;
use std::time::Duration;

use crate::adapters::desktop_interaction::ax_ffi::{
    activate_pid, frontmost_app_pid, list_cg_windows_for_pid, pasteboard_read_string,
    pasteboard_write_string, post_key_combo, post_mouse_click, WindowBounds,
};
use crate::adapters::desktop_interaction::{DesktopSendResult, PermissionStatus, SendOutcomeStatus};

const VK_ANSI_V: u16 = 0x09;

pub fn calculate_relative_targets(bounds: WindowBounds) -> ((f64, f64), (f64, f64)) {
    let composer_x = (bounds.x + bounds.width * 0.55).clamp(bounds.x + 24.0, bounds.x + bounds.width - 40.0);
    let composer_y = (bounds.y + bounds.height - 84.0).clamp(bounds.y + 80.0, bounds.y + bounds.height - 24.0);

    let send_x = (bounds.x + bounds.width - 44.0).clamp(bounds.x + 24.0, bounds.x + bounds.width - 16.0);
    let send_y = (bounds.y + bounds.height - 52.0).clamp(bounds.y + 80.0, bounds.y + bounds.height - 16.0);

    ((composer_x, composer_y), (send_x, send_y))
}

pub fn try_send_via_coordinates(
    pid: i32,
    expected_window_title: Option<&str>,
    message: &str,
    perms: PermissionStatus,
) -> DesktopSendResult {
    let windows = list_cg_windows_for_pid(pid);
    if windows.is_empty() {
        return DesktopSendResult {
            status: SendOutcomeStatus::ComposerNotFound,
            strategy_used: "coordinate-fallback".to_string(),
            message: "Could not obtain Antigravity window bounds for coordinate fallback."
                .to_string(),
            verified_submitted: false,
            target_window_title: expected_window_title.map(ToString::to_string),
            permission_status: perms,
        };
    }

    if windows.len() > 1 && expected_window_title.is_none() {
        return DesktopSendResult {
            status: SendOutcomeStatus::TargetAmbiguous,
            strategy_used: "coordinate-fallback".to_string(),
            message: "Multiple Antigravity windows present; refusing blind coordinate click."
                .to_string(),
            verified_submitted: false,
            target_window_title: None,
            permission_status: perms,
        };
    }

    let target_win = if let Some(exp) = expected_window_title {
        windows
            .iter()
            .find(|w| w.title == exp || w.title.contains(exp))
            .unwrap_or(&windows[0])
    } else {
        &windows[0]
    };

    let previous_pid = frontmost_app_pid();
    let previous_clipboard = pasteboard_read_string();

    if let Err(err) = pasteboard_write_string(message) {
        return DesktopSendResult {
            status: SendOutcomeStatus::SubmissionFailed,
            strategy_used: "coordinate-fallback".to_string(),
            message: err,
            verified_submitted: false,
            target_window_title: Some(target_win.title.clone()),
            permission_status: perms,
        };
    }

    let ((composer_x, composer_y), (send_x, send_y)) = calculate_relative_targets(target_win.bounds);

    let _ = activate_pid(pid);
    thread::sleep(Duration::from_millis(160));

    let _ = post_mouse_click(composer_x, composer_y);
    thread::sleep(Duration::from_millis(80));
    let _ = post_key_combo(VK_ANSI_V, CGEventFlags::CGEventFlagCommand);
    thread::sleep(Duration::from_millis(140));
    let _ = post_mouse_click(send_x, send_y);
    thread::sleep(Duration::from_millis(120));

    if let Some(prev_clip) = previous_clipboard {
        let _ = pasteboard_write_string(&prev_clip);
    }
    if let Some(prev_pid) = previous_pid {
        let _ = activate_pid(prev_pid);
    }

    // Coordinate-only fallback cannot inspect AXValue to prove submission.
    DesktopSendResult {
        status: SendOutcomeStatus::SubmissionFailed,
        strategy_used: "coordinate-fallback".to_string(),
        message: "Attempted window-relative paste/click fallback, but Accessibility verification was unavailable so submission cannot be confirmed.".to_string(),
        verified_submitted: false,
        target_window_title: Some(target_win.title.clone()),
        permission_status: perms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_bounds_inside_window() {
        let b = WindowBounds {
            x: 100.0,
            y: 50.0,
            width: 1200.0,
            height: 800.0,
        };
        let ((cx, cy), (sx, sy)) = calculate_relative_targets(b);
        assert!(cx > b.x && cx < b.x + b.width);
        assert!(cy > b.y && cy < b.y + b.height);
        assert!(sx > cx && sx < b.x + b.width);
        assert!(sy > b.y && sy < b.y + b.height);
    }
}
