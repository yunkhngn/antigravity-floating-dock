use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::redact::{redact_json, redact_string};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DockEventType {
    SessionStart,
    PreInvocation,
    PostToolUse,
    PostInvocation,
    Stop,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentActivityState {
    Idle,
    Working,
    NeedsAttention,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockEvent {
    pub id: String,
    pub event_type: DockEventType,
    pub timestamp_ms: u64,
    pub conversation_id: Option<String>,
    pub workspace_path: Option<String>,
    pub model_name: Option<String>,
    pub summary: String,
    pub detail: Option<String>,
    pub activity_state: AgentActivityState,
    pub termination_reason: Option<String>,
    pub transcript_path: Option<String>,
    pub source_product: String,
    pub dedupe_key: String,
    pub redacted_raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockSettings {
    pub toggle_chat_shortcut: String,
    pub toggle_bubble_shortcut: String,
    pub pin_bubble: bool,
    pub auto_hide_idle_ms: u64,
    pub launch_at_login_hint_acknowledged: bool,
}

impl Default for DockSettings {
    fn default() -> Self {
        Self {
            toggle_chat_shortcut: "Alt+Space".to_string(),
            toggle_bubble_shortcut: "Alt+Shift+Space".to_string(),
            pin_bubble: false,
            auto_hide_idle_ms: 8000,
            launch_at_login_hint_acknowledged: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub antigravity_running: bool,
    pub hook_installed: bool,
    pub relay_installed: bool,
    pub ipc_listening: bool,
    pub ipc_socket_path: String,
    pub last_event_at_ms: Option<u64>,
    pub vibrancy_mode: String,
    pub shortcut_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptMessage {
    pub role: String,
    pub text: String,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCapabilities {
    pub source_product: String,
    pub can_send_message: bool,
    pub limitation_reason: String,
    pub supports_open_in_antigravity: bool,
    pub supports_copy_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockSnapshot {
    pub status: ConnectionStatus,
    pub settings: DockSettings,
    pub active_event: Option<DockEvent>,
    pub recent_events: Vec<DockEvent>,
    pub transcript_tail: Vec<TranscriptMessage>,
    pub transcript_error: Option<String>,
    pub session_capabilities: SessionCapabilities,
    pub bubble_pinned: bool,
}

fn non_empty_str(val: Option<&Value>) -> Option<String> {
    val.and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn detect_source_product(transcript_path: Option<&str>, artifact_dir: Option<&str>) -> String {
    let combined = transcript_path.or(artifact_dir).unwrap_or_default();
    if combined.contains("/antigravity-cli/") {
        "antigravity-cli".to_string()
    } else if combined.contains("/antigravity-ide/") {
        "antigravity-ide".to_string()
    } else if combined.contains("/antigravity/") {
        "antigravity".to_string()
    } else {
        "unknown".to_string()
    }
}

fn summarize_tool_name(tool_name: &str) -> String {
    match tool_name {
        "run_command" => "Ran terminal command".to_string(),
        "read_file" | "view_file" => "Read files".to_string(),
        "edit_file" | "write_file" | "replace_file_content" | "multi_replace_file_content" => {
            "Edited files".to_string()
        }
        "search_web" => "Searched web".to_string(),
        "invoke_subagent" => "Delegated subagent".to_string(),
        other if !other.is_empty() => format!("Finished {other}"),
        _ => "Finished tool step".to_string(),
    }
}

pub fn normalize_hook_payload(raw: &Value, timestamp_ms: u64, seq: u64) -> Result<DockEvent, String> {
    let obj = raw
        .as_object()
        .ok_or_else(|| "Hook payload must be a JSON object".to_string())?;

    let hook_event_name = non_empty_str(obj.get("hookEventName")).unwrap_or_default();
    let conversation_id = non_empty_str(obj.get("conversationId"));
    let workspace_path = obj
        .get("workspacePaths")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| non_empty_str(obj.get("cwd")));
    let model_name = non_empty_str(obj.get("modelName"));
    let transcript_path = non_empty_str(obj.get("transcriptPath"));
    let artifact_dir = non_empty_str(obj.get("artifactDirectoryPath"));
    let last_user_input = non_empty_str(obj.get("lastUserInput")).map(|s| redact_string(&s));
    let source_product = detect_source_product(transcript_path.as_deref(), artifact_dir.as_deref());

    let (event_type, activity_state, summary, detail, termination_reason) =
        match hook_event_name.as_str() {
            "SessionStart" => (
                DockEventType::SessionStart,
                AgentActivityState::Working,
                "Session started".to_string(),
                model_name.as_ref().map(|m| format!("Model: {m}")),
                None,
            ),
            "PreInvocation" => (
                DockEventType::PreInvocation,
                AgentActivityState::Working,
                "Thinking…".to_string(),
                last_user_input.clone(),
                None,
            ),
            "PostToolUse" => {
                let tool_name = obj
                    .get("toolCall")
                    .and_then(|tc| tc.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool");
                let err = non_empty_str(obj.get("error"));
                if let Some(err_msg) = err {
                    (
                        DockEventType::PostToolUse,
                        AgentActivityState::NeedsAttention,
                        format!("Tool failed: {tool_name}"),
                        Some(redact_string(&err_msg)),
                        None,
                    )
                } else {
                    (
                        DockEventType::PostToolUse,
                        AgentActivityState::Working,
                        summarize_tool_name(tool_name),
                        Some(format!("Tool: {tool_name}")),
                        None,
                    )
                }
            }
            "PostInvocation" => (
                DockEventType::PostInvocation,
                AgentActivityState::Working,
                "Reviewing tool results…".to_string(),
                None,
                None,
            ),
            "Stop" => {
                let reason = non_empty_str(obj.get("terminationReason"));
                let err = non_empty_str(obj.get("error"));
                let (state, sum, det) = match reason.as_deref() {
                    Some("ERROR") => (
                        AgentActivityState::Error,
                        "Stopped with error".to_string(),
                        err.map(|e| redact_string(&e)),
                    ),
                    Some("USER_CANCELED") => (
                        AgentActivityState::Idle,
                        "Turn canceled".to_string(),
                        Some("Canceled by user".to_string()),
                    ),
                    Some("MAX_INVOCATIONS")
                    | Some("MAX_FORCED_INVOCATIONS")
                    | Some("MAX_TOKEN_BUDGET_EXCEEDED")
                    | Some("TERMINAL_CUSTOM_HOOK") => (
                        AgentActivityState::NeedsAttention,
                        "Stopped before completion".to_string(),
                        reason.clone(),
                    ),
                    Some("NO_TOOL_CALL") => (
                        AgentActivityState::Idle,
                        "Turn ended (idle)".to_string(),
                        Some("Stop reason: NO_TOOL_CALL (success not inferred)".to_string()),
                    ),
                    Some(other) => (
                        AgentActivityState::Unknown,
                        format!("Turn stopped ({other})"),
                        err.map(|e| redact_string(&e)),
                    ),
                    None => (
                        AgentActivityState::Unknown,
                        "Turn stopped (reason unknown)".to_string(),
                        err.map(|e| redact_string(&e)),
                    ),
                };
                (DockEventType::Stop, state, sum, det, reason)
            }
            _ => (
                DockEventType::Unknown,
                AgentActivityState::Unknown,
                if hook_event_name.is_empty() {
                    "Unknown hook event".to_string()
                } else {
                    format!("Event: {hook_event_name}")
                },
                None,
                None,
            ),
        };

    let dedupe_key = format!(
        "{}:{}:{}:{}:{}",
        conversation_id.as_deref().unwrap_or("-"),
        hook_event_name,
        summary,
        detail.as_deref().unwrap_or("-"),
        termination_reason.as_deref().unwrap_or("-")
    );

    Ok(DockEvent {
        id: format!("evt-{timestamp_ms}-{seq}"),
        event_type,
        timestamp_ms,
        conversation_id,
        workspace_path,
        model_name,
        summary,
        detail,
        activity_state,
        termination_reason,
        transcript_path,
        source_product,
        dedupe_key,
        redacted_raw: redact_json(raw),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stop_no_tool_call_is_idle_not_completed() {
        let payload = json!({
            "hookEventName": "Stop",
            "conversationId": "conv-1",
            "terminationReason": "NO_TOOL_CALL",
            "fullyIdle": true,
            "transcriptPath": "/Users/test/.gemini/antigravity/conversations/conv-1.jsonl"
        });
        let event = normalize_hook_payload(&payload, 1000, 1).unwrap();
        assert_eq!(event.event_type, DockEventType::Stop);
        assert_eq!(event.activity_state, AgentActivityState::Idle);
        assert_eq!(event.source_product, "antigravity");
        assert!(event.summary.contains("idle"));
    }

    #[test]
    fn stop_error_maps_to_error() {
        let payload = json!({
            "hookEventName": "Stop",
            "terminationReason": "ERROR",
            "error": "rate limit"
        });
        let event = normalize_hook_payload(&payload, 1000, 2).unwrap();
        assert_eq!(event.activity_state, AgentActivityState::Error);
        assert_eq!(event.detail.as_deref(), Some("rate limit"));
    }

    #[test]
    fn unknown_event_stays_unknown() {
        let payload = json!({
            "hookEventName": "FutureExperimentalHook"
        });
        let event = normalize_hook_payload(&payload, 1000, 3).unwrap();
        assert_eq!(event.event_type, DockEventType::Unknown);
        assert_eq!(event.activity_state, AgentActivityState::Unknown);
    }
}
