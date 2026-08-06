// software-center-tray — Background daemon + system tray for Software Center

mod checker;
mod settings;
mod tray;

use checker::run_checks;
use settings::Settings;
use std::sync::mpsc as stdmpsc;
use std::time::Duration;
use tokio::sync::mpsc;
use tray::TrayStatus;

#[derive(Debug)]
pub enum DaemonMsg {
    CheckNow,
    Quit,
    OpenUi,
}

fn pid_file() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".cache/software-center/software-ui.pid")
}

fn show_flag_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-show")
}

fn quit_flag_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-quit")
}

fn check_trigger_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-check-requested")
}

fn badge_count_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-badge-count")
}

/// Returns true if the UI process is currently running (pid file + /proc check).
fn ui_is_running() -> bool {
    std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|pid| std::path::Path::new(&format!("/proc/{}", pid)).exists())
        .unwrap_or(false)
}

fn daemon_cache_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".cache/software-center/daemon-update-cache.json")
}

/// Locate the installed UI frontend binary.
/// Checks sibling directory of this binary first (covers dev builds), then libexec.
fn ui_binary() -> std::path::PathBuf {
    let libexec = std::path::Path::new("/usr/libexec/software-center");
    let candidates: &[&str] = &["software-center"];

    // Check sibling directory (dev builds in target/debug/)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in candidates.iter().copied() {
                let p = dir.join(name);
                if p.exists() {
                    return p;
                }
            }
        }
    }

    // Check installed libexec location
    for name in candidates.iter().copied() {
        let p = libexec.join(name);
        if p.exists() {
            return p;
        }
    }

    // Last resort: hope it's on PATH
    std::path::PathBuf::from("software-center")
}

/// If the UI is already running, write the show-flag so its polling timer
/// picks it up. Otherwise spawn a fresh instance.
fn signal_or_spawn_ui() {
    if ui_is_running() {
        let _ = std::fs::write(show_flag_path(), "1");
    } else {
        let _ = std::process::Command::new(ui_binary()).spawn();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // std::sync::mpsc so ksni's non-tokio thread can send without needing a runtime.
    let (std_tx, std_rx) = stdmpsc::sync_channel::<DaemonMsg>(8);

    // Spawn tray icon (registers D-Bus StatusNotifierItem)
    let tray_handle = tray::spawn(std_tx.clone())?;

    // On startup: apply any existing cache immediately so the tray icon
    // reflects the last known state instead of showing "Checking" for 20s.
    if let Ok(j) = std::fs::read_to_string(daemon_cache_path()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&j) {
            let count  = v["total"].as_i64().unwrap_or(0) as usize;
            let reboot = v["reboot_required"].as_bool().unwrap_or(false);
            tray_handle.update(|t| t.status = if reboot {
                TrayStatus::RebootRequired
            } else if count > 0 {
                TrayStatus::Available(count)
            } else {
                TrayStatus::UpToDate
            });
        }
    }

    // Bridge: read from the std channel in a blocking thread and forward into
    // the tokio channel so the async loop below can await on it normally.
    let (tok_tx, mut tok_rx) = mpsc::channel::<DaemonMsg>(8);
    std::thread::spawn(move || {
        while let Ok(msg) = std_rx.recv() {
            if tok_tx.blocking_send(msg).is_err() {
                break;
            }
        }
    });

    // ── Scheduled check loop ──────────────────────────────────────────────────
    let sched_tx = std_tx.clone();
    tokio::spawn(async move {
        // Delay so the tray registers before the first check.
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let _ = sched_tx.send(DaemonMsg::CheckNow);
            let settings = Settings::load();
            match settings.effective_interval_secs() {
                Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
                None => tokio::time::sleep(Duration::from_secs(u64::MAX)).await,
            }
        }
    });

    // ── Badge sync loop ───────────────────────────────────────────────────────
    // The UI writes this file as rows complete so the tray badge can decrement
    // immediately, instead of waiting for the next scheduled background check.
    {
        let tray_c = tray_handle.clone();
        tokio::spawn(async move {
            let mut last_seen: Option<std::time::SystemTime> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let path = badge_count_path();
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                let mtime = meta.modified().ok();
                if mtime.is_some() && mtime == last_seen {
                    continue;
                }
                last_seen = mtime;
                if let Ok(raw) = std::fs::read_to_string(&path) {
                    if let Ok(count) = raw.trim().parse::<usize>() {
                        tray_c.update(|t| t.status = if count > 0 {
                            TrayStatus::Available(count)
                        } else {
                            TrayStatus::UpToDate
                        });
                    }
                }
            }
        });
    }

    // ── Message loop ──────────────────────────────────────────────────────────
    while let Some(msg) = tok_rx.recv().await {
        match msg {
            DaemonMsg::Quit => {
                log::info!("Quitting daemon.");
                // Signal the UI to quit via flag file (SIGTERM is blocked by the
                // close-request handler that hides instead of closing the window).
                let _ = std::fs::write(quit_flag_path(), "1");
                std::process::exit(0);
            }
            DaemonMsg::OpenUi => {
                signal_or_spawn_ui();
            }
            DaemonMsg::CheckNow => {
                // Always run the daemon's own check. Delegating to the UI via
                // check_trigger is unreliable — the UI may not process it, and
                // Qt's handler only covers the reboot case, not a full check.
                log::info!("Running update check.");
                let settings = Settings::load();
                let result = run_checks(&settings).await;
                let count = result.total;

                let cache = serde_json::json!({
                    "total":           count,
                    "packages":        result.packages,
                    "flatpak":         result.flatpak,
                    "appimages":       result.appimages,
                    "reboot_required": false,
                });
                let cache_path = daemon_cache_path();
                if let Some(parent) = cache_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&cache_path,
                    serde_json::to_string_pretty(&cache).unwrap_or_default());

                tray_handle.update(|t| t.status = if count > 0 {
                    TrayStatus::Available(count)
                } else {
                    TrayStatus::UpToDate
                });

                // Signal any running UI to refresh its display from the new cache.
                let _ = std::fs::write(check_trigger_path(), "1");

                if let Some(body) = result.notification_body() {
                    send_notification("Updates Available", &body).await;
                }

                log::info!("Check complete: {} update(s).", count);
            }
        }
    }

    Ok(())
}

async fn send_notification(summary: &str, body: &str) {
    let _ = notify_rust::Notification::new()
        .summary(summary)
        .body(body)
        .appname("Software Center")
        .icon("system-software-update")
        .timeout(notify_rust::Timeout::Milliseconds(8000))
        .show_async()
        .await;
}
