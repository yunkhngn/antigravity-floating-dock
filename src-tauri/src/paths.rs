use std::fs;
use std::path::{Path, PathBuf};

const APP_SUPPORT_DIR_NAME: &str = "com.antigravity.floating-dock";

pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn app_support_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| home_dir().join("Library/Application Support"))
        .join(APP_SUPPORT_DIR_NAME)
}

pub fn run_dir() -> PathBuf {
    app_support_dir().join("run")
}

pub fn socket_path() -> PathBuf {
    run_dir().join("dock.sock")
}

pub fn token_path() -> PathBuf {
    run_dir().join("dock.token")
}

pub fn bin_dir() -> PathBuf {
    app_support_dir().join("bin")
}

pub fn relay_bin_path() -> PathBuf {
    bin_dir().join("agy-dock-hook-relay")
}

pub fn settings_path() -> PathBuf {
    app_support_dir().join("settings.json")
}

pub fn logs_dir() -> PathBuf {
    app_support_dir().join("logs")
}

pub fn diagnostic_log_path() -> PathBuf {
    logs_dir().join("dock.log")
}

pub fn agy_config_dir() -> PathBuf {
    home_dir().join(".gemini/config")
}

pub fn agy_hooks_path() -> PathBuf {
    agy_config_dir().join("hooks.json")
}

pub fn allowed_transcript_roots() -> Vec<PathBuf> {
    let gemini = home_dir().join(".gemini");
    vec![
        gemini.join("antigravity"),
        gemini.join("antigravity-cli"),
        gemini.join("antigravity-ide"),
    ]
}

pub fn ensure_dir_with_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    Ok(())
}
