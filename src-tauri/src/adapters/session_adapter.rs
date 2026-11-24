use serde_json::Value;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;

use crate::adapters::{desktop_interaction, sidecar_bridge};
use crate::models::{ConversationSummary, SessionCapabilities, TranscriptMessage};
use crate::paths;
use crate::redact::{redact_string, redact_text_with_limit};

const MAX_TAIL_READ_BYTES: u64 = 512 * 1024;
const MAX_MESSAGE_CHARS: usize = 8000;

pub trait SessionAdapter: Send + Sync {
    fn capabilities(&self, source_product: &str) -> SessionCapabilities;
    fn read_transcript_tail(
        &self,
        transcript_path: &str,
        max_items: usize,
    ) -> Result<Vec<TranscriptMessage>, String>;
    fn resolve_transcript_for_conversation(&self, conversation_id: &str) -> Option<String>;
    fn open_antigravity(&self, source_product: &str, conversation_id: Option<&str>) -> Result<(), String>;
    fn is_antigravity_running(&self) -> bool;
}

pub struct AntigravitySessionAdapter;

impl SessionAdapter for AntigravitySessionAdapter {
    fn capabilities(&self, source_product: &str) -> SessionCapabilities {
        let running = self.is_antigravity_running();
        let desktop_perms = desktop_interaction::check_permissions();
        let sidecar = sidecar_bridge::inspect_sidecar_status();

        let can_send_message =
            sidecar.connected || (desktop_perms.accessibility_trusted && running);

        let limitation_reason = if !running {
            "Antigravity is not running. Start Antigravity to send messages.".to_string()
        } else if sidecar.connected {
            format!(
                "Sidecar Bridge active: sends directly in background to the active {source_product} conversation."
            )
        } else if sidecar.installed && !desktop_perms.accessibility_trusted {
            "Sidecar Bridge installed — restart Antigravity once to activate background sending (or grant Accessibility as fallback).".to_string()
        } else if desktop_perms.accessibility_trusted {
            format!(
                "Using macOS Accessibility fallback for {source_product}. Install Hook & Sidecar for background sending."
            )
        } else {
            "Install Hook & Sidecar (and restart Antigravity once) for background sending, or grant Accessibility access.".to_string()
        };

        SessionCapabilities {
            source_product: source_product.to_string(),
            can_send_message,
            limitation_reason,
            supports_open_in_antigravity: true,
            supports_copy_fallback: true,
        }
    }

    fn read_transcript_tail(
        &self,
        transcript_path: &str,
        max_items: usize,
    ) -> Result<Vec<TranscriptMessage>, String> {
        let candidate = Path::new(transcript_path);
        let canonical = fs::canonicalize(candidate)
            .map_err(|_| "Transcript path is missing or inaccessible".to_string())?;

        let allowed = paths::allowed_transcript_roots().iter().any(|root| {
            fs::canonicalize(root)
                .map(|canon_root| canonical.starts_with(&canon_root))
                .unwrap_or(false)
        });

        if !allowed {
            return Err("Transcript path is outside allowed Antigravity directories".to_string());
        }

        let meta = fs::metadata(&canonical)
            .map_err(|_| "Unable to read transcript metadata".to_string())?;
        if !meta.is_file() {
            return Err("Transcript path is not a regular file".to_string());
        }

        let ext = canonical
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext != "jsonl" && ext != "json" {
            return Err("Transcript format is not a readable JSONL log".to_string());
        }

        let mut file =
            File::open(&canonical).map_err(|_| "Failed to open transcript file".to_string())?;
        let file_len = meta.len();
        if file_len > MAX_TAIL_READ_BYTES {
            let offset = file_len - MAX_TAIL_READ_BYTES;
            file.seek(SeekFrom::Start(offset))
                .map_err(|_| "Failed to seek transcript tail".to_string())?;
        }

        let mut raw = String::new();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|_| "Failed to read transcript contents".to_string())?;
        raw.push_str(&String::from_utf8_lossy(&bytes));

        let mut parsed: Vec<TranscriptMessage> = Vec::new();
        let lines: Vec<&str> = raw.lines().collect();
        let start_idx = if file_len > MAX_TAIL_READ_BYTES && !lines.is_empty() {
            1
        } else {
            0
        };

