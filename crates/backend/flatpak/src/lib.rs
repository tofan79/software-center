// scenter-flatpak — Flatpak package management
// Mirrors src/backend/flatpak.py

use anyhow::Result;
use scenter_appstream::{get_appstream, AppInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::{Command, Stdio};

const FLATHUB_URL: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";
const COSMIC_REPO_URL: &str = "https://apt.pop-os.org/cosmic/cosmic.flatpakrepo";
/// Marker used to detect whether the system ships the COSMIC welcome app,
/// which is what gates whether the COSMIC remote buttons should be shown.
/// Only present in the original RakuOS builds; absent here, so the COSMIC
/// remote buttons stay hidden on stock Fedora.
const COSMIC_WELCOME_BIN: &str = "/usr/libexec/software-center/rakuos-welcome-cosmic";

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakApp {
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub origin: String,
    pub installed: bool,
    pub icon: String,
    pub icon_url: String,
    pub categories: Vec<String>,
    pub screenshots: Vec<String>,
    pub description: String,
    pub developer: String,
    pub url_homepage: String,
    pub url_bugtracker: String,
    pub url_donation: String,
    pub url_help: String,
    pub url_faq: String,
    pub url_vcs_browser: String,
    pub url_contribute: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FlatpakRemote {
    pub name: String,
    pub title: String,
    pub url: String,
    pub enabled: bool,
    pub system: bool,
}

impl From<&AppInfo> for FlatpakApp {
    fn from(a: &AppInfo) -> Self {
        FlatpakApp {
            id: a.id.clone(),
            name: a.name.clone(),
            version: a.version.clone(),
            summary: a.summary.clone(),
            origin: "flathub".to_string(),
            installed: false,
            icon: a.icon.clone(),
            icon_url: a.icon_url.clone(),
            categories: a.categories.clone(),
            screenshots: a.screenshots.clone(),
            description: a.description.clone(),
            developer: a.developer.clone(),
            url_homepage: a.url_homepage.clone(),
            url_bugtracker: a.url_bugtracker.clone(),
            url_donation: a.url_donation.clone(),
            url_help: a.url_help.clone(),
            url_faq: a.url_faq.clone(),
            url_vcs_browser: a.url_vcs_browser.clone(),
            url_contribute: a.url_contribute.clone(),
            source: "flatpak".to_string(),
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Return list of installed Flatpaks enriched with AppStream metadata.
pub fn get_installed() -> Result<Vec<FlatpakApp>> {
    let appstream = get_appstream();
    let appstream_flatpak: HashMap<&str, &AppInfo> = appstream
        .values()
        .filter(|a| a.source == "flatpak")
        .map(|a| (a.id.as_str(), a))
        .collect();

    let out = Command::new("flatpak")
        .args([
            "list",
            "--app",
            "--columns=application,name,version,description,origin",
        ])
        .output()?;

    let mut apps = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let app_id = parts[0].trim();
        let name = parts.get(1).map(|s| s.trim()).unwrap_or(app_id);
        let version = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
        let summary = parts.get(3).map(|s| s.trim()).unwrap_or("").to_string();
        let origin = parts
            .get(4)
            .map(|s| s.trim())
            .unwrap_or("flathub")
            .to_string();

        let meta = appstream_flatpak.get(app_id);
        apps.push(FlatpakApp {
            id: app_id.to_string(),
            name: meta
                .map(|m| m.name.clone())
                .unwrap_or_else(|| name.to_string()),
            version,
            summary: meta.map(|m| m.summary.clone()).unwrap_or(summary),
            origin,
            installed: true,
            icon: meta.map(|m| m.icon.clone()).unwrap_or_default(),
            icon_url: meta.map(|m| m.icon_url.clone()).unwrap_or_default(),
            categories: meta.map(|m| m.categories.clone()).unwrap_or_default(),
            screenshots: meta.map(|m| m.screenshots.clone()).unwrap_or_default(),
            description: meta.map(|m| m.description.clone()).unwrap_or_default(),
            developer: meta.map(|m| m.developer.clone()).unwrap_or_default(),
            url_homepage: meta.map(|m| m.url_homepage.clone()).unwrap_or_default(),
            url_bugtracker: meta.map(|m| m.url_bugtracker.clone()).unwrap_or_default(),
            url_donation: meta.map(|m| m.url_donation.clone()).unwrap_or_default(),
            url_help: meta.map(|m| m.url_help.clone()).unwrap_or_default(),
            url_faq: meta.map(|m| m.url_faq.clone()).unwrap_or_default(),
            url_vcs_browser: meta.map(|m| m.url_vcs_browser.clone()).unwrap_or_default(),
            url_contribute: meta.map(|m| m.url_contribute.clone()).unwrap_or_default(),
            source: "flatpak".to_string(),
        });
    }
    Ok(apps)
}

/// Search Flatpak remotes for apps matching query. Returns up to `limit` results.
pub fn search(query: &str, limit: usize) -> Vec<FlatpakApp> {
    let out = Command::new("flatpak")
        .args([
            "search",
            "--columns=application,name,version,description,origin",
            query,
        ])
        .output();

    let Ok(out) = out else { return Vec::new() };

    let installed_ids = get_installed_ids();

    let mut apps = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 || parts[0].trim().is_empty() {
            continue;
        }
        let app_id = parts[0].trim().to_string();
        apps.push(FlatpakApp {
            installed: installed_ids.contains(&app_id),
            name: parts
                .get(1)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| app_id.clone()),
            version: parts
                .get(2)
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            summary: parts
                .get(3)
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            origin: parts
                .get(4)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "flathub".to_string()),
            id: app_id,
            icon: String::new(),
            icon_url: String::new(),
            categories: Vec::new(),
            screenshots: Vec::new(),
            description: String::new(),
            developer: String::new(),
            url_homepage: String::new(),
            url_bugtracker: String::new(),
            url_donation: String::new(),
            url_help: String::new(),
            url_faq: String::new(),
            url_vcs_browser: String::new(),
            url_contribute: String::new(),
            source: "flatpak".to_string(),
        });
        if apps.len() >= limit {
            break;
        }
    }
    apps
}

/// A single Flatpak update entry — includes both apps and runtimes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakUpdate {
    pub name: String,
    pub app_id: String,
    pub version: String,
    pub current_version: String,
    pub runtime: bool,
    /// True when this entry is a new runtime branch to install, not an update to an existing one.
    /// e.g. org.gnome.Platform//50 required by an app update, when only //49 is installed.
    #[serde(default)]
    pub needs_install: bool,
    /// The remote to install from when needs_install is true.
    #[serde(default)]
    pub install_remote: String,
    /// "system" or "user" — which flatpak installation this update belongs to.
    #[serde(default = "default_system")]
    pub installation: String,
    /// Resolved local icon path (e.g. from flatpak appstream cache or per-app deploy dir).
    #[serde(default)]
    pub icon_path: String,
    /// Remote icon URL fallback when no local icon is found.
    #[serde(default)]
    pub icon_url: String,
}

