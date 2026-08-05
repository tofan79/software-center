// daemon/checker.rs — Background update check logic

use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

// Package check runs `dnf5 check-update` which can be slow on first run
// or on a slow mirror — allow up to 3 minutes.
const PKG_TIMEOUT: Duration = Duration::from_secs(180);
// Flatpak and other checks are faster; 60s is plenty.
const CMD_TIMEOUT: Duration = Duration::from_secs(60);
const GNOME_EXTENSIONS_BASE: &str = "https://extensions.gnome.org";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateResult {
    pub packages: Vec<serde_json::Value>,
    pub flatpak: Vec<serde_json::Value>,
    pub gnome_extensions: Vec<serde_json::Value>,
    pub appimages: Vec<serde_json::Value>,
    pub total: usize,
}

impl UpdateResult {
    pub fn notification_body(&self) -> Option<String> {
        if self.total == 0 {
            return None;
        }
        let mut parts = Vec::new();
        if !self.packages.is_empty() {
            let n = self.packages.len();
            parts.push(format!("{} package update{}", n, if n != 1 { "s" } else { "" }));
        }
        if !self.flatpak.is_empty() {
            let n = self.flatpak.len();
            parts.push(format!("{} Flatpak update{}", n, if n != 1 { "s" } else { "" }));
        }
        if !self.appimages.is_empty() {
            let n = self.appimages.len();
            parts.push(format!("{} AppImage update{}", n, if n != 1 { "s" } else { "" }));
        }
        if !self.gnome_extensions.is_empty() {
            let n = self.gnome_extensions.len();
            parts.push(format!("{} GNOME extension update{}", n, if n != 1 { "s" } else { "" }));
        }
        Some(parts.join("\n"))
    }
}

/// Run all checks in parallel so total time ≈ slowest single check.
pub async fn run_checks(settings: &Settings) -> UpdateResult {
    let check_pkgs   = settings.auto_check_packages;
    let check_fp     = settings.auto_check_flatpak;
    let check_ai     = settings.auto_check_appimages;
    let check_ext    = true;
    let auto_update  = settings.auto_update;

    log::info!("Starting update checks (packages={check_pkgs}, flatpak={check_fp}, appimages={check_ai}, gnome_extensions={check_ext})");

    // Run all checks concurrently
    let (pkg_res, fp_res, ai_res, ext_res) = tokio::join!(
        async {
            if check_pkgs {
                log::info!("Running dnf5 check-update");
                let res = run_scenter_update_with_timeout(PKG_TIMEOUT).await;
                match &res {
                    Ok((_, pkgs)) => log::info!("Package check done: {} update(s)", pkgs.len()),
                    Err(e)        => log::warn!("Package check failed: {}", e),
                }
                res.ok()
            } else {
                log::info!("Package check skipped (disabled in settings)");
                None
            }
        },
        async {
            if check_fp {
                log::info!("Checking flatpak updates via Rust backend (icon-enriched)");
                let updates: Vec<serde_json::Value> =
                    tokio::task::spawn_blocking(|| {
                        scenter_flatpak::get_all_updates()
                            .into_iter()
                            .filter_map(|f| serde_json::to_value(f).ok())
                            .collect()
                    })
                    .await
                    .unwrap_or_default();
                log::info!("Flatpak check done: {} update(s)", updates.len());
                Some((true, updates))
            } else {
                log::info!("Flatpak check skipped (disabled in settings)");
                None
            }
        },
        async {
            if check_ai {
                log::info!("Running AppImage update checks");
                let res = check_appimages().await;
                log::info!("AppImage check done: {} update(s)", res.len());
                res
            } else {
                log::info!("AppImage check skipped (disabled in settings)");
                vec![]
            }
        },
        async {
            if check_ext {
                log::info!("Running GNOME extension update checks");
                let updates = check_gnome_extension_updates().await;
                log::info!("GNOME extension check done: {} update(s)", updates.len());
                Some((true, updates))
            } else {
                None
            }
        },
    );

    let raw_packages = pkg_res.map(|(_, v)| v).unwrap_or_default();
    let packages = tokio::task::spawn_blocking(move || {
        scenter_updates::enrich_package_updates(raw_packages)
    })
    .await
    .unwrap_or_default();

    let mut result = UpdateResult {
        packages,
        flatpak:         fp_res.map(|(_, v)| v).unwrap_or_default(),
        gnome_extensions: ext_res.map(|(_, v)| v).unwrap_or_default(),
        appimages:       ai_res,
        total: 0,
    };

    result.total = result.packages.len()
        + result.flatpak.len()
        + result.gnome_extensions.len()
        + result.appimages.len();

    // Auto-install if enabled (re-check after to get accurate count)
    if auto_update {
        let (new_pkg, new_fp) = tokio::join!(
            async {
                if !result.packages.is_empty() {
                    let _ = run_command(&["pkexec", "dnf5", "upgrade", "-y"]).await;
                    run_scenter_update().await.ok().map(|(_, v)| v)
                } else {
                    None
                }
            },
            async {
                if !result.flatpak.is_empty() {
                    let _ = run_command(&["flatpak", "update", "-y", "--noninteractive"]).await;
                    let updates: Vec<serde_json::Value> =
                        tokio::task::spawn_blocking(|| {
                            scenter_flatpak::get_all_updates()
                                .into_iter()
                                .filter_map(|f| serde_json::to_value(f).ok())
                                .collect()
                        })
                        .await
                        .ok()
                        .unwrap_or_default();
                    Some(updates)
                } else {
                    None
                }
            },
        );
        if let Some(pkgs) = new_pkg { result.packages = pkgs; }
        if let Some(fps)  = new_fp  { result.flatpak  = fps;  }
        result.total = result.packages.len()
            + result.flatpak.len()
            + result.gnome_extensions.len()
            + result.appimages.len();
    }

    result
}

