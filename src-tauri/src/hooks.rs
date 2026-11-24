use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;

const HOOK_KEY: &str = "antigravity-floating-dock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub hooks_file_path: String,
    pub hooks_file_exists: bool,
    pub hook_installed: bool,
    pub hook_enabled: bool,
    pub relay_path: String,
    pub relay_installed: bool,
    pub snippet: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookMutationResult {
    pub status: HookStatus,
    pub backup_path: Option<String>,
    pub message: String,
}

fn find_source_relay_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join("agy-dock-hook-relay");
    if candidate.is_file() {
        return Some(candidate);
    }
    None
}

pub fn sync_relay_binary() -> Result<PathBuf, String> {
    let bin_dir = paths::bin_dir();
    paths::ensure_dir_with_mode(&bin_dir, 0o755)
        .map_err(|e| format!("Failed to create relay bin directory: {e}"))?;

    let dest = paths::relay_bin_path();
    if let Some(src) = find_source_relay_binary() {
        let tmp = dest.with_extension("tmp");
        fs::copy(&src, &tmp).map_err(|e| format!("Failed to copy hook relay binary: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed to set relay permissions: {e}"))?;
        }
        fs::rename(&tmp, &dest).map_err(|e| format!("Failed to install hook relay: {e}"))?;
        return Ok(dest);
    }

    if dest.is_file() {
        return Ok(dest);
    }

    Err("Could not locate compiled agy-dock-hook-relay binary next to executable".to_string())
}

pub fn build_hook_entry(relay_command: &str) -> Value {
    json!({
        "enabled": true,
        "SessionStart": [
            {
                "type": "command",
                "command": relay_command,
                "timeout": 5
            }
        ],
        "PreInvocation": [
            {
                "type": "command",
                "command": relay_command,
                "timeout": 5
            }
        ],
        "PostToolUse": [
            {
                "matcher": "*",
                "hooks": [
                    {
                        "type": "command",
                        "command": relay_command,
                        "timeout": 5
                    }
                ]
            }
        ],
        "PostInvocation": [
            {
                "type": "command",
                "command": relay_command,
                "timeout": 5
            }
        ],
        "Stop": [
            {
                "type": "command",
                "command": relay_command,
                "timeout": 5
            }
        ]
    })
}

pub fn copyable_hook_snippet() -> String {
    let relay = paths::relay_bin_path();
    let cmd = format!("\"{}\"", relay.display());
    let wrapper = json!({
        HOOK_KEY: build_hook_entry(&cmd)
    });
    serde_json::to_string_pretty(&wrapper).unwrap_or_default()
}

pub fn inspect_hook_status() -> HookStatus {
    let hooks_path = paths::agy_hooks_path();
    let relay_path = paths::relay_bin_path();
    let relay_installed = is_executable_file(&relay_path);
    let snippet = copyable_hook_snippet();

    if !hooks_path.exists() {
        return HookStatus {
            hooks_file_path: hooks_path.display().to_string(),
            hooks_file_exists: false,
            hook_installed: false,
            hook_enabled: false,
            relay_path: relay_path.display().to_string(),
            relay_installed,
            snippet,
            error: None,
        };
    }

    match fs::read_to_string(&hooks_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(Value::Object(map)) => {
                let entry = map.get(HOOK_KEY);
                let hook_installed = entry.is_some();
                let hook_enabled = entry
                    .and_then(|v| v.get("enabled"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(hook_installed);
                HookStatus {
                    hooks_file_path: hooks_path.display().to_string(),
                    hooks_file_exists: true,
                    hook_installed,
                    hook_enabled,
                    relay_path: relay_path.display().to_string(),
                    relay_installed,
                    snippet,
                    error: None,
                }
            }
            Ok(_) => HookStatus {
                hooks_file_path: hooks_path.display().to_string(),
                hooks_file_exists: true,
                hook_installed: false,
                hook_enabled: false,
                relay_path: relay_path.display().to_string(),
                relay_installed,
                snippet,
                error: Some("Existing hooks.json root must be a JSON object".to_string()),
            },
            Err(e) => HookStatus {
                hooks_file_path: hooks_path.display().to_string(),
                hooks_file_exists: true,
                hook_installed: false,
                hook_enabled: false,
                relay_path: relay_path.display().to_string(),
                relay_installed,
                snippet,
                error: Some(format!("Existing hooks.json is not valid JSON: {e}")),
            },
        },
        Err(e) => HookStatus {
            hooks_file_path: hooks_path.display().to_string(),
            hooks_file_exists: true,
            hook_installed: false,
            hook_enabled: false,
            relay_path: relay_path.display().to_string(),
            relay_installed,
            snippet,
            error: Some(format!("Cannot read hooks.json: {e}")),
        },
    }
}

fn is_executable_file(path: &PathBuf) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    true
}