fn default_system() -> String {
    "system".to_string()
}

/// Return all available Flatpak updates (apps + runtimes) by querying `flatpak`
/// directly for both the system and user installations — no separate
/// `updates`/sudo round-trip, since `flatpak remote-ls --updates` is a
/// read-only query that doesn't need elevated privileges either way.
/// Each entry is enriched with icon_path/icon_url from the AppStream cache.
pub fn get_all_updates() -> Vec<FlatpakUpdate> {
    let appstream = scenter_appstream::get_appstream();

    // Build a flatpak-specific lookup keyed by app id.
    // Index each entry under BOTH the full AppStream id (may include .desktop suffix)
    // AND the bare id (no .desktop suffix) so the lookup works regardless of whether
    // the flatpak command output uses one form or the other.
    let mut flatpak_meta: HashMap<String, &scenter_appstream::AppInfo> = HashMap::new();
    for a in appstream.values().filter(|a| a.source == "flatpak") {
        flatpak_meta.entry(a.id.clone()).or_insert(a);
        let bare = a.id.trim_end_matches(".desktop");
        if bare != a.id {
            flatpak_meta.entry(bare.to_string()).or_insert(a);
        }
    }

    let mut all = Vec::new();
    for (scope, flag) in [("system", "--system"), ("user", "--user")] {
        let installed: HashMap<String, String> = Command::new("flatpak")
            .args(["list", flag, "--columns=application,version"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let mut cols = line.splitn(2, '\t');
                let app_id = cols.next()?.trim().to_string();
                let version = cols.next().unwrap_or("").trim().to_string();
                Some((app_id, version))
            })
            .collect();

        let out = Command::new("flatpak")
            .args([
                "remote-ls",
                flag,
                "--updates",
                "--columns=application,branch,version,options",
            ])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        for line in out.lines() {
            let cols: Vec<&str> = line.splitn(4, '\t').collect();
            if cols.len() < 3 {
                continue;
            }
            let app_id = cols[0].trim().to_string();
            if app_id.is_empty() {
                continue;
            }
            let version = cols[2].trim().to_string();
            let options = cols.get(3).unwrap_or(&"").to_lowercase();
            let runtime = options.contains("runtime");
            let name = app_id.split('.').next_back().unwrap_or(&app_id).to_string();
            let current_version = installed.get(&app_id).cloned().unwrap_or_default();

            // Look up via flatpak-filtered map so the flatpak icon is always used,
            // even for apps that also have a native RPM in the AppStream cache.
            let (icon_path, icon_url) = flatpak_meta
                .get(&app_id)
                .map(|a| (a.icon_path.clone(), a.icon_url.clone()))
                .unwrap_or_default();

            all.push(FlatpakUpdate {
                name,
                app_id,
                version,
                current_version,
                runtime,
                needs_install: false,
                install_remote: String::new(),
                installation: scope.to_string(),
                icon_path,
                icon_url,
            });
        }
    }
    all
}

