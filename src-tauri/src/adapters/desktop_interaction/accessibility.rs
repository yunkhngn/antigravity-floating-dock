use std::time::{Duration, Instant};

use crate::adapters::desktop_interaction::ax_ffi::{
    is_accessibility_trusted, running_apps_for_bundle, AxElement, WindowBounds,
};
use crate::adapters::desktop_interaction::{
    AxElementSummary, AxUiInspection, DesktopSendRequest, SendOutcomeStatus,
};

const MAX_TRAVERSE_NODES: usize = 1800;
const TRAVERSE_DEADLINE_MS: u64 = 900;

#[derive(Debug, Clone)]
pub struct LocatedTarget {
    pub pid: i32,
    pub window: AxElement,
    pub window_title: String,
    pub composer: AxElement,
    pub send_button: Option<AxElement>,
}

pub enum AccessibilityOutcome {
    Submitted {
        window_title: String,
    },
    EscalateToKeyboard {
        target: LocatedTarget,
    },
    Failed {
        status: SendOutcomeStatus,
        message: String,
        window_title: Option<String>,
        verified_window_pid: Option<i32>,
    },
}

#[derive(Debug, Clone)]
struct CandidateComposer {
    element: AxElement,
    score: i32,
    bounds: Option<WindowBounds>,
    disabled: bool,
}

#[derive(Debug, Clone)]
struct CandidateButton {
    element: AxElement,
    score: i32,
    bounds: Option<WindowBounds>,
}

pub fn resolve_antigravity_pid(source_product: Option<&str>) -> Option<i32> {
    let bundle_id = match source_product {
        Some("antigravity-ide") => "com.google.antigravity.ide",
        _ => "com.google.antigravity",
    };
    let apps = running_apps_for_bundle(bundle_id);
    apps.first().map(|a| a.pid)
}

pub fn try_send_via_accessibility(req: &DesktopSendRequest) -> AccessibilityOutcome {
    let Some(pid) = resolve_antigravity_pid(req.source_product.as_deref()) else {
        return AccessibilityOutcome::Failed {
            status: SendOutcomeStatus::AppNotRunning,
            message: "Antigravity is not running.".to_string(),
            window_title: None,
            verified_window_pid: None,
        };
    };

    if !is_accessibility_trusted() {
        return AccessibilityOutcome::Failed {
            status: SendOutcomeStatus::PermissionDenied,
            message: "macOS Accessibility permission is required to interact with the Antigravity composer.".to_string(),
            window_title: None,
            verified_window_pid: None,
        };
    }

    let Some(app_elem) = AxElement::from_pid(pid) else {
        return AccessibilityOutcome::Failed {
            status: SendOutcomeStatus::AppNotRunning,
            message: "Unable to attach to the Antigravity process.".to_string(),
            window_title: None,
            verified_window_pid: None,
        };
    };

    enable_electron_accessibility(&app_elem);

    let (window, window_title, window_bounds) = match resolve_target_window(&app_elem, req) {
        Ok(found) => found,
        Err((status, message, title)) => {
            return AccessibilityOutcome::Failed {
                status,
                message,
                window_title: title,
                verified_window_pid: None,
            };
        }
    };

    if let Err((status, msg)) = verify_conversation_safety(&window, &window_title, req) {
        return AccessibilityOutcome::Failed {
            status,
            message: msg,
            window_title: Some(window_title),
            verified_window_pid: None,
        };
    }

    let (composer, send_button) = match find_composer_and_send(&window, window_bounds) {
        Ok(pair) => pair,
        Err((status, msg)) => {
            return AccessibilityOutcome::Failed {
                status,
                message: msg,
                window_title: Some(window_title),
                verified_window_pid: Some(pid),
            };
        }
    };

    if composer.get_bool("AXEnabled") == Some(false) {
        return AccessibilityOutcome::Failed {
            status: SendOutcomeStatus::SubmissionFailed,
            message: "Antigravity chat composer is currently disabled or busy.".to_string(),
            window_title: Some(window_title),
            verified_window_pid: Some(pid),
        };
    }

    let target = LocatedTarget {
        pid,
        window: window.clone(),
        window_title: window_title.clone(),
        composer: composer.clone(),
        send_button: send_button.clone(),
    };

    // Attempt direct AX text insertion first.
    let _ = composer.set_bool_attr("AXFocused", true);
    if composer.set_string_attr("AXValue", &req.message).is_err() {
        return AccessibilityOutcome::EscalateToKeyboard { target };
    }

    std::thread::sleep(Duration::from_millis(80));
    let inserted_ok = composer_contains_text(&composer, &req.message);
    if !inserted_ok {
        return AccessibilityOutcome::EscalateToKeyboard { target };
    }

    let Some(btn) = send_button.as_ref() else {
        return AccessibilityOutcome::EscalateToKeyboard { target };
    };

    if btn.get_bool("AXEnabled") == Some(false) {
        return AccessibilityOutcome::EscalateToKeyboard { target };
    }

    if btn.perform_action("AXPress").is_err() {
        return AccessibilityOutcome::EscalateToKeyboard { target };
    }

    if verify_submission(&composer, send_button.as_ref(), &req.message) {
        AccessibilityOutcome::Submitted { window_title }
    } else {
        AccessibilityOutcome::EscalateToKeyboard { target }
    }
}

