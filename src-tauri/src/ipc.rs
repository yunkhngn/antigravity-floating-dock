use serde::Deserialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::models::normalize_hook_payload;
use crate::paths;
use crate::state::AppState;

const MAX_IPC_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
struct IpcEnvelope {
    token: String,
    payload: serde_json::Value,
}

fn generate_hex_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    let mut f = File::open("/dev/urandom")
        .map_err(|e| format!("Failed to open /dev/urandom for IPC token: {e}"))?;
    f.read_exact(&mut bytes)
        .map_err(|e| format!("Failed to read random bytes for IPC token: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in ab.iter().zip(bb.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn cleanup_ipc_files() {
    let _ = fs::remove_file(paths::socket_path());
    let _ = fs::remove_file(paths::token_path());
}

pub async fn start_ipc_server(
    app_handle: tauri::AppHandle,
    state: Arc<AppState>,
) -> Result<PathBuf, String> {
    let run_dir = paths::run_dir();
    paths::ensure_dir_with_mode(&run_dir, 0o700)
        .map_err(|e| format!("Failed to create IPC run dir: {e}"))?;

    let sock_path = paths::socket_path();
    let token_path = paths::token_path();

    // Remove stale socket if an earlier run crashed.
    if sock_path.exists() {
        let _ = fs::remove_file(&sock_path);
    }

    let token = generate_hex_token()?;
    fs::write(&token_path, format!("{token}\n"))
        .map_err(|e| format!("Failed to write IPC token: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&token_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set IPC token permissions: {e}"))?;
    }

    let listener = UnixListener::bind(&sock_path)
        .map_err(|e| format!("Failed to bind Unix socket {}: {e}", sock_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set IPC socket permissions: {e}"))?;
    }

    state.set_ipc_listening(true, sock_path.display().to_string());
    state.logger.log("INFO", "Local Unix IPC listener started");

    let expected_token = Arc::new(token);
    tauri::async_runtime::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let st = Arc::clone(&state);
                    let tok = Arc::clone(&expected_token);
                    let app = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = handle_client(stream, tok, st, app).await;
                    });
                }
                Err(err) => {
                    state
                        .logger
                        .log("WARN", &format!("IPC accept error: {err}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });

    Ok(sock_path)
}

async fn handle_client(
    stream: UnixStream,
    expected_token: Arc<String>,
    state: Arc<AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), ()> {
    let (reader, mut writer) = stream.into_split();
    let mut limited = tokio::io::AsyncReadExt::take(reader, MAX_IPC_LINE_BYTES as u64 + 1);
    let mut buf_reader = BufReader::new(&mut limited);
    let mut line = String::new();

    let read_res = tokio::time::timeout(Duration::from_millis(300), buf_reader.read_line(&mut line))
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;

    if read_res == 0 || line.len() > MAX_IPC_LINE_BYTES {
        let _ = writer.write_all(b"{\"ok\":false,\"error\":\"invalid_size\"}\n").await;
        return Ok(());
    }

    let envelope: IpcEnvelope = match serde_json::from_str(line.trim_end()) {
        Ok(env) => env,
        Err(_) => {
            state.logger.log("WARN", "Rejected malformed IPC JSON frame");
            let _ = writer.write_all(b"{\"ok\":false,\"error\":\"malformed_json\"}\n").await;
            return Ok(());
        }
    };

    if !constant_time_eq(envelope.token.trim(), expected_token.as_str()) {
        state.logger.log("WARN", "Rejected IPC message with invalid token");
        let _ = writer.write_all(b"{\"ok\":false,\"error\":\"unauthorized\"}\n").await;
        return Ok(());
    }

    let seq = state.next_seq();
    match normalize_hook_payload(&envelope.payload, now_ms(), seq) {
        Ok(event) => {
            state.ingest_event(event, Some(&app_handle));
            let _ = writer.write_all(b"{\"ok\":true}\n").await;
        }
        Err(err) => {
            state
                .logger
                .log("WARN", &format!("Rejected hook payload: {err}"));
            let _ = writer.write_all(b"{\"ok\":false,\"error\":\"invalid_payload\"}\n").await;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_checks_exact_match() {
        assert!(constant_time_eq("abcdef", "abcdef"));
        assert!(!constant_time_eq("abcdef", "abcdeg"));
        assert!(!constant_time_eq("abc", "abcd"));
    }
}