/// Return list of Flatpaks with available updates (apps only, legacy).
pub fn get_updates() -> Vec<FlatpakApp> {
    let out = Command::new("flatpak")
        .args([
            "remote-ls",
            "--updates",
            "--app",
            "--columns=application,name,version",
        ])
        .output();

    let Ok(out) = out else { return Vec::new() };

    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 || parts[0].trim().is_empty() {
                return None;
            }
            Some(FlatpakApp {
                id: parts[0].trim().to_string(),
                name: parts
                    .get(1)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default(),
                version: parts
                    .get(2)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default(),
                source: "flatpak".to_string(),
                installed: true,
                origin: String::new(),
                summary: String::new(),
                description: String::new(),
                icon: String::new(),
                icon_url: String::new(),
                categories: Vec::new(),
                screenshots: Vec::new(),
                developer: String::new(),
                url_homepage: String::new(),
                url_bugtracker: String::new(),
                url_donation: String::new(),
                url_help: String::new(),
                url_faq: String::new(),
                url_vcs_browser: String::new(),
                url_contribute: String::new(),
            })
        })
        .collect()
}

/// Install a Flatpak. Streams output lines, last line is "__done__<code>".
pub fn install_stream(app_id: &str, remote: &str, system: bool) -> impl Iterator<Item = String> {
    let r = if remote.is_empty() {
        "flathub".to_string()
    } else {
        remote.to_string()
    };
    let scope = if system { "--system" } else { "--user" };
    let args: Vec<String> = vec![
        "install".into(),
        scope.into(),
        "--noninteractive".into(),
        "-y".into(),
        r,
        app_id.to_string(),
    ];
    run_flatpak_stream_owned(args)
}

/// Uninstall a Flatpak. Streams output lines.
pub fn uninstall_stream(app_id: &str, system: bool) -> impl Iterator<Item = String> {
    let scope = if system { "--system" } else { "--user" };
    let args: Vec<String> = vec![
        "uninstall".into(),
        scope.into(),
        "--noninteractive".into(),
        "-y".into(),
        app_id.to_string(),
    ];
    run_flatpak_stream_owned(args)
}

