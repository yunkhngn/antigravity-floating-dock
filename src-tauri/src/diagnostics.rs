use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::paths;
use crate::redact::redact_log_line;

const MAX_MEMORY_LOG_LINES: usize = 80;

pub struct DiagnosticLogger {
    lines: Mutex<VecDeque<String>>,
}

impl DiagnosticLogger {
    pub fn new() -> Self {
        Self {
            lines: Mutex::new(VecDeque::with_capacity(MAX_MEMORY_LOG_LINES)),
        }
    }

    pub fn log(&self, level: &str, message: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = format!("[{ts}] [{level}] {}", redact_log_line(message));

        if let Ok(mut guard) = self.lines.lock() {
            if guard.len() >= MAX_MEMORY_LOG_LINES {
                guard.pop_front();
            }
            guard.push_back(entry.clone());
        }

        let log_dir = paths::logs_dir();
        if paths::ensure_dir_with_mode(&log_dir, 0o700).is_ok() {
            let log_file = paths::diagnostic_log_path();
            // Rotate if log exceeds 512 KiB
            if let Ok(meta) = fs::metadata(&log_file) {
                if meta.len() > 512 * 1024 {
                    let _ = fs::rename(&log_file, log_dir.join("dock.log.prev"));
                }
            }
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_file) {
                let _ = writeln!(file, "{entry}");
            }
        }
    }

    pub fn recent_lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .map(|g| g.iter().cloned().collect())
            .unwrap_or_default()
    }
}