async fn check_gnome_extension_updates() -> Vec<serde_json::Value> {
    let Some(shell_version) = detect_shell_version() else {
        return Vec::new();
    };
    let base = user_extensions_base();
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };

    let mut updates = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let metadata_path = dir.join("metadata.json");
        let Ok(raw) = fs::read_to_string(metadata_path) else {
            continue;
        };
        let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let uuid = metadata["uuid"]
            .as_str()
            .map(str::to_string)
            .or_else(|| dir.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .unwrap_or_default();
        if uuid.is_empty() {
            continue;
        }

        let installed_version = json_u32(&metadata["version"]).unwrap_or(0);
        let installed_name = metadata["name"].as_str().unwrap_or(&uuid).to_string();
        let Ok(remote) = get_remote_gnome_extension_info(&uuid, &shell_version).await else {
            continue;
        };
        let available_version = json_u32(&remote["version"]).unwrap_or(0);
        if available_version <= installed_version {
            continue;
        }

        let icon_url = remote["icon"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(absolute_gnome_extension_url)
            .unwrap_or_else(|| format!("{}/static/images/plugin.png", GNOME_EXTENSIONS_BASE));
        let name = remote["name"].as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(&installed_name);

        updates.push(serde_json::json!({
            "uuid": uuid,
            "name": name,
            "installed_version": installed_version,
            "available_version": available_version,
            "shell_version": shell_version,
            "icon_url": icon_url,
        }));
    }

    updates.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    updates
}

async fn get_remote_gnome_extension_info(
    uuid: &str,
    shell_version: &str,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/extension-info/?uuid={}&shell_version={}",
        GNOME_EXTENSIONS_BASE,
        url_escape(uuid),
        url_escape(shell_version)
    );
    reqwest::get(&url)
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

fn detect_shell_version() -> Option<String> {
    let out = Command::new("gnome-shell").arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|s| s.to_string())
}

fn user_extensions_base() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/gnome-shell/extensions")
}

fn absolute_gnome_extension_url(url: &str) -> String {
    if url.starts_with("http") {
        url.to_string()
    } else {
        format!("{}{}", GNOME_EXTENSIONS_BASE, url)
    }
}

fn url_escape(input: &str) -> String {
    input
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('@', "%40")
        .replace('+', "%2B")
        .replace('/', "%2F")
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| value.as_str().and_then(|s| s.parse::<u32>().ok()))
}

async fn run_scenter_update(
) -> anyhow::Result<(bool, Vec<serde_json::Value>)> {
    run_scenter_update_with_timeout(CMD_TIMEOUT).await
}

async fn run_scenter_update_with_timeout(
    t: Duration,
) -> anyhow::Result<(bool, Vec<serde_json::Value>)> {
    let updates = tokio::time::timeout(
        t,
        tokio::task::spawn_blocking(|| {
            scenter_updates::check_packages_script()
        }),
    )
    .await??;

    let has_updates = !updates.is_empty();
    Ok((has_updates, updates))
}

/// Check all installed AppImages in parallel, each with an individual timeout.
async fn check_appimages() -> Vec<serde_json::Value> {
    let installed = scenter_appimages::get_installed();
    if installed.is_empty() {
        return vec![];
    }

    let tasks: Vec<_> = installed
        .into_iter()
        .map(|app| {
            tokio::spawn(async move {
                match timeout(CMD_TIMEOUT, scenter_appimages::check_update(&app)).await {
                    Ok(Some(result)) => Some(serde_json::json!({
                        "id":              result.id,
                        "name":            result.name,
                        "current_version": result.current_version,
                        "new_version":     result.new_version,
                        "download_url":    result.download_url,
                    })),
                    _ => None,
                }
            })
        })
        .collect();

    let mut updates = Vec::new();
    for task in tasks {
        if let Ok(Some(val)) = task.await {
            updates.push(val);
        }
    }
    updates
}

async fn run_command(args: &[&str]) -> anyhow::Result<()> {
    timeout(
        CMD_TIMEOUT,
        tokio::process::Command::new(args[0])
            .args(&args[1..])
            .output(),
    )
    .await??;
    Ok(())
}
