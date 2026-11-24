#[cfg(target_os = "macos")]
pub mod accessibility;
#[cfg(target_os = "macos")]
pub mod ax_ffi;
#[cfg(target_os = "macos")]
pub mod coordinate_fallback;
#[cfg(target_os = "macos")]
pub mod keyboard;

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum SendOutcomeStatus {
    Success,
    Queued,
    AppNotRunning,
    PermissionDenied,
    ComposerNotFound,
    ConversationMismatch,
    SubmissionFailed,
    TargetAmbiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub accessibility_trusted: bool,
    pub input_monitoring_needed: bool,
    pub instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSendRequest {
    pub message: String,
    pub expected_conversation_id: Option<String>,
    pub expected_conversation_title: Option<String>,
    pub expected_workspace_path: Option<String>,
    pub source_product: Option<String>,
    pub idempotency_key: Option<String>,
    pub allow_keyboard_fallback: bool,
    pub allow_coordinate_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSendResult {
    pub status: SendOutcomeStatus,
    pub strategy_used: String,
    pub message: String,
    pub verified_submitted: bool,
    pub target_window_title: Option<String>,
    pub permission_status: PermissionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxElementSummary {
    pub role: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub placeholder: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxUiInspection {
    pub accessibility_trusted: bool,
    pub running_pids: Vec<i32>,
    pub window_titles: Vec<String>,
    pub interesting_elements: Vec<AxElementSummary>,
}

const MAX_MESSAGE_CHARS: usize = 32_000;
const DUPLICATE_KEY_WINDOW_MS: u64 = 10_000;
const DUPLICATE_HASH_WINDOW_MS: u64 = 2_500;

#[derive(Default)]
struct SubmissionGuard {
    in_flight: bool,
    last_idempotency_key: Option<(String, u64)>,
    last_success_hash: Option<(u64, u64)>,
}

pub struct DesktopInteractionAdapter {
    guard: Mutex<SubmissionGuard>,
}

impl DesktopInteractionAdapter {
    pub fn new() -> Self {
        Self {
            guard: Mutex::new(SubmissionGuard::default()),
        }
    }

    pub fn send_message(&self, req: &DesktopSendRequest) -> DesktopSendResult {
        let perms = check_permissions();
        let trimmed = req.message.trim();
        if trimmed.is_empty() {
            return DesktopSendResult {
                status: SendOutcomeStatus::SubmissionFailed,
                strategy_used: "none".to_string(),
                message: "Cannot send an empty message.".to_string(),
                verified_submitted: false,
                target_window_title: None,
                permission_status: perms,
            };
        }
        if trimmed.chars().count() > MAX_MESSAGE_CHARS {
            return DesktopSendResult {
                status: SendOutcomeStatus::SubmissionFailed,
                strategy_used: "none".to_string(),
                message: "Message exceeds maximum supported length (32,000 characters)."
                    .to_string(),
                verified_submitted: false,
                target_window_title: None,
                permission_status: perms,
            };
        }

        let now = now_ms();
        let msg_hash = hash_str(trimmed);
        {
            let mut g = self.guard.lock().expect("submission guard poisoned");
            if g.in_flight {
                return DesktopSendResult {
                    status: SendOutcomeStatus::SubmissionFailed,
                    strategy_used: "none".to_string(),
                    message: "A message submission is already in progress.".to_string(),
                    verified_submitted: false,
                    target_window_title: None,
                    permission_status: perms,
                };
            }
            if let (Some(req_key), Some((prev_key, prev_ts))) =
                (req.idempotency_key.as_deref(), g.last_idempotency_key.as_ref())
            {
                if req_key == prev_key && now.saturating_sub(*prev_ts) < DUPLICATE_KEY_WINDOW_MS {
                    return DesktopSendResult {
                        status: SendOutcomeStatus::SubmissionFailed,
                        strategy_used: "none".to_string(),
                        message: "Duplicate submission prevented (idempotency key already processed).".to_string(),
                        verified_submitted: false,
                        target_window_title: None,
                        permission_status: perms,
                    };
                }
            }
            if let Some((prev_hash, prev_ts)) = g.last_success_hash {
                if prev_hash == msg_hash && now.saturating_sub(prev_ts) < DUPLICATE_HASH_WINDOW_MS {
                    return DesktopSendResult {
                        status: SendOutcomeStatus::SubmissionFailed,
                        strategy_used: "none".to_string(),
                        message: "Duplicate rapid resubmission prevented.".to_string(),
                        verified_submitted: false,
                        target_window_title: None,
                        permission_status: perms,
                    };
                }
            }
            g.in_flight = true;
        }

        let result = self.execute_send(req, perms);

        {
            let mut g = self.guard.lock().expect("submission guard poisoned");
            g.in_flight = false;
            if result.status == SendOutcomeStatus::Success {
                let done_at = now_ms();
                g.last_success_hash = Some((msg_hash, done_at));
                if let Some(key) = &req.idempotency_key {
                    g.last_idempotency_key = Some((key.clone(), done_at));
                }
            }
        }

        result
    }

    #[cfg(target_os = "macos")]
    fn execute_send(&self, req: &DesktopSendRequest, perms: PermissionStatus) -> DesktopSendResult {
        use accessibility::{try_send_via_accessibility, AccessibilityOutcome};
        use crate::adapters::sidecar_bridge;

        // Priority 0: Official Antigravity Sidecar Bridge (background, exact conversationId).
        if let Some(cid) = req.expected_conversation_id.as_deref() {
            let trimmed_cid = cid.trim();
            let is_real_cid = !trimmed_cid.is_empty()
                && trimmed_cid != "sample-session"
                && trimmed_cid != "default"
                && trimmed_cid != "preview";
            if is_real_cid && sidecar_bridge::ping_bridge() {
                return match sidecar_bridge::send_message_via_sidecar(trimmed_cid, &req.message) {
                    Ok(()) => DesktopSendResult {
                        status: SendOutcomeStatus::Success,
                        strategy_used: "sidecar".to_string(),
                        message: "Message sent in background via Antigravity Sidecar Bridge."
                            .to_string(),
                        verified_submitted: true,
                        target_window_title: req.expected_conversation_title.clone(),
                        permission_status: perms,
                    },
                    Err(err) => DesktopSendResult {
                        status: SendOutcomeStatus::SubmissionFailed,
                        strategy_used: "sidecar".to_string(),
                        message: format!("Sidecar Bridge send failed: {err}"),
                        verified_submitted: false,
                        target_window_title: req.expected_conversation_title.clone(),
                        permission_status: perms,
                    },
                };
            }
        }

        match try_send_via_accessibility(req) {
            AccessibilityOutcome::Submitted { window_title } => DesktopSendResult {
                status: SendOutcomeStatus::Success,
                strategy_used: "accessibility".to_string(),
                message: "Message inserted and submitted via macOS Accessibility.".to_string(),
                verified_submitted: true,
                target_window_title: Some(window_title),
                permission_status: perms,
            },
            AccessibilityOutcome::EscalateToKeyboard { target } => {
                if req.allow_keyboard_fallback {
                    keyboard::try_send_via_keyboard(&target, &req.message, perms)
                } else {
                    DesktopSendResult {
                        status: SendOutcomeStatus::SubmissionFailed,
                        strategy_used: "accessibility".to_string(),
                        message: "Accessibility located the composer, but keyboard fallback is disabled.".to_string(),
                        verified_submitted: false,
                        target_window_title: Some(target.window_title),
                        permission_status: perms,
                    }
                }
            }
            AccessibilityOutcome::Failed {
                status,
                message,
                window_title,
                verified_window_pid,
            } => {
                if status == SendOutcomeStatus::ComposerNotFound && req.allow_coordinate_fallback {
                    if let Some(pid) = verified_window_pid {
                        return coordinate_fallback::try_send_via_coordinates(
                            pid,
                            window_title.as_deref(),
                            &req.message,
                            perms,
                        );
                    }
                }
                DesktopSendResult {
                    status,
                    strategy_used: "accessibility".to_string(),
                    message,
                    verified_submitted: false,
                    target_window_title: window_title,
                    permission_status: perms,
                }
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn execute_send(&self, _req: &DesktopSendRequest, perms: PermissionStatus) -> DesktopSendResult {
        DesktopSendResult {
            status: SendOutcomeStatus::SubmissionFailed,
            strategy_used: "none".to_string(),
            message: "Desktop composer automation is supported on macOS only.".to_string(),
            verified_submitted: false,
            target_window_title: None,
            permission_status: perms,
        }
    }
}

pub fn check_permissions() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        let trusted = ax_ffi::is_accessibility_trusted();
        let instructions = if trusted {
            "macOS Accessibility permission granted.".to_string()
        } else {
            "Enable Accessibility for Antigravity Floating Dock in System Settings → Privacy & Security → Accessibility.".to_string()
        };
        PermissionStatus {
            accessibility_trusted: trusted,
            input_monitoring_needed: !trusted,
            instructions,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        PermissionStatus {
            accessibility_trusted: false,
            input_monitoring_needed: false,
            instructions: "macOS only.".to_string(),
        }
    }
}

pub fn request_accessibility_permission() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        let _ = ax_ffi::prompt_accessibility_permission();
    }
    check_permissions()
}

pub fn find_antigravity_pids(source_product: Option<&str>) -> Vec<i32> {
    #[cfg(target_os = "macos")]
    {
        accessibility::resolve_antigravity_pid(source_product)
            .into_iter()
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = source_product;
        Vec::new()
    }
}

pub fn inspect_ui(source_product: Option<&str>) -> AxUiInspection {
    #[cfg(target_os = "macos")]
    {
        accessibility::inspect_antigravity_ui(source_product)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = source_product;
        AxUiInspection {
            accessibility_trusted: false,
            running_pids: Vec::new(),
            window_titles: Vec::new(),
            interesting_elements: Vec::new(),
        }
    }
}

fn hash_str(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_message_without_touching_desktop() {
        let adapter = DesktopInteractionAdapter::new();
        let res = adapter.send_message(&DesktopSendRequest {
            message: "   \n  ".to_string(),
            expected_conversation_id: None,
            expected_conversation_title: None,
            expected_workspace_path: None,
            source_product: None,
            idempotency_key: None,
            allow_keyboard_fallback: true,
            allow_coordinate_fallback: false,
        });
        assert_eq!(res.status, SendOutcomeStatus::SubmissionFailed);
        assert!(!res.verified_submitted);
    }
}
