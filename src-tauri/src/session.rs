use serde_json::Value;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;

use crate::models::{SessionCapabilities, TranscriptMessage};
use crate::paths;
use crate::redact::redact_string;

const MAX_TAIL_READ_BYTES: u64 = 64 * 1024;

pub trait SessionAdapter: Send + Sync {
    fn capabilities(&self, source_product: &str) -> SessionCapabilities;
    fn read_transcript_tail(
        &self,
        transcript_path: &str,
        max_items: usize,
    ) -> Result<Vec<TranscriptMessage>, String>;
    fn open_antigravity(&self, source_product: &str) -> Result<(), String>;
    fn is_antigravity_running(&self) -> bool;
}

pub struct AntigravitySessionAdapter;

impl SessionAdapter for AntigravitySessionAdapter {
    fn capabilities(&self, source_product: &str) -> SessionCapabilities {
        let limitation_reason = match source_product {
            "antigravity-cli" => {
                "Live bidirectional injection into an active Antigravity CLI turn is not enabled in this read-only dock. Use Open in Antigravity or Copy message."
            }
            "antigravity-ide" => {
                "Antigravity IDE does not expose a verified external API for sending messages into an active session."
            }
            _ => {
                "Antigravity 2.0 does not expose a supported external API to send messages into an existing session. Use Open in Antigravity or Copy message."
            }
        };

        SessionCapabilities {
            source_product: source_product.to_string(),
            can_send_message: false,
            limitation_reason: limitation_reason.to_string(),
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

        let mut parsed = Vec::new();
        let lines: Vec<&str> = raw.lines().collect();
        // If we seeked mid-file, skip the first potentially truncated line.
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
                if let Some(msg) = extract_transcript_message(&val) {
                    parsed.push(msg);
                }
            }
        }

        if parsed.len() > max_items {
            parsed.drain(0..(parsed.len() - max_items));
        }
        Ok(parsed)
    }

    fn open_antigravity(&self, source_product: &str) -> Result<(), String> {
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

        if status.success() {
            Ok(())
        } else {
            Err(format!("Could not focus {app_name} (exit status {status})"))
        }
    }

    fn is_antigravity_running(&self) -> bool {
        Command::new("pgrep")
            .args(["-f", "/Applications/Antigravity"])
            .output()
            .map(|out| out.status.success() && !out.stdout.is_empty())
            .unwrap_or(false)
    }
}

fn extract_transcript_message(val: &Value) -> Option<TranscriptMessage> {
    let obj = val.as_object()?;
    let role = obj
        .get("role")
        .or_else(|| obj.get("type"))
        .or_else(|| obj.get("author"))
        .and_then(|v| v.as_str())
        .unwrap_or("event")
        .to_string();

    let text = obj
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
        })?;

    let cleaned = redact_string(text.trim());
    if cleaned.is_empty() {
        return None;
    }

    let timestamp = obj
        .get("timestamp")
        .or_else(|| obj.get("createdAt"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    Some(TranscriptMessage {
        role,
        text: cleaned,
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