        for line in &lines[start_idx..] {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                if let Some(mut msg) = extract_transcript_message(&val) {
                    if msg.role == "Activity" {
                        if let Some(prev) = parsed.last_mut() {
                            if prev.role == "Activity" {
                                prev.steps.extend(msg.steps);
                                if msg.timestamp.is_some() {
                                    prev.timestamp = msg.timestamp;
                                }
                                continue;
                            }
                        }
                    } else if msg.role == "Assistant" {
                        if let Some(prev) = parsed.last_mut() {
                            if prev.role == "Activity" {
                                let mut combined = std::mem::take(&mut prev.steps);
                                combined.extend(msg.steps);
                                msg.steps = combined;
                                parsed.pop();
                            }
                        }
                    }
                    parsed.push(msg);
                }
            }
        }

        if parsed.len() > max_items {
            parsed.drain(0..(parsed.len() - max_items));
        }
        Ok(parsed)
    }

    fn resolve_transcript_for_conversation(&self, conversation_id: &str) -> Option<String> {
        let trimmed = conversation_id.trim();
        if trimmed.is_empty()
            || !trimmed
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return None;
        }
        for root in paths::allowed_transcript_roots() {
            let candidate = root
                .join("brain")
                .join(trimmed)
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
        None
    }

    fn open_antigravity(&self, source_product: &str, conversation_id: Option<&str>) -> Result<(), String> {
        let app_name = if source_product == "antigravity-ide" {
            "Antigravity IDE"
        } else {
            "Antigravity"
        };

        let status = Command::new("open")
            .arg("-a")
            .arg(app_name)
            .status()
            .map_err(|e| format!("Failed to launch {app_name}: {e}"))?;

        if !status.success() {
            return Err(format!("Could not focus {app_name} (exit status {status})"));
        }

        if let Some(cid) = conversation_id {
            let trimmed = cid.trim();
            let valid_id = !trimmed.is_empty()
                && trimmed != "sample-session"
                && trimmed != "default"
                && trimmed != "preview"
                && trimmed
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if valid_id && source_product != "antigravity-ide" {
                let _ = Command::new("open")
                    .arg(format!("antigravity://conversation/{trimmed}"))
                    .status();
            }
        }

        Ok(())
    }

    fn is_antigravity_running(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            if !desktop_interaction::find_antigravity_pids(None).is_empty() {
                return true;
            }
        }
        Command::new("pgrep")
            .args(["-f", "/Applications/Antigravity"])
            .output()
            .map(|out| out.status.success() && !out.stdout.is_empty())
            .unwrap_or(false)
    }
}

impl AntigravitySessionAdapter {
    pub fn list_local_conversations(&self, max_items: usize) -> Vec<(ConversationSummary, String)> {
        let mut entries: Vec<(std::time::SystemTime, ConversationSummary, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for root in paths::allowed_transcript_roots() {
            let brain_dir = root.join("brain");
            let Ok(read_dir) = fs::read_dir(&brain_dir) else {
                continue;
            };
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(id_os) = path.file_name() else {
                    continue;
                };
                let id = id_os.to_string_lossy().to_string();
                if id.is_empty()
                    || id == "tempmediaStorage"
                    || !id
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    || !seen.insert(id.clone())
                {
                    continue;
                }

                let transcript_path = path
                    .join(".system_generated")
                    .join("logs")
                    .join("transcript.jsonl");
                let Ok(meta) = fs::metadata(&transcript_path) else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }

                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                let update_secs = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let title = extract_conversation_title(&transcript_path).unwrap_or_else(|| {
                    let short: String = id.chars().take(8).collect();
                    format!("Session {short}")
                });

                entries.push((
                    modified,
                    ConversationSummary {
                        id,
                        title,
                        run_state: "RUN_STATE_IDLE".to_string(),
                        update_time: update_secs.to_string(),
                        project_id: None,
                    },
                    transcript_path.display().to_string(),
                ));
            }
        }

        entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.truncate(max_items);
        entries
            .into_iter()
            .map(|(_, summary, tpath)| (summary, tpath))
            .collect()
    }
}

fn extract_conversation_title(transcript_path: &Path) -> Option<String> {
    let mut file = File::open(transcript_path).ok()?;
    let mut buf = vec![0u8; 24 * 1024];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    let raw = String::from_utf8_lossy(&buf);

    for line in raw.lines().take(24) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(msg) = extract_transcript_message(&val) else {
            continue;
        };
        if msg.role == "You" {
            let first_line = msg.text.lines().find(|l| !l.trim().is_empty())?.trim();
            let clean = redact_string(first_line);
            if clean.chars().count() > 54 {
                let cut: String = clean.chars().take(54).collect();
                return Some(format!("{cut}…"));
            }
            return Some(clean);
        }
    }
    None
}

fn clean_tool_arg(val: Option<&Value>) -> Option<String> {
    let raw = val.and_then(|v| v.as_str())?.trim();
    let unquoted = raw.trim_matches('"').trim();
    if unquoted.is_empty() {
        return None;
    }
    Some(unquoted.to_string())
}

fn format_tool_call_step(tc: &Value) -> Option<String> {
    let name = tc.get("name").and_then(|v| v.as_str())?.trim();
    if name.is_empty() || name == "update_focus" || name == "update_steps" {
        return None;
    }
    let args = tc.get("args").and_then(|v| v.as_object());
    let path_str = args
        .and_then(|a| a.get("Path"))
        .and_then(|v| clean_tool_arg(Some(v)));
    let basename = path_str
        .as_deref()
        .and_then(|p| Path::new(p).file_name().and_then(|n| n.to_str()))
        .map(ToString::to_string);

    match name {
        "read_file" | "view_file" => Some(match basename {
            Some(file) => format!("Analyzed {file}"),
            None => "Read file".to_string(),
        }),
        "edit_file" | "write_file" | "replace_file_content" | "multi_replace_file_content" => {
            Some(match basename {
                Some(file) => format!("Edited {file}"),
                None => "Edited file".to_string(),
            })
        }
        "run_command" => {
            let summary = args
                .and_then(|a| a.get("toolSummary"))
                .and_then(|v| clean_tool_arg(Some(v)));
            Some(match summary {
                Some(s) => format!("Ran {s}"),
                None => "Ran terminal command".to_string(),
            })
        }
        "search_web" => {
            let query = args
                .and_then(|a| a.get("query"))
                .and_then(|v| clean_tool_arg(Some(v)));
            Some(match query {
                Some(q) => format!("Searched web: {}", redact_string(&q)),
                None => "Searched web".to_string(),
            })
        }
        "invoke_subagent" => {
            let role = args
                .and_then(|a| a.get("Role"))
                .and_then(|v| clean_tool_arg(Some(v)));
            Some(match role {
                Some(r) => format!("Delegated to {r}"),
                None => "Delegated subagent".to_string(),
            })
        }
        other => Some(format!("Used {other}")),
    }
}

fn extract_tool_steps(obj: &serde_json::Map<String, Value>) -> Vec<String> {
    let Some(arr) = obj.get("tool_calls").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter().filter_map(format_tool_call_step).collect()
}

