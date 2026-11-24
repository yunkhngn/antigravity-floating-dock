use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

const MAX_STDIN_BYTES: usize = 60 * 1024;

fn run_dir() -> PathBuf {
    if let Some(base) = dirs::data_local_dir() {
        base.join("com.antigravity.floating-dock").join("run")
    } else if let Some(home) = dirs::home_dir() {
        home.join("Library/Application Support/com.antigravity.floating-dock/run")
    } else {
        PathBuf::from("/tmp/com.antigravity.floating-dock/run")
    }
}

fn send_event() -> Result<(), ()> {
    let mut stdin_buf = Vec::with_capacity(4096);
    let mut handle = std::io::stdin().take(MAX_STDIN_BYTES as u64);
    handle.read_to_end(&mut stdin_buf).map_err(|_| ())?;
    if stdin_buf.is_empty() {
        return Ok(());
    }

    let payload: serde_json::Value = serde_json::from_slice(&stdin_buf).map_err(|_| ())?;
    let dir = run_dir();
    let sock_path = dir.join("dock.sock");
    let token_path = dir.join("dock.token");

    if !sock_path.exists() || !token_path.exists() {
        return Ok(());
    }

    let token = fs::read_to_string(token_path).map_err(|_| ())?;
    let token = token.trim();
    if token.is_empty() {
        return Ok(());
    }

    let mut stream = UnixStream::connect(&sock_path).map_err(|_| ())?;
    let _ = stream.set_write_timeout(Some(Duration::from_millis(150)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(150)));

    let envelope = serde_json::json!({
        "token": token,
        "payload": payload,
    });
    let mut encoded = serde_json::to_vec(&envelope).map_err(|_| ())?;
    encoded.push(b'\n');

    stream.write_all(&encoded).map_err(|_| ())?;
    let _ = stream.flush();

    let mut reader = BufReader::new(stream);
    let mut ack = String::new();
    let _ = reader.read_line(&mut ack);
    Ok(())
}

fn main() {
    let _ = send_event();
    // Always return an empty JSON object so Antigravity hooks remain passive and non-blocking.
    println!("{{}}");
}