/// Uninstall a Flatpak with --force-remove (for runtimes/add-ons with dependents).
pub fn force_uninstall_stream(app_id: &str, system: bool) -> impl Iterator<Item = String> {
    let scope = if system { "--system" } else { "--user" };
    let args: Vec<String> = vec![
        "uninstall".into(),
        scope.into(),
        "--noninteractive".into(),
        "-y".into(),
        "--force-remove".into(),
        app_id.to_string(),
    ];
    run_flatpak_stream_owned(args)
}

/// Check whether a named remote is user-scoped (vs system).
pub fn is_user_remote(name: &str) -> bool {
    get_remotes()
        .into_iter()
        .find(|r| r.name == name)
        .map(|r| !r.system)
        .unwrap_or(false)
}

/// Update all Flatpaks. Streams output lines.
pub fn update_stream() -> impl Iterator<Item = String> {
    run_flatpak_stream(&["update", "--noninteractive", "-y"])
}

/// Update a single Flatpak app. Streams output lines.
pub fn update_single_stream(app_id: &str, installation: &str) -> impl Iterator<Item = String> {
    let scope = if installation == "user" {
        "--user"
    } else {
        "--system"
    };
    let owned: Vec<String> = ["update", scope, "--app", "--noninteractive", "-y", app_id]
        .iter()
        .map(|s| s.to_string())
        .collect();
    run_flatpak_stream_owned(owned)
}

/// Remove unused Flatpak runtimes/extensions (global cache cleanup).
/// User-scoped unused refs are pruned first, then system-wide via pkexec.
pub fn clean_unused_stream() -> impl Iterator<Item = String> {
    let user = run_flatpak_stream(&["uninstall", "--unused", "-y", "--noninteractive", "--user"]);
    let system = run_pkexec_flatpak_stream(&[
        "uninstall",
        "--unused",
        "-y",
        "--noninteractive",
        "--system",
    ]);
    user.chain(system)
}

/// Update an already-installed Flatpak runtime branch (patch update).
pub fn update_runtime_stream(
    app_id: &str,
    branch: &str,
    installation: &str,
) -> impl Iterator<Item = String> {
    let scope = if installation == "user" {
        "--user"
    } else {
        "--system"
    };
    let ref_spec = format!("{}//{}", app_id, branch);
    let owned: Vec<String> = ["update", scope, "--noninteractive", "-y", &ref_spec]
        .iter()
        .map(|s| s.to_string())
        .collect();
    run_flatpak_stream_owned(owned)
}

/// Install from a local .flatpak bundle (system-wide via pkexec).
pub fn install_local_bundle_stream(path: &str) -> impl Iterator<Item = String> + '_ {
    run_pkexec_flatpak_stream(&["install", "--bundle", "--noninteractive", "-y", path])
}

/// Install from a .flatpakref file (system-wide via pkexec).
pub fn install_flatpakref_stream(path: &str) -> impl Iterator<Item = String> + '_ {
    run_pkexec_flatpak_stream(&["install", "--from", "--noninteractive", "-y", path])
}