pub fn enable_electron_accessibility(app_elem: &AxElement) {
    let _ = app_elem.set_bool_attr("AXManualAccessibility", true);
    let _ = app_elem.set_bool_attr("AXEnhancedUserInterface", true);
    std::thread::sleep(Duration::from_millis(50));
}

pub fn composer_contains_text(composer: &AxElement, expected: &str) -> bool {
    let Some(val) = composer.get_string("AXValue") else {
        return false;
    };
    let needle = expected.trim();
    if needle.is_empty() {
        return false;
    }
    val.contains(needle) || val.trim() == needle
}

pub fn verify_submission(
    composer: &AxElement,
    send_button: Option<&AxElement>,
    sent_message: &str,
) -> bool {
    let needle = sent_message.trim();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(900) {
        let current_val = composer.get_string("AXValue").unwrap_or_default();
        let trimmed = current_val.trim();
        if trimmed.is_empty() || (!needle.is_empty() && !trimmed.contains(needle)) {
            return true;
        }
        if let Some(btn) = send_button {
            let role_title = btn
                .get_string("AXTitle")
                .or_else(|| btn.get_string("AXDescription"))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if role_title.contains("stop") || role_title.contains("cancel") {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(75));
    }
    false
}

fn resolve_target_window(
    app_elem: &AxElement,
    req: &DesktopSendRequest,
) -> Result<(AxElement, String, Option<WindowBounds>), (SendOutcomeStatus, String, Option<String>)> {
    let mut windows = app_elem.get_children("AXWindows");
    if windows.is_empty() {
        if let Some(focused) = app_elem.get_element("AXFocusedWindow") {
            windows.push(focused);
        } else if let Some(main_win) = app_elem.get_element("AXMainWindow") {
            windows.push(main_win);
        }
    }

    let valid: Vec<(AxElement, String, Option<WindowBounds>, bool)> = windows
        .into_iter()
        .filter_map(|w| {
            let bounds = w.get_bounds();
            if let Some(b) = bounds {
                if b.width < 300.0 || b.height < 240.0 {
                    return None;
                }
            }
            let title = w.get_string("AXTitle").unwrap_or_default();
            let is_main = w.get_bool("AXMain").unwrap_or(false)
                || w.get_bool("AXFocused").unwrap_or(false);
            Some((w, title, bounds, is_main))
        })
        .collect();

    if valid.is_empty() {
        return Err((
            SendOutcomeStatus::ComposerNotFound,
            "No Antigravity conversation window is currently open.".to_string(),
            None,
        ));
    }

    if valid.len() == 1 {
        let (w, title, bounds, _) = &valid[0];
        return Ok((w.clone(), title.clone(), *bounds));
    }

    // Score multiple windows against expected title/workspace, or fall back to main window.
    let exp_title = req
        .expected_conversation_title
        .as_deref()
        .map(str::to_ascii_lowercase);
    let exp_ws_name = req
        .expected_workspace_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase);

    let mut scored: Vec<(i32, usize)> = valid
        .iter()
        .enumerate()
        .map(|(idx, (_, title, _, is_main))| {
            let lower = title.to_ascii_lowercase();
            let mut s = if *is_main { 10 } else { 0 };
            if let Some(t) = &exp_title {
                if !t.is_empty() && lower.contains(t) {
                    s += 50;
                }
            }
            if let Some(ws) = &exp_ws_name {
                if !ws.is_empty() && lower.contains(ws) {
                    s += 30;
                }
            }
            (s, idx)
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    if scored.len() >= 2 && scored[0].0 == scored[1].0 && scored[0].0 < 10 {
        return Err((
            SendOutcomeStatus::TargetAmbiguous,
            "Multiple Antigravity windows are open and the active target is ambiguous."
                .to_string(),
            None,
        ));
    }

    let best_idx = scored[0].1;
    let (w, title, bounds, _) = &valid[best_idx];
    Ok((w.clone(), title.clone(), *bounds))
}

fn verify_conversation_safety(
    _window: &AxElement,
    window_title: &str,
    req: &DesktopSendRequest,
) -> Result<(), (SendOutcomeStatus, String)> {
    let lower_title = window_title.trim().to_ascii_lowercase();

    // Prevent accidentally sending into a blank "New Conversation" when expecting an active session.
    if req.expected_conversation_id.is_some()
        && (lower_title == "new conversation"
            || lower_title == "new chat"
            || lower_title == "antigravity")
    {
        return Err((
            SendOutcomeStatus::ConversationMismatch,
            format!(
                "Antigravity appears to be on '{window_title}' rather than the expected active session."
            ),
        ));
    }

    if let Some(expected_title) = req.expected_conversation_title.as_deref() {
        let exp = expected_title.trim().to_ascii_lowercase();
        if !exp.is_empty()
            && !lower_title.is_empty()
            && !lower_title.contains(&exp)
            && !exp.contains(&lower_title)
        {
            return Err((
                SendOutcomeStatus::ConversationMismatch,
                format!(
                    "Active Antigravity window ('{window_title}') does not match expected conversation ('{expected_title}')."
                ),
            ));
        }
    }

    Ok(())
}

fn find_composer_and_send(
    window: &AxElement,
    window_bounds: Option<WindowBounds>,
) -> Result<(AxElement, Option<AxElement>), (SendOutcomeStatus, String)> {
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(window.clone());

    let mut visited = 0usize;
    let start = Instant::now();
    let mut composers: Vec<CandidateComposer> = Vec::new();
    let mut buttons: Vec<CandidateButton> = Vec::new();

    while let Some(node) = queue.pop_front() {
        if visited >= MAX_TRAVERSE_NODES || start.elapsed() > Duration::from_millis(TRAVERSE_DEADLINE_MS)
        {
            break;
        }
        visited += 1;

        let role = node.get_string("AXRole").unwrap_or_default();
        let subrole = node.get_string("AXSubrole").unwrap_or_default();
        let title = node.get_string("AXTitle").unwrap_or_default();
        let desc = node.get_string("AXDescription").unwrap_or_default();
        let placeholder = node.get_string("AXPlaceholderValue").unwrap_or_default();
        let help = node.get_string("AXHelp").unwrap_or_default();
        let dom_id = node.get_string("AXDOMIdentifier").unwrap_or_default();

        let text_meta =
            format!("{title} {desc} {placeholder} {help} {dom_id}").to_ascii_lowercase();

        let is_search = subrole == "AXSearchField"
            || text_meta.contains("search")
            || text_meta.contains("filter")
            || text_meta.contains("find");

        let is_text_input = matches!(
            role.as_str(),
            "AXTextArea" | "AXTextField" | "AXComboBox"
        ) || (node.get_bool("AXEditable") == Some(true)
            && node.is_attr_settable("AXValue"));

        if is_text_input && !is_search {
            let mut score = if role == "AXTextArea" { 25 } else { 10 };
            for kw in [
                "ask",
                "message",
                "chat",
                "prompt",
                "follow",
                "reply",
                "composer",
                "anything",
                "type",
            ] {
                if text_meta.contains(kw) {
                    score += 20;
                }
            }
            if node.is_attr_settable("AXValue") {
                score += 10;
            }
            let bounds = node.get_bounds();
            if let (Some(wb), Some(eb)) = (window_bounds, bounds) {
                if eb.y >= wb.y + wb.height * 0.45 {
                    score += 15;
                }
                if eb.width >= 200.0 {
                    score += 10;
                }
            }
            let disabled = node.get_bool("AXEnabled") == Some(false);
            composers.push(CandidateComposer {
                element: node.clone(),
                score,
                bounds,
                disabled,
            });
        } else if role == "AXButton" {
            let is_stop = text_meta.contains("stop") || text_meta.contains("cancel");
            let is_send = !is_stop
                && (text_meta.contains("send")
                    || text_meta.contains("submit")
                    || dom_id.to_ascii_lowercase().contains("send"));
            if is_send {
                let mut score = 30;
                if text_meta.contains("send") {
                    score += 20;
                }
                let bounds = node.get_bounds();
                if let (Some(wb), Some(eb)) = (window_bounds, bounds) {
                    if eb.y >= wb.y + wb.height * 0.45 {
                        score += 15;
                    }
                }
                buttons.push(CandidateButton {
                    element: node.clone(),
                    score,
                    bounds,
                });
            }
        }

        for child in node.get_children("AXChildren") {
            queue.push_back(child);
        }
    }

    if composers.is_empty() {
        return Err((
            SendOutcomeStatus::ComposerNotFound,
            "Could not locate an active chat composer in Antigravity's accessibility tree."
                .to_string(),
        ));
    }

    composers.sort_by(|a, b| b.score.cmp(&a.score));
    if composers.len() >= 2 && composers[0].score == composers[1].score {
        let diff_y = match (composers[0].bounds, composers[1].bounds) {
            (Some(a), Some(b)) => (a.y - b.y).abs(),
            _ => 100.0,
        };
        if diff_y > 40.0 {
            return Err((
                SendOutcomeStatus::TargetAmbiguous,
                "Multiple candidate chat inputs detected in Antigravity.".to_string(),
            ));
        }
    }

    if composers[0].disabled {
        return Err((
            SendOutcomeStatus::SubmissionFailed,
            "Antigravity chat composer is currently disabled.".to_string(),
        ));
    }

    let chosen_composer = composers.remove(0);

    // Pick Send button closest to/below the chosen composer.
    buttons.sort_by(|a, b| {
        let a_prox = proximity_bonus(chosen_composer.bounds, a.bounds);
        let b_prox = proximity_bonus(chosen_composer.bounds, b.bounds);
        (b.score + b_prox).cmp(&(a.score + a_prox))
    });

    let chosen_send = buttons.into_iter().next().map(|b| b.element);
    Ok((chosen_composer.element, chosen_send))
}

fn proximity_bonus(composer: Option<WindowBounds>, button: Option<WindowBounds>) -> i32 {
    match (composer, button) {
        (Some(c), Some(b)) => {
            let dy = (b.y - c.y).abs();
            if dy < 140.0 {
                25
            } else {
                0
            }
        }
        _ => 0,
    }
}

pub fn inspect_antigravity_ui(source_product: Option<&str>) -> AxUiInspection {
    let trusted = is_accessibility_trusted();
    let pids = resolve_antigravity_pid(source_product).into_iter().collect::<Vec<_>>();
    let mut windows_summary = Vec::new();
    let mut elements = Vec::new();

    if trusted {
        if let Some(&pid) = pids.first() {
            if let Some(app_elem) = AxElement::from_pid(pid) {
                enable_electron_accessibility(&app_elem);
                let wins = app_elem.get_children("AXWindows");
                for w in &wins {
                    let title = w.get_string("AXTitle").unwrap_or_default();
                    let bounds = w.get_bounds();
                    windows_summary.push(format!(
                        "{} ({:.0}x{:.0})",
                        if title.is_empty() { "<untitled>" } else { &title },
                        bounds.map(|b| b.width).unwrap_or(0.0),
                        bounds.map(|b| b.height).unwrap_or(0.0)
                    ));
                }
                if let Some(first_win) = wins.first() {
                    let mut queue = std::collections::VecDeque::new();
                    queue.push_back(first_win.clone());
                    let mut visited = 0usize;
                    while let Some(node) = queue.pop_front() {
                        if visited >= 400 || elements.len() >= 40 {
                            break;
                        }
                        visited += 1;
                        let role = node.get_string("AXRole").unwrap_or_default();
                        if matches!(
                            role.as_str(),
                            "AXTextArea" | "AXTextField" | "AXButton" | "AXTab"
                        ) {
                            elements.push(AxElementSummary {
                                role,
                                title: node.get_string("AXTitle"),
                                description: node.get_string("AXDescription"),
                                placeholder: node.get_string("AXPlaceholderValue"),
                                enabled: node.get_bool("AXEnabled"),
                            });
                        }
                        for c in node.get_children("AXChildren") {
                            queue.push_back(c);
                        }
                    }
                }
            }
        }
    }

    AxUiInspection {
        accessibility_trusted: trusted,
        running_pids: pids,
        window_titles: windows_summary,
        interesting_elements: elements,
    }
}
