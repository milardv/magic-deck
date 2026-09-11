use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::Settings;

#[cfg(unix)]
pub fn default_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/Steam/steamapps/compatdata/2141910/pfx/drive_c/users/steamuser/AppData/LocalLow/Wizards Of The Coast/MTGA/Player.log")
}

#[cfg(windows)]
pub fn default_log_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("AppData/LocalLow/Wizards Of The Coast/MTGA/Player.log")
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("magic-deck/config.json")
}

pub fn load() -> Settings {
    let path = config_path();
    let mut settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_else(|| Settings {
            log_path: default_log_path().to_string_lossy().into_owned(),
            gemini_api_key: None,
        });
    if let Ok(log_path) = std::env::var("MTGA_LOG_PATH") {
        if !log_path.trim().is_empty() {
            settings.log_path = log_path;
        }
    }
    settings
}

pub fn save(settings: &Settings) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let temporary = temporary_path(&path);
    std::fs::write(&temporary, serde_json::to_vec_pretty(settings)?)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(
        &temporary,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    std::fs::rename(&temporary, &path)
        .with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}