/// Check if a Flatpak app_id is installed.
pub fn is_installed(app_id: &str) -> bool {
    Command::new("flatpak")
        .args(["info", app_id])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Remote / repo management ──────────────────────────────────────────────────

/// Return all configured Flatpak remotes (system + user).
pub fn get_remotes() -> Vec<FlatpakRemote> {
    let mut remotes: Vec<FlatpakRemote> = Vec::new();
    // Keyed by (scope, name) — the same remote name (e.g. "flathub") can
    // legitimately be configured in both system and user scope at once.
    let mut seen: std::collections::HashSet<(&str, String)> = std::collections::HashSet::new();

    for (scope, flag) in &[("system", "--system"), ("user", "--user")] {
        // Try with --columns first (Flatpak >= 1.2)
        let out = Command::new("flatpak")
            .args(["remotes", flag, "--columns=name,title,url,options"])
            .output();

        let text = match out {
            Ok(ref o) if o.status.success() && !o.stdout.is_empty() => {
                String::from_utf8_lossy(&o.stdout).to_string()
            }
            _ => {
                // Fall back to plain output
                Command::new("flatpak")
                    .args(["remotes", flag])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_default()
            }
        };

        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with("Name") {
                continue;
            }
            let parts: Vec<&str> = line.split('\t').collect();
            let name = parts[0].trim().to_string();
            if name.is_empty() || seen.contains(&(*scope, name.clone())) {
                continue;
            }
            seen.insert((*scope, name.clone()));
            let title = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
            let url = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
            let options = parts
                .get(3)
                .map(|s| s.trim().to_lowercase())
                .unwrap_or_default();
            let enabled = !options.contains("disabled");
            remotes.push(FlatpakRemote {
                name,
                title: if title.is_empty() {
                    parts[0].trim().to_string()
                } else {
                    title
                },
                url,
                enabled,
                system: *scope == "system",
            });
        }
    }

    // Last-resort: no-scope query
    if remotes.is_empty() {
        if let Ok(out) = Command::new("flatpak")
            .args(["remotes", "--columns=name,title,url,options"])
            .output()
        {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if line.trim().is_empty() || line.starts_with("Name") {
                    continue;
                }
                let parts: Vec<&str> = line.split('\t').collect();
                let name = parts[0].trim().to_string();
                if name.is_empty() || seen.contains(&("system", name.clone())) {
                    continue;
                }
                seen.insert(("system", name.clone()));
                let title = parts.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                let url = parts.get(2).map(|s| s.trim()).unwrap_or("").to_string();
                let options = parts
                    .get(3)
                    .map(|s| s.trim().to_lowercase())
                    .unwrap_or_default();
                remotes.push(FlatpakRemote {
                    name,
                    title,
                    url,
                    enabled: !options.contains("disabled"),
                    system: true,
                });
            }
        }
    }

    remotes
}

/// Check if Flathub is configured (system or user).
pub fn has_flathub() -> bool {
    get_remotes().iter().any(|r| {
        r.name.to_lowercase().contains("flathub") || r.url.to_lowercase().contains("flathub")
    })
}

/// Check if Flathub is configured for the given scope (system or user).
pub fn has_flathub_scoped(system: bool) -> bool {
    get_remotes().iter().any(|r| {
        r.system == system
            && (r.name.to_lowercase().contains("flathub")
                || r.url.to_lowercase().contains("flathub"))
    })
}

/// Whether the COSMIC welcome app is present on this system. Gates whether
/// the "Add COSMIC remote" buttons should be shown at all.
pub fn has_cosmic_welcome() -> bool {
    std::path::Path::new(COSMIC_WELCOME_BIN).exists()
}

/// Check if the COSMIC remote is configured for the given scope (system or user).
pub fn has_cosmic_remote_scoped(system: bool) -> bool {
    get_remotes().iter().any(|r| {
        r.system == system
            && (r.name.to_lowercase().contains("cosmic")
                || r.url.to_lowercase().contains("apt.pop-os.org/cosmic"))
    })
}

/// Add the COSMIC remote.
pub fn add_cosmic_remote(system: bool) -> (bool, String) {
    add_remote("cosmic", COSMIC_REPO_URL, system)
}

/// Add a Flatpak remote. Returns (success, message).
pub fn add_remote(name: &str, url: &str, system: bool) -> (bool, String) {
    let scope = if system { "--system" } else { "--user" };
    let result = if system {
        Command::new("pkexec")
            .args(["flatpak", "remote-add", scope, "--if-not-exists", name, url])
            .output()
    } else {
        Command::new("flatpak")
            .args(["remote-add", scope, "--if-not-exists", name, url])
            .output()
    };
    match result {
        Ok(o) if o.status.success() => {
            // Fetch the new remote's AppStream data immediately so it shows up
            // in search/browse right away, instead of waiting for the next
            // scheduled `flatpak update --appstream`.
            update_appstream(name, system);
            scenter_appstream::reload_appstream();
            (true, format!("Remote '{}' added.", name))
        }
        Ok(o) => (false, String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, e.to_string()),
    }
}

