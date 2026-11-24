use serde_json::{Map, Value};

const SENSITIVE_KEY_FRAGMENTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "authorization",
    "csrf",
    "cookie",
    "credential",
    "private_key",
    "bearer",
];

const MAX_STRING_LEN: usize = 240;

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEY_FRAGMENTS
        .iter()
        .any(|fragment| lower.contains(fragment))
}

pub fn redact_string(input: &str) -> String {
    redact_text_with_limit(input, MAX_STRING_LEN)
}

pub fn redact_text_with_limit(input: &str, max_chars: usize) -> String {
    let mut cleaned = input.to_string();
    for prefix in ["Bearer ", "bearer ", "AIza", "ghp_", "gho_", "sk-"] {
        if let Some(pos) = cleaned.find(prefix) {
            let head = &cleaned[..pos];
            return format!("{head}[REDACTED]");
        }
    }
    if cleaned.chars().count() > max_chars {
        cleaned = cleaned.chars().take(max_chars).collect::<String>();
        cleaned.push_str("…");
    }
    cleaned
}

pub fn redact_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                if is_sensitive_key(k) {
                    out.insert(k.clone(), Value::String("[REDACTED]".to_string()));
                } else {
                    out.insert(k.clone(), redact_json(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().take(20).map(redact_json).collect()),
        Value::String(s) => Value::String(redact_string(s)),
        other => other.clone(),
    }
}

pub fn redact_log_line(line: &str) -> String {
    redact_string(line).replace('\n', " ").replace('\r', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redacts_sensitive_keys_and_tokens() {
        let raw = json!({
            "hookEventName": "PostToolUse",
            "csrfToken": "abc123",
            "nested": {
                "authorization": "Bearer supersecret",
                "note": "Use sk-1234567890"
            }
        });
        let redacted = redact_json(&raw);
        assert_eq!(redacted["csrfToken"], "[REDACTED]");
        assert_eq!(redacted["nested"]["authorization"], "[REDACTED]");
        assert_eq!(redacted["nested"]["note"], "Use [REDACTED]");
    }
}