fn extract_transcript_message(val: &Value) -> Option<TranscriptMessage> {
    let obj = val.as_object()?;
    let raw_role = obj
        .get("role")
        .or_else(|| obj.get("type"))
        .or_else(|| obj.get("author"))
        .and_then(|v| v.as_str())
        .unwrap_or("event");

    // Skip internal tool execution output and low-level system notices.
    if raw_role == "GENERIC" || raw_role == "THOUGHT" || raw_role == "TOOL_CALL" {
        return None;
    }

    let steps = if raw_role == "PLANNER_RESPONSE" {
        extract_tool_steps(obj)
    } else {
        Vec::new()
    };

    let raw_text = obj
        .get("text")
        .or_else(|| obj.get("message"))
        .or_else(|| obj.get("content"))
        .or_else(|| obj.get("summary"))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Array(parts) => parts.iter().find_map(|p| {
                p.get("text")
                    .and_then(|t| t.as_str())
                    .map(ToString::to_string)
            }),
            _ => None,
        })
        .unwrap_or_default();

    let (role, normalized_text) = if raw_role == "SYSTEM_MESSAGE" {
        // Only surface user-sent queued messages (e.g. Sidecar Bridge), drop internal reminders/task logs.
        let extracted = extract_high_priority_user_msg(&raw_text)?;
        ("You".to_string(), extracted)
    } else if raw_role == "USER_INPUT" || raw_role.eq_ignore_ascii_case("user") {
        ("You".to_string(), clean_transcript_markup(raw_text.trim()))
    } else if raw_role == "PLANNER_RESPONSE"
        || raw_role.eq_ignore_ascii_case("assistant")
        || raw_role.eq_ignore_ascii_case("model")
    {
        ("Assistant".to_string(), raw_text.trim().to_string())
    } else {
        (raw_role.to_string(), clean_transcript_markup(raw_text.trim()))
    };

    let cleaned = redact_text_with_limit(normalized_text.trim(), MAX_MESSAGE_CHARS);
    let final_role = if cleaned.is_empty() {
        if steps.is_empty() {
            return None;
        }
        "Activity".to_string()
    } else {
        role
    };

    let timestamp = obj
        .get("timestamp")
        .or_else(|| obj.get("created_at"))
        .or_else(|| obj.get("createdAt"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    Some(TranscriptMessage {
        role: final_role,
        text: cleaned,
        timestamp,
        steps,
    })
}

fn extract_high_priority_user_msg(raw: &str) -> Option<String> {
    if !raw.contains("priority=MESSAGE_PRIORITY_HIGH content=") {
        return None;
    }
    let idx = raw.find("content=")?;
    let body = raw[idx + "content=".len()..].trim();
    let body = body.strip_suffix("</SYSTEM_MESSAGE>").unwrap_or(body).trim();
    if body.is_empty() || body.starts_with("Task id ") || body.starts_with("[Notice]") {
        return None;
    }
    Some(body.to_string())
}

fn clean_transcript_markup(raw: &str) -> String {
    let without_meta = if let Some(idx) = raw.find("<ADDITIONAL_METADATA>") {
        raw[..idx].trim()
    } else {
        raw
    };
    without_meta
        .replace("<USER_REQUEST>", "")
        .replace("</USER_REQUEST>", "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_arbitrary_paths_outside_gemini() {
        let adapter = AntigravitySessionAdapter;
        let err = adapter
            .read_transcript_tail("/etc/passwd", 10)
            .unwrap_err();
        assert!(
            err.contains("outside allowed") || err.contains("missing"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn filters_generic_noise_and_formats_user_and_assistant() {
        let generic = json!({
            "type": "GENERIC",
            "content": "Created At: 2026-11-25\nCommand exited with code 0"
        });
        assert!(extract_transcript_message(&generic).is_none());

        let sys_reminder = json!({
            "type": "SYSTEM_MESSAGE",
            "content": "Reminder: work efficiently."
        });
        assert!(extract_transcript_message(&sys_reminder).is_none());

        let user = json!({
            "type": "USER_INPUT",
            "content": "<USER_REQUEST>\nhello world\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nlocal time\n</ADDITIONAL_METADATA>"
        });
        let parsed_user = extract_transcript_message(&user).unwrap();
        assert_eq!(parsed_user.role, "You");
        assert_eq!(parsed_user.text, "hello world");

        let assistant = json!({
            "type": "PLANNER_RESPONSE",
            "content": "### Title\n- **Item**",
            "tool_calls": [
                { "name": "read_file", "args": { "Path": "\"/tmp/state.rs\"" } },
                { "name": "run_command", "args": { "toolSummary": "\"Verify checks\"" } }
            ]
        });
        let parsed_asst = extract_transcript_message(&assistant).unwrap();
        assert_eq!(parsed_asst.role, "Assistant");
        assert!(parsed_asst.text.contains("### Title"));
        assert_eq!(
            parsed_asst.steps,
            vec!["Analyzed state.rs".to_string(), "Ran Verify checks".to_string()]
        );
    }
}