/// Fetch/refresh AppStream metadata for a single remote. Best-effort — a
/// failure here shouldn't fail the calling operation (e.g. remote-add).
fn update_appstream(name: &str, system: bool) {
    let scope = if system { "--system" } else { "--user" };
    let result = if system {
        Command::new("pkexec")
            .args(["flatpak", "update", "--appstream", scope, "-y", name])
            .output()
    } else {
        Command::new("flatpak")
            .args(["update", "--appstream", scope, "-y", name])
            .output()
    };
    if let Err(e) = result {
        log::warn!(
            "Failed to fetch AppStream data for remote '{}': {}",
            name,
            e
        );
    }
}

/// Add Flathub remote.
pub fn add_flathub(system: bool) -> (bool, String) {
    add_remote("flathub", FLATHUB_URL, system)
}

/// Remove a Flatpak remote. Returns (success, message).
pub fn remove_remote(name: &str, system: bool) -> (bool, String) {
    let scope = if system { "--system" } else { "--user" };
    let result = if system {
        Command::new("pkexec")
            .args(["flatpak", "remote-delete", scope, "--force", name])
            .output()
    } else {
        Command::new("flatpak")
            .args(["remote-delete", scope, "--force", name])
            .output()
    };
    match result {
        Ok(o) if o.status.success() => {
            scenter_appstream::reload_appstream();
            (true, format!("Remote '{}' removed.", name))
        }
        Ok(o) => (false, String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, e.to_string()),
    }
}

/// Enable or disable a Flatpak remote. Returns (success, message).
pub fn set_remote_enabled(name: &str, enabled: bool, system: bool) -> (bool, String) {
    let scope = if system { "--system" } else { "--user" };
    let flag = if enabled { "--enable" } else { "--disable" };
    let result = if system {
        Command::new("pkexec")
            .args(["flatpak", "remote-modify", scope, flag, name])
            .output()
    } else {
        Command::new("flatpak")
            .args(["remote-modify", scope, flag, name])
            .output()
    };
    match result {
        Ok(o) if o.status.success() => {
            scenter_appstream::reload_appstream();
            (
                true,
                format!(
                    "Remote '{}' {}.",
                    name,
                    if enabled { "enabled" } else { "disabled" }
                ),
            )
        }
        Ok(o) => (false, String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, e.to_string()),
    }
}

// ── Local file info ───────────────────────────────────────────────────────────

