use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::models::ConversationSummary;
use crate::paths;

const PLUGIN_DIR_NAME: &str = "antigravity-floating-dock";
const LEGACY_SIDECAR_DIR_NAME: &str = "antigravity-floating-dock-bridge";
const BRIDGE_SCRIPT: &str = include_str!("../../resources/sidecar/bridge.mjs");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarStatus {
    pub installed: bool,
    pub connected: bool,
    pub sidecar_dir: String,
    pub node_path: Option<String>,
}

pub fn plugin_dir() -> PathBuf {
    paths::agy_config_dir()
        .join("plugins")
        .join(PLUGIN_DIR_NAME)
}

pub fn plugin_manifest_path() -> PathBuf {
    plugin_dir().join("plugin.json")
}

pub fn sidecar_dir() -> PathBuf {
    plugin_dir().join("sidecars").join("bridge")
}

pub fn sidecar_manifest_path() -> PathBuf {
    sidecar_dir().join("sidecar.json")
}

pub fn sidecar_script_path() -> PathBuf {
    sidecar_dir().join("bridge.mjs")
}

pub fn bridge_socket_path() -> PathBuf {
    paths::run_dir().join("sidecar-bridge.sock")
}

pub fn bridge_token_path() -> PathBuf {
    paths::run_dir().join("sidecar-bridge.token")
}

pub fn install_sidecar() -> Result<SidecarStatus, String> {
    // Clean up legacy global sidecars path if an earlier version created it.
    let legacy_dir = paths::agy_config_dir()
        .join("sidecars")
        .join(LEGACY_SIDECAR_DIR_NAME);
    if legacy_dir.exists() {
        let _ = fs::remove_dir_all(&legacy_dir);
    }

    let p_dir = plugin_dir();
    let s_dir = sidecar_dir();
    fs::create_dir_all(&s_dir).map_err(|e| format!("Failed to create plugin sidecar directory: {e}"))?;

    let plugin_manifest = json!({
        "name": PLUGIN_DIR_NAME,
        "displayName": "Antigravity Floating Dock",
        "version": "0.1.0",
        "description": "Background message bridge plugin for Antigravity Floating Dock."
    });
    let plugin_str = serde_json::to_string_pretty(&plugin_manifest)
        .map_err(|e| format!("Failed to serialize plugin.json: {e}"))?;
    write_atomically(&p_dir.join("plugin.json"), &format!("{plugin_str}\n"))?;

    // Antigravity intercepts "command": "node" to launch via its built-in Electron Node + sidecar_sdk loader.
    let sidecar_manifest = json!({
        "command": "node",
        "args": ["bridge.mjs"],
        "restart_policy": "always",
        "display_name": "Floating Dock Bridge",
        "description": "Background message bridge for Antigravity Floating Dock",
        "has_web_ui": true,
        "agent_permissions": {
            "allows_user_interactions": true
        },
        "ui_config": {
            "display_name": "Floating Dock Bridge",
            "views": [
                {
                    "path": "/",
                    "entrypoint": "SIDECAR_UI_ENTRYPOINT_AUX_PANE",
                    "title": "Floating Dock Bridge"
                }
            ]
        }
    });

    let manifest_str = serde_json::to_string_pretty(&sidecar_manifest)
        .map_err(|e| format!("Failed to serialize sidecar.json: {e}"))?;
    write_atomically(&sidecar_manifest_path(), &format!("{manifest_str}\n"))?;
    write_atomically(&sidecar_script_path(), BRIDGE_SCRIPT)?;

    Ok(inspect_sidecar_status())
}

fn write_atomically(dest: &PathBuf, content: &str) -> Result<(), String> {
    let tmp = dest.with_extension("tmp");
    fs::write(&tmp, content).map_err(|e| format!("Failed to write {}: {e}", dest.display()))?;
    fs::rename(&tmp, dest).map_err(|e| format!("Failed to finalize {}: {e}", dest.display()))
}

pub fn uninstall_sidecar() -> Result<(), String> {
    let legacy_dir = paths::agy_config_dir()
        .join("sidecars")
        .join(LEGACY_SIDECAR_DIR_NAME);
    if legacy_dir.exists() {
        let _ = fs::remove_dir_all(&legacy_dir);
    }

    let p_dir = plugin_dir();
    if p_dir.exists() {
        fs::remove_dir_all(&p_dir)
            .map_err(|e| format!("Failed to remove plugin sidecar directory: {e}"))?;
    }
    let _ = fs::remove_file(bridge_socket_path());
    let _ = fs::remove_file(bridge_token_path());
    Ok(())
}

pub fn inspect_sidecar_status() -> SidecarStatus {
    let installed = plugin_manifest_path().is_file()
        && sidecar_manifest_path().is_file()
        && sidecar_script_path().is_file();
    let connected = ping_bridge();
    SidecarStatus {
        installed,
        connected,
        sidecar_dir: sidecar_dir().display().to_string(),
        node_path: Some("node (Antigravity managed)".to_string()),
    }
}

pub fn ping_bridge() -> bool {
    let payload = json!({ "action": "ping" });
    call_bridge(payload, Duration::from_millis(200))
        .map(|resp| resp.get("ok").and_then(|v| v.as_bool()) == Some(true))
        .unwrap_or(false)
}

pub fn list_conversations() -> Result<Vec<ConversationSummary>, String> {
    let payload = json!({ "action": "list_conversations" });
    let resp = call_bridge(payload, Duration::from_millis(2000))?;
    if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        let err = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Failed to list conversations");
        return Err(err.to_string());
    }
    let val = resp
        .get("conversations")
        .cloned()
        .unwrap_or_else(|| json!([]));
    serde_json::from_value::<Vec<ConversationSummary>>(val)
        .map_err(|e| format!("Invalid conversations payload: {e}"))
}

pub fn send_message_via_sidecar(conversation_id: &str, message: &str) -> Result<(), String> {
    let payload = json!({
        "action": "send_message",
        "conversationId": conversation_id,
        "message": message,
    });

    let resp = call_bridge(payload, Duration::from_secs(8))?;
    if resp.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        Ok(())
    } else {
        let err = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Sidecar bridge rejected send_message");
        Err(err.to_string())
    }
}

fn call_bridge(mut body: Value, read_timeout: Duration) -> Result<Value, String> {
    let sock = bridge_socket_path();
    let tok_path = bridge_token_path();
    if !sock.exists() || !tok_path.exists() {
        return Err("Sidecar bridge is not running".to_string());
    }

    let token = fs::read_to_string(&tok_path)
        .map_err(|_| "Cannot read sidecar bridge token".to_string())?;
    let token = token.trim();
    if token.is_empty() {
        return Err("Sidecar bridge token is empty".to_string());
    }

    if let Some(obj) = body.as_object_mut() {
        obj.insert("token".to_string(), Value::String(token.to_string()));
    }

    let mut stream = UnixStream::connect(&sock)
        .map_err(|_| "Sidecar bridge socket is unavailable".to_string())?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
    let _ = stream.set_read_timeout(Some(read_timeout));

    let mut encoded = serde_json::to_vec(&body)
        .map_err(|e| format!("Failed to encode bridge request: {e}"))?;
    encoded.push(b'\n');
    stream
        .write_all(&encoded)
        .map_err(|e| format!("Failed to write to sidecar bridge: {e}"))?;
    let _ = stream.flush();

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("Failed to read sidecar bridge response: {e}"))?;

    serde_json::from_str::<Value>(line.trim())
        .map_err(|e| format!("Invalid JSON from sidecar bridge: {e}"))
}