fn backup_existing_hooks(hooks_path: &PathBuf) -> Result<Option<String>, String> {
    if !hooks_path.exists() {
        return Ok(None);
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = hooks_path.with_file_name(format!("hooks.json.bak.{ts}"));
    fs::copy(hooks_path, &backup).map_err(|e| format!("Failed to back up hooks.json: {e}"))?;
    Ok(Some(backup.display().to_string()))
}

fn write_hooks_map_atomically(hooks_path: &PathBuf, map: &Map<String, Value>) -> Result<(), String> {
    let parent = hooks_path
        .parent()
        .ok_or_else(|| "Invalid hooks.json parent path".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create ~/.gemini/config: {e}"))?;

    let formatted = serde_json::to_string_pretty(&Value::Object(map.clone()))
        .map_err(|e| format!("Failed to serialize hooks.json: {e}"))?;
    let tmp = hooks_path.with_extension("json.tmp");
    fs::write(&tmp, format!("{formatted}\n"))
        .map_err(|e| format!("Failed to write temporary hooks file: {e}"))?;
    fs::rename(&tmp, hooks_path).map_err(|e| format!("Failed to replace hooks.json: {e}"))?;
    Ok(())
}

pub fn install_hook() -> Result<HookMutationResult, String> {
    let relay = sync_relay_binary()?;
    let hooks_path = paths::agy_hooks_path();

    let mut root_map = if hooks_path.exists() {
        let raw = fs::read_to_string(&hooks_path)
            .map_err(|e| format!("Could not read existing hooks.json: {e}"))?;
        if raw.trim().is_empty() {
            Map::new()
        } else {
            match serde_json::from_str::<Value>(&raw) {
                Ok(Value::Object(map)) => map,
                Ok(_) => {
                    return Err(
                        "Existing hooks.json is not a JSON object; refusing to overwrite."
                            .to_string(),
                    )
                }
                Err(e) => {
                    return Err(format!(
                        "Existing hooks.json contains invalid JSON ({e}); please fix or install manually."
                    ))
                }
            }
        }
    } else {
        Map::new()
    };

    let backup_path = backup_existing_hooks(&hooks_path)?;
    let cmd = format!("\"{}\"", relay.display());
    root_map.insert(HOOK_KEY.to_string(), build_hook_entry(&cmd));
    write_hooks_map_atomically(&hooks_path, &root_map)?;

    let status = inspect_hook_status();
    Ok(HookMutationResult {
        status,
        backup_path,
        message: "Installed antigravity-floating-dock hook into ~/.gemini/config/hooks.json."
            .to_string(),
    })
}

pub fn uninstall_hook() -> Result<HookMutationResult, String> {
    let hooks_path = paths::agy_hooks_path();
    if !hooks_path.exists() {
        return Ok(HookMutationResult {
            status: inspect_hook_status(),
            backup_path: None,
            message: "hooks.json does not exist; nothing to remove.".to_string(),
        });
    }

    let raw = fs::read_to_string(&hooks_path)
        .map_err(|e| format!("Could not read hooks.json: {e}"))?;
    let mut root_map = match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(map)) => map,
        Ok(_) => {
            return Err("hooks.json root is not a JSON object; refusing to modify.".to_string())
        }
        Err(e) => return Err(format!("hooks.json is invalid JSON: {e}")),
    };

    let backup_path = backup_existing_hooks(&hooks_path)?;
    let removed = root_map.remove(HOOK_KEY).is_some();
    if removed {
        write_hooks_map_atomically(&hooks_path, &root_map)?;
    }

    let status = inspect_hook_status();
    Ok(HookMutationResult {
        status,
        backup_path,
        message: if removed {
            "Removed antigravity-floating-dock entry while preserving existing hooks.".to_string()
        } else {
            "antigravity-floating-dock entry was not present in hooks.json.".to_string()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_removes_only_dock_key() {
        let mut map = serde_json::from_value::<Map<String, Value>>(json!({
            "user-linter": {
                "PostToolUse": []
            }
        }))
        .unwrap();

        map.insert(HOOK_KEY.to_string(), build_hook_entry("\"/tmp/relay\""));
        assert!(map.contains_key("user-linter"));
        assert!(map.contains_key(HOOK_KEY));

        map.remove(HOOK_KEY);
        assert!(map.contains_key("user-linter"));
        assert!(!map.contains_key(HOOK_KEY));
    }
}