/// Extract metadata from a local .flatpak bundle file using `flatpak info --bundle`.
pub fn get_local_flatpak_info(path: &str) -> serde_json::Value {
    let out = Command::new("flatpak")
        .args(["info", "--bundle", path])
        .output();

    if let Ok(o) = out {
        let mut data: HashMap<String, String> = HashMap::new();
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            if let Some((k, v)) = line.split_once(':') {
                data.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
        let app_id = data
            .get("id")
            .cloned()
            .unwrap_or_else(|| basename_no_ext(path));
        let name = data
            .get("name")
            .cloned()
            .unwrap_or_else(|| app_id.split('.').next_back().unwrap_or(&app_id).to_string());
        let version = data.get("version").cloned().unwrap_or_default();
        let summary = data.get("subject").cloned().unwrap_or_default();
        let branch = data
            .get("branch")
            .cloned()
            .unwrap_or_else(|| "stable".to_string());
        let (icon_path, icon_url) = local_flatpak_icon(&app_id);

        return serde_json::json!({
            "id": app_id, "name": name, "summary": summary,
            "description": "", "categories": [], "icon": "", "screenshots": [],
            "pkg_name": app_id, "url": "", "source": "flatpak",
            "installed": false, "is_addon": false,
            "local_flatpak": path, "version": version, "branch": branch,
            "icon_path": icon_path, "icon_url": icon_url,
        });
    }

    // Minimal fallback
    let app_id = basename_no_ext(path);
    let (icon_path, icon_url) = local_flatpak_icon(&app_id);
    serde_json::json!({
        "id": app_id, "name": app_id, "summary": "Local Flatpak bundle",
        "description": "", "categories": [], "icon": "", "screenshots": [],
        "pkg_name": app_id, "url": "", "source": "flatpak",
        "installed": false, "is_addon": false, "local_flatpak": path,
        "icon_path": icon_path,
        "icon_url": icon_url,
    })
}

/// Parse a .flatpakref INI file and return app info.
pub fn get_flatpakref_info(path: &str) -> serde_json::Value {
    let Ok(text) = std::fs::read_to_string(path) else {
        let app_id = basename_no_ext(path);
        return serde_json::json!({
            "id": app_id, "name": app_id, "summary": "",
            "source": "flatpak", "installed": false, "local_flatpakref": path,
        });
    };

    let mut app_id = basename_no_ext(path);
    let mut title = String::new();
    let mut comment = String::new();
    let mut url = String::new();
    let mut branch = "stable".to_string();
    let mut in_section = false;

    for line in text.lines() {
        let line = line.trim();
        if line.eq_ignore_ascii_case("[Flatpak Ref]") {
            in_section = true;
            continue;
        }
        if line.starts_with('[') {
            in_section = false;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" => app_id = v.trim().to_string(),
                "Title" => title = v.trim().to_string(),
                "Comment" => comment = v.trim().to_string(),
                "Url" => url = v.trim().to_string(),
                "Branch" => branch = v.trim().to_string(),
                _ => {}
            }
        }
    }

    let name = if title.is_empty() {
        app_id.split('.').next_back().unwrap_or(&app_id).to_string()
    } else {
        title
    };
    let (icon_path, icon_url) = local_flatpak_icon(&app_id);

    serde_json::json!({
        "id": app_id, "name": name, "summary": comment,
        "description": "", "categories": [], "icon": "", "screenshots": [],
        "pkg_name": app_id, "url": url, "source": "flatpak",
        "installed": false, "is_addon": false,
        "local_flatpakref": path, "branch": branch,
        "icon_path": icon_path, "icon_url": icon_url,
    })
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn local_flatpak_icon(app_id: &str) -> (String, String) {
    let appstream = get_appstream();
    appstream
        .get(&format!("flatpak:{app_id}"))
        .or_else(|| appstream.get(app_id).filter(|app| app.source == "flatpak"))
        .or_else(|| {
            appstream
                .values()
                .find(|app| app.source == "flatpak" && app.id == app_id)
        })
        .map(|app| (app.icon_path.clone(), app.icon_url.clone()))
        .unwrap_or_default()
}

fn get_installed_ids() -> std::collections::HashSet<String> {
    Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| l.trim().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Spawn `bin` with `args` in a PTY so flatpak sees a real terminal (no buffering,
/// live progress). Falls back to concurrent-piped streaming if PTY allocation fails.
/// Stderr is always drained concurrently to prevent pipe-buffer deadlocks.
fn spawn_stream(bin: &str, args: Vec<String>) -> impl Iterator<Item = String> {
    use std::os::unix::io::FromRawFd;
    use std::sync::mpsc;

    let bin = bin.to_string();
    let (tx, rx) = mpsc::channel::<String>();

    std::thread::spawn(move || {
        // ── Try PTY ──────────────────────────────────────────────────────────
        let mut master_raw: libc::c_int = -1;
        let mut slave_raw: libc::c_int = -1;
        let pty_ok = unsafe {
            libc::openpty(
                &mut master_raw,
                &mut slave_raw,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == 0
        };

        if pty_ok {
            let slave_out = unsafe { libc::dup(slave_raw) };
            let slave_err = unsafe { libc::dup(slave_raw) };

            if slave_out >= 0 && slave_err >= 0 {
                use std::os::unix::process::CommandExt;
                let mut builder = Command::new(&bin);
                builder.args(&args);
                unsafe {
                    builder
                        .stdin(Stdio::from_raw_fd(slave_raw))
                        .stdout(Stdio::from_raw_fd(slave_out))
                        .stderr(Stdio::from_raw_fd(slave_err))
                        .pre_exec(|| {
                            libc::setsid();
                            Ok(())
                        });
                }
                let spawn_result = builder.spawn();
                // Drop builder immediately: this closes the Stdio-owned slave fds
                // in the parent right now (before any other fd reuse can happen),
                // so EIO fires on the master when the child exits.
                drop(builder);
                match spawn_result {
                    Ok(mut child) => {
                        let master = unsafe { std::fs::File::from_raw_fd(master_raw) };
                        let tx2 = tx.clone();
                        let reader = std::thread::spawn(move || drain_pty(master, tx2));
                        let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
                        reader.join().ok();
                        let _ = tx.send(format!("__done__{}", code));
                        return;
                    }
                    Err(e) => {
                        unsafe {
                            libc::close(master_raw);
                        }
                        let _ = tx.send(format!("Error: {e}"));
                        let _ = tx.send("__done__1".to_string());
                        return;
                    }
                }
            } else {
                unsafe {
                    if slave_out >= 0 {
                        libc::close(slave_out);
                    }
                    if slave_err >= 0 {
                        libc::close(slave_err);
                    }
                    libc::close(slave_raw);
                    libc::close(master_raw);
                }
            }
        }

        // ── Pipe fallback — stderr drained concurrently to prevent deadlock ──
        let mut child = match Command::new(&bin)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(format!("Error: {e}"));
                let _ = tx.send("__done__1".to_string());
                return;
            }
        };
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let tx_out = tx.clone();
        let t1 = std::thread::spawn(move || drain_pipe(stdout, tx_out));
        let tx_err = tx.clone();
        let t2 = std::thread::spawn(move || drain_pipe(stderr, tx_err));
        t1.join().ok();
        t2.join().ok();
        let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
        let _ = tx.send(format!("__done__{}", code));
    });

    rx.into_iter()
}

fn drain_pty(reader: std::fs::File, tx: std::sync::mpsc::Sender<String>) {
    use std::io::Read;
    let mut reader = reader;
    let mut buf = [0u8; 4096];
    let mut pending = String::new();
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let clean = strip_ansi(&buf[..n]);
                pending.push_str(&String::from_utf8_lossy(&clean));
                flush_lines(&mut pending, &tx);
            }
            Err(e) if e.raw_os_error() == Some(libc::EIO) => break,
            Err(_) => break,
        }
    }
    flush_tail(&pending, &tx);
}

