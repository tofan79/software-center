// daemon/settings.rs — Load/save software center settings
// Mirrors the settings dict used throughout the Python daemon

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".config/software-center/settings.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Check interval in minutes. 0 = manual only, default = 1440 (daily).
    pub update_interval: u64,
    pub auto_check_packages: bool,
    pub auto_check_flatpak: bool,
    pub auto_check_appimages: bool,
    /// Auto-install package + flatpak updates.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            update_interval: 1440,
            auto_check_packages: true,
            auto_check_flatpak: true,
            auto_check_appimages: true,
            auto_update: false,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = settings_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Effective check interval clamped to minimum 60 minutes.
    pub fn effective_interval_secs(&self) -> Option<u64> {
        if self.update_interval == 0 {
            return None; // manual only
        }
        Some(self.update_interval.max(60) * 60)
    }
}