fn drain_pipe<R: std::io::Read>(reader: R, tx: std::sync::mpsc::Sender<String>) {
    let mut reader = reader;
    let mut buf = [0u8; 4096];
    let mut pending = String::new();
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                flush_lines(&mut pending, &tx);
            }
            Err(_) => break,
        }
    }
    flush_tail(&pending, &tx);
}

fn flush_lines(pending: &mut String, tx: &std::sync::mpsc::Sender<String>) {
    let mut start = 0;
    for (i, b) in pending.bytes().enumerate() {
        if b == b'\n' || b == b'\r' {
            let seg = pending[start..i].trim();
            if !seg.is_empty() {
                let _ = tx.send(seg.to_string());
            }
            start = i + 1;
        }
    }
    *pending = pending[start..].to_string();
}

fn flush_tail(pending: &str, tx: &std::sync::mpsc::Sender<String>) {
    let seg = pending.trim();
    if !seg.is_empty() {
        let _ = tx.send(seg.to_string());
    }
}

fn strip_ansi(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'\x1b' {
            i += 1;
            if i >= input.len() {
                break;
            }
            match input[i] {
                b'[' => {
                    i += 1;
                    while i < input.len() && !input[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    if i < input.len() {
                        i += 1;
                    }
                }
                b']' => {
                    i += 1;
                    while i < input.len() {
                        if input[i] == b'\x07' {
                            i += 1;
                            break;
                        } else if input[i] == b'\x1b'
                            && i + 1 < input.len()
                            && input[i + 1] == b'\\'
                        {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    i += 1;
                }
            }
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

fn run_flatpak_stream_owned(args: Vec<String>) -> impl Iterator<Item = String> {
    spawn_stream("flatpak", args)
}

fn run_flatpak_stream(args: &[&str]) -> impl Iterator<Item = String> {
    spawn_stream("flatpak", args.iter().map(|s| s.to_string()).collect())
}

fn run_pkexec_flatpak_stream(flatpak_args: &[&str]) -> impl Iterator<Item = String> {
    let mut args = vec!["flatpak".to_string()];
    args.extend(flatpak_args.iter().map(|s| s.to_string()));
    spawn_stream("pkexec", args)
}

fn basename_no_ext(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}
