// scenter-packages — Native package management via dnf5
// Mirrors src/backend/packages.py

use anyhow::Result;
use scenter_appstream::{AppInfo, get_appstream, get_rpm_to_flatpak};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

// ── Data types ────────────────────────────────────────────────────────────────

/// One installable source for an app (native RPM or a Flatpak remote).
/// Carries the full display metadata for this source so the UI can update
/// description, screenshots, version etc. when the user switches sources.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SourceOption {
    pub id: String,           // package_name for native, app_id for flatpak
    pub source: String,       // "native", "flatpak", "terra"
    pub package_name: String,
    pub installed: bool,
    pub label: String,        // "RakuOS Linux" or "Flathub" / remote title
    pub remote: String,       // flatpak remote name
    #[serde(default)]
    pub user_remote: bool,    // true if this is a user-scoped flatpak remote
    // Full display data for this source — shown when the user picks this source
    #[serde(default)] pub summary: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub version: String,
    #[serde(default)] pub developer: String,
    #[serde(default)] pub url_homepage: String,
    #[serde(default)] pub url_bugtracker: String,
    #[serde(default)] pub url_donation: String,
    #[serde(default)] pub url_help: String,
    #[serde(default)] pub url_faq: String,
    #[serde(default)] pub url_vcs_browser: String,
    #[serde(default)] pub url_contribute: String,
    #[serde(default)] pub license: String,
    #[serde(default)] pub screenshots: Vec<String>,
    #[serde(default)] pub icon_path: String,
    #[serde(default)] pub icon_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeApp {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub version: String,
    pub package_name: String,
    pub icon: String,
    pub icon_url: String,
    pub icon_path: String,  // resolved local filesystem path
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub screenshots: Vec<String>,
    pub developer: String,
    pub url_homepage: String,
    pub url_bugtracker: String,
    pub url_donation: String,
    pub url_help: String,
    pub url_faq: String,
    pub url_vcs_browser: String,
    pub url_contribute: String,
    pub license: String,
    pub source: String, // "native", "terra", "flatpak", etc.
    pub installed: bool,
    pub is_addon: bool,
    pub pkg_name_guessed: bool,
    #[serde(default)]
    pub update_source: String,
    #[serde(default)]
    pub update_url: String,
    #[serde(default)]
    pub update_pattern: String,
    #[serde(default)]
    pub sources: Vec<SourceOption>, // populated in get_app_by_id for detail page
    #[serde(default)]
    pub remotes: Vec<String>, // all flatpak remote names this app was found in (e.g. ["flathub", "cosmic"]); empty for non-flatpak
}

impl From<&AppInfo> for NativeApp {
    fn from(a: &AppInfo) -> Self {
        NativeApp {
            id: a.id.clone(),
            name: a.name.clone(),
            summary: a.summary.clone(),
            description: a.description.clone(),
            version: a.version.clone(),
            package_name: a.package_name.clone(),
            icon: a.icon.clone(),
            icon_url: a.icon_url.clone(),
            icon_path: a.icon_path.clone(),
            categories: a.categories.clone(),
            keywords: a.keywords.clone(),
            screenshots: a.screenshots.clone(),
            developer: a.developer.clone(),
            url_homepage: a.url_homepage.clone(),
            url_bugtracker: a.url_bugtracker.clone(),
            url_donation: a.url_donation.clone(),
            url_help: a.url_help.clone(),
            url_faq: a.url_faq.clone(),
            url_vcs_browser: a.url_vcs_browser.clone(),
            url_contribute: a.url_contribute.clone(),
            license: a.license.clone(),
            source: a.source.clone(),
            installed: false,
            is_addon: a.is_addon,
            pkg_name_guessed: a.pkg_name_guessed,
            update_source: String::new(),
            update_url: String::new(),
            update_pattern: String::new(),
            sources: Vec::new(),
            remotes: a.remotes.clone(),
        }
    }
}

// ── Browsability filter ───────────────────────────────────────────────────────

/// Mirror of Python's _is_browseable(): excludes addons (unless standalone),
/// and apps without a package_name.
fn is_browseable(a: &NativeApp) -> bool {
    if a.package_name.is_empty() {
        return false;
    }
    // Guessed pkg_names may not exist in repos — exclude from browse/search
    if a.pkg_name_guessed {
        return false;
    }
    if a.is_addon {
        let standalone = [
            "game", "network", "audiovideo", "audio", "video", "graphics",
            "office", "development", "education", "science", "accessibility",
        ];
        let cats: Vec<String> = a.categories.iter().map(|c| c.to_lowercase()).collect();
        return cats.iter().any(|c| standalone.contains(&c.as_str()));
    }
    true
}

/// Strip `.desktop` suffix from a flatpak app ID.
/// AppStream component IDs sometimes end in `.desktop` but flatpak itself never does.
fn fp_bare(id: &str) -> &str {
    id.strip_suffix(".desktop").unwrap_or(id)
}

/// Check whether a flatpak ID (with or without .desktop suffix) is installed.
fn fp_installed(installed_fp: &HashSet<String>, id: &str) -> bool {
    installed_fp.contains(fp_bare(id))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Return all available apps from AppStream (native + flatpak), with installed status.
pub fn get_available() -> Result<Vec<NativeApp>> {
    let appstream = get_appstream();
    let installed_rpm = get_installed_packages()?;
    let installed_fp = get_installed_flatpaks();

    let apps: Vec<NativeApp> = appstream
        .values()
        .filter(|a| !a.package_name.is_empty())
        .map(|a| {
            let mut app = NativeApp::from(a);
            if a.source == "flatpak" {
                app.installed = fp_installed(&installed_fp, &a.id);
            } else {
                app.installed = installed_rpm.contains(&a.package_name);
            }
            app
        })
        .collect();

    Ok(apps)
}

/// Return user-installed overlay packages (from packages.list), enriched with AppStream metadata.
/// Mirrors Python's get_installed_with_metadata().
/// Only returns packages the user explicitly added — NOT the base OS image packages.
pub fn get_installed() -> Result<Vec<NativeApp>> {
    let overlay_pkgs = read_packages_list();

    let appstream = get_appstream();

    // Build pkg_name → AppInfo lookup
    let mut pkg_map: std::collections::HashMap<String, &scenter_appstream::AppInfo> =
        std::collections::HashMap::new();
    for a in appstream.values() {
        if a.is_addon || a.package_name.is_empty() {
            continue;
        }
        pkg_map.entry(a.package_name.clone()).or_insert(a);
    }

    // Build flatpak-by-name/id-segment for fallback matching
    let mut flatpak_by_name: std::collections::HashMap<String, &scenter_appstream::AppInfo> =
        std::collections::HashMap::new();
    for a in appstream.values() {
        if a.source == "flatpak" {
            flatpak_by_name.entry(a.name.to_lowercase()).or_insert(a);
            let last_seg = a.id.split('.').last().unwrap_or("").to_lowercase();
            if !last_seg.is_empty() {
                flatpak_by_name.entry(last_seg).or_insert(a);
            }
        }
    }

    let mut results = Vec::new();
    for pkg in &overlay_pkgs {
        // Verify it is actually installed
        if !rpm_is_installed(pkg) {
            continue;
        }
        if let Some(meta) = pkg_map.get(pkg) {
            let mut app = NativeApp::from(*meta);
            app.installed = true;
            app.source = "native".to_string();
            if app.icon_path.is_empty() {
                app.icon_path = find_local_rpm_icon(pkg);
            }
            results.push(app);
        } else if let Some(meta) = flatpak_by_name.get(&pkg.to_lowercase()) {
            // Borrow Flatpak metadata for icon/description
            let mut app = NativeApp::from(*meta);
            app.id = pkg.clone();
            app.package_name = pkg.clone();
            app.source = "native".to_string();
            app.installed = true;
            if app.icon_path.is_empty() {
                app.icon_path = find_local_rpm_icon(pkg);
            }
            results.push(app);
        } else {
            results.push(NativeApp {
                id: pkg.clone(),
                name: pkg.clone(),
                summary: "Installed package".to_string(),
                package_name: pkg.clone(),
                source: "native".to_string(),
                installed: true,
                ..Default::default()
            });
        }
    }

    // Append locally-cached RPM installs (packages-rpm.list)
    let already_shown: std::collections::HashSet<String> =
        results.iter().map(|a| a.package_name.clone()).collect();
    for pkg in &read_local_rpm_list() {
        if already_shown.contains(pkg) {
            continue;
        }
        if !rpm_is_installed(pkg) {
            continue;
        }
        let icon_path = find_local_rpm_icon(pkg);
        // Try to get a friendly display name from rpm metadata
        let display_name = Command::new("rpm")
            .args(["-q", "--queryformat", "%{NAME}", pkg])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| pkg.clone());
        results.push(NativeApp {
            id: pkg.clone(),
            name: display_name,
            summary: "Locally installed RPM".to_string(),
            package_name: pkg.clone(),
            source: "local-rpm".to_string(),
            installed: true,
            icon_path,
            ..Default::default()
        });
    }

    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let installed_rpm = get_installed_packages()?;
    let installed_fp = get_installed_flatpaks();
    enrich_sources(&mut results, &appstream, &installed_rpm, &installed_fp);
    Ok(results)
}

/// A lightweight view of an installed Flatpak runtime or add-on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakRuntime {
    pub app_id:  String,
    pub name:    String,
    pub version: String,
    pub origin:  String,
}

/// Return all installed Flatpak runtimes and add-ons (not apps).
pub fn get_installed_flatpak_runtimes() -> Vec<FlatpakRuntime> {
    let Ok(out) = Command::new("flatpak")
        .args(["list", "--runtime", "--columns=application,name,version,origin"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let app_id  = parts.next().unwrap_or("").trim();
            let name    = parts.next().unwrap_or("").trim();
            let version = parts.next().unwrap_or("").trim();
            let origin  = parts.next().unwrap_or("flathub").trim();
            if app_id.is_empty() { return None; }
            Some(FlatpakRuntime {
                app_id:  app_id.to_string(),
                name:    if name.is_empty() { app_id.to_string() } else { name.to_string() },
                version: version.to_string(),
                origin:  origin.to_string(),
            })
        })
        .collect()
}

/// Return installed Flatpak apps enriched with AppStream metadata.
pub fn get_installed_flatpaks_enriched() -> Result<Vec<NativeApp>> {
    let appstream = get_appstream();
    let installed_fp_set = get_installed_flatpaks();
    let installed_rpm = get_installed_packages()?;
    let installed_fp = get_installed_flatpaks_with_info();

    let mut results: Vec<NativeApp> = Vec::new();
    for (app_id, name, version, summary, _origin) in &installed_fp {
        // flatpak list returns IDs without .desktop suffix, but AppStream may
        // store them differently. Try multiple key variants, always preferring
        // a flatpak-sourced entry (native entries with the same ID are re-keyed
        // to "flatpak:<id>" by parse_catalog_file when both sources exist).
        let desktop_id = format!("{}.desktop", app_id);
        let meta = appstream.get(app_id.as_str()).filter(|m| m.source == "flatpak")
            .or_else(|| appstream.get(&format!("flatpak:{}", app_id)))
            .or_else(|| appstream.get(&format!("flatpak:{}", desktop_id)))
            .or_else(|| appstream.get(&desktop_id).filter(|m| m.source == "flatpak"));
        if let Some(meta) = meta {
            let mut app = NativeApp::from(meta);
            app.installed = true;
            // Use the real flatpak ID (without .desktop) so install/remove work
            app.id = app_id.clone();
            app.package_name = app_id.clone();
            if !version.is_empty() {
                app.version = version.clone();
            }
            results.push(app);
        } else {
            // No AppStream data — use flatpak list output directly
            results.push(NativeApp {
                id: app_id.clone(),
                name: name.clone(),
                summary: summary.clone(),
                version: version.clone(),
                package_name: app_id.clone(),
                source: "flatpak".to_string(),
                installed: true,
                icon: format!("{}.png", app_id),
                ..Default::default()
            });
        }
    }
    enrich_sources(&mut results, &appstream, &installed_rpm, &installed_fp_set);
    Ok(results)
}

/// Search all apps (native + flatpak) by name, summary, id, or keywords.
/// Results ranked: name-starts-with > name-contains > id/pkg-contains > summary > keyword.
/// Apps with both a native and a Flatpak source are merged into one result.
pub fn search(query: &str) -> Result<Vec<NativeApp>> {
    let appstream = get_appstream();
    let installed_rpm = get_installed_packages()?;
    let installed_fp = get_installed_flatpaks();
    let q = query.to_lowercase();

    let mut scored: Vec<(u8, NativeApp)> = Vec::new();

    for a in appstream.values() {
        let mut app = NativeApp::from(a);
        if a.source == "flatpak" {
            app.installed = fp_installed(&installed_fp, &a.id);
        } else {
            app.installed = installed_rpm.contains(&a.package_name);
        }
        if !is_browseable(&app) { continue; }

        let name_lc = app.name.to_lowercase();
        let id_lc = app.id.to_lowercase();
        let pkg_lc = app.package_name.to_lowercase();
        let summary_lc = app.summary.to_lowercase();

        let score = if name_lc.starts_with(&q) { 6 }
            else if name_lc.contains(&q) { 5 }
            else if id_lc.contains(&q) || pkg_lc.contains(&q) { 4 }
            else if summary_lc.contains(&q) { 3 }
            else if app.keywords.iter().any(|k| k.to_lowercase().contains(&q)) { 2 }
            else { continue };

        scored.push((score, app));
    }

    // Merge same-name apps across sources, keeping best score per name
    let mut by_name: HashMap<String, (u8, NativeApp)> = HashMap::new();
    for (score, app) in scored {
        let key = app.name.to_lowercase();
        if let Some(existing) = by_name.get_mut(&key) {
            if score > existing.0 {
                // New entry scores higher — it becomes primary, old becomes alt
                let old = std::mem::replace(&mut existing.1, app);
                existing.0 = score;
                existing.1.sources.push(source_option_from_native_app(&old));
            } else {
                existing.1.sources.push(source_option_from_native_app(&app));
            }
        } else {
            by_name.insert(key, (score, app));
        }
    }

    // For entries that gained sources, prepend the primary as sources[0]
    for (_, app) in by_name.values_mut() {
        if !app.sources.is_empty() {
            let primary_src = source_option_from_native_app(app);
            app.sources.insert(0, primary_src);
            // Prefer native/terra as primary; re-sort if needed
            app.sources.sort_by_key(|s| if s.source == "flatpak" { 1u8 } else { 0 });
            // Sync installed flag: true if any source is installed
            app.installed = app.sources.iter().any(|s| s.installed);
        }
    }

    let mut results: Vec<(u8, NativeApp)> = by_name.into_values().collect();
    results.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    let mut apps: Vec<NativeApp> = results.into_iter().map(|(_, a)| a).collect();
    enrich_sources(&mut apps, &appstream, &installed_rpm, &installed_fp);
    Ok(apps)
}

/// Return all apps in a given AppStream category (case-insensitive).
/// Apps with both a native and a Flatpak source are returned as a single
/// entry with `sources` populated so the detail page can switch between them.
pub fn get_by_category(category: &str) -> Result<Vec<NativeApp>> {
    let appstream = get_appstream();
    let installed_rpm = get_installed_packages()?;
    let installed_fp = get_installed_flatpaks();
    let cat = category.to_lowercase();

    let mut raw: Vec<NativeApp> = Vec::new();
    for a in appstream.values() {
        let mut app = NativeApp::from(a);
        if !is_browseable(&app) { continue; }
        if !app.categories.iter().any(|c| c.to_lowercase() == cat) { continue; }
        if a.source == "flatpak" {
            app.installed = fp_installed(&installed_fp, &a.id);
        } else {
            app.installed = installed_rpm.contains(&a.package_name);
        }
        raw.push(app);
    }

    let mut results: Vec<NativeApp> =
        merge_multi_source(raw).into_values().collect();
    enrich_sources(&mut results, &appstream, &installed_rpm, &installed_fp);
    results.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(results)
}

/// Look up a single app by its AppStream ID, including available install sources.
/// The returned NativeApp has its `sources` field populated for use by the detail page.
pub fn get_app_by_id(app_id: &str) -> Result<Option<NativeApp>> {
    let appstream = get_appstream();
    let installed_rpm = get_installed_packages()?;
    let installed_fp = get_installed_flatpaks();

    // Try multiple key patterns:
    // - Plain key (most apps)
    // - "native:{id}" (injected stubs from flatpak-to-rpm.json)
    // - "flatpak:{id}" (flatpak entries re-keyed by inject_native_stubs)
    let a = appstream.get(app_id)
        .or_else(|| appstream.get(&format!("native:{}", app_id)))
        .or_else(|| appstream.get(&format!("flatpak:{}", app_id)));
    let Some(a) = a else {
        return Ok(None);
    };

    let mut app = NativeApp::from(a);
    if a.source == "flatpak" {
        app.installed = fp_installed(&installed_fp, &a.id);
    } else {
        app.installed = installed_rpm.contains(&a.package_name);
    }

    // Build sources list for the detail page install-source dropdown
    app.sources = build_sources(&app, app_id, &appstream, &installed_rpm, &installed_fp);

    // Prefer flatpak screenshots for native apps — Flathub CDN is reliable,
    // Fedora swcatalog screenshots often have encoding/format issues.
    if app.source != "flatpak" {
        if let Some(fp_src) = app.sources.iter().find(|s| s.source == "flatpak") {
            if !fp_src.screenshots.is_empty() {
                app.screenshots = fp_src.screenshots.clone();
            }
        }
    }

    Ok(Some(app))
}

/// Return all add-ons for a given app filtered by source type.
///
/// - `source_type == "flatpak"`: returns all Flatpak add-ons extending `app_id`,
///   regardless of which remote they come from (a Flatpak add-on from any remote
///   can work with any Flatpak install of the same app).
/// - anything else (native/terra/rpm): returns only non-Flatpak add-ons extending
///   `app_id`; RPM add-ons cannot work inside a Flatpak sandbox and vice versa.
pub fn get_addons_for_app(app_id: &str, source_type: &str) -> Result<Vec<serde_json::Value>> {
    let appstream = get_appstream();
    let installed_fp = get_installed_flatpaks();
    let installed_rpm = get_installed_packages()?;
    let is_flatpak_source = source_type == "flatpak";

    let mut addons: Vec<serde_json::Value> = appstream
        .values()
        .filter(|a| {
            if !a.is_addon || a.extends != app_id { return false; }
            // Match add-on source to the selected source type
            if is_flatpak_source { a.source == "flatpak" } else { a.source != "flatpak" }
        })
        .map(|a| {
            let installed = if a.source == "flatpak" {
                fp_installed(&installed_fp, &a.id)
            } else {
                installed_rpm.contains(&a.package_name)
            };
            serde_json::json!({
                "id":           a.id,
                "name":         a.name,
                "summary":      a.summary,
                "source":       a.source,
                "package_name": a.package_name,
                "installed":    installed,
            })
        })
        .collect();

    addons.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });

    Ok(addons)
}

/// Build a `SourceOption` from a `NativeApp`, carrying its full display data.
fn source_option_from_native_app(app: &NativeApp) -> SourceOption {
    let remote = if app.source == "flatpak" {
        app.remotes.first().map(|s| s.as_str()).unwrap_or("flathub")
    } else {
        ""
    };
    SourceOption {
        id: if app.source == "flatpak" { fp_bare(&app.id).to_string() } else { app.id.clone() },
        source: app.source.clone(),
        package_name: app.package_name.clone(),
        installed: app.installed,
        label: source_label(&app.source, remote, false),
        remote: remote.to_string(),
        user_remote: false,
        summary: app.summary.clone(),
        description: app.description.clone(),
        version: app.version.clone(),
        developer: app.developer.clone(),
        url_homepage: app.url_homepage.clone(),
        url_bugtracker: app.url_bugtracker.clone(),
        url_donation: app.url_donation.clone(),
        url_help: app.url_help.clone(),
        url_faq: app.url_faq.clone(),
        url_vcs_browser: app.url_vcs_browser.clone(),
        url_contribute: app.url_contribute.clone(),
        license: app.license.clone(),
        screenshots: app.screenshots.clone(),
        icon_path: app.icon_path.clone(),
        icon_url: app.icon_url.clone(),
    }
}

/// Build a `SourceOption` from an `AppInfo`, carrying its full display data.
fn source_option_from_app_info(
    info: &AppInfo,
    installed: bool,
    remote: &str,
    user_remote: bool,
) -> SourceOption {
    SourceOption {
        id: if info.source == "flatpak" { fp_bare(&info.id).to_string() } else { info.id.clone() },
        source: info.source.clone(),
        package_name: info.package_name.clone(),
        installed,
        label: source_label(&info.source, remote, user_remote),
        remote: remote.to_string(),
        user_remote,
        summary: info.summary.clone(),
        description: info.description.clone(),
        version: info.version.clone(),
        developer: info.developer.clone(),
        url_homepage: info.url_homepage.clone(),
        url_bugtracker: info.url_bugtracker.clone(),
        url_donation: info.url_donation.clone(),
        url_help: info.url_help.clone(),
        url_faq: info.url_faq.clone(),
        url_vcs_browser: info.url_vcs_browser.clone(),
        url_contribute: info.url_contribute.clone(),
        license: info.license.clone(),
        screenshots: info.screenshots.clone(),
        icon_path: info.icon_path.clone(),
        icon_url: info.icon_url.clone(),
    }
}

/// Merge a flat list of apps (possibly with duplicate names from different sources)
/// into a map keyed by name_lower. When the same app has both a native/terra entry
/// and a flatpak entry, they are merged: native is primary, flatpak becomes sources[1].
fn merge_multi_source(apps: Vec<NativeApp>) -> HashMap<String, NativeApp> {
    // Group by name_lower
    let mut by_name: HashMap<String, Vec<NativeApp>> = HashMap::new();
    for app in apps {
        by_name.entry(app.name.to_lowercase()).or_default().push(app);
    }

    let mut out: HashMap<String, NativeApp> = HashMap::new();
    for (key, mut group) in by_name {
        if group.len() == 1 {
            out.insert(key, group.remove(0));
            continue;
        }

        // Sort: native/terra first so native is primary
        group.sort_by_key(|a| if a.source == "flatpak" { 1u8 } else { 0 });

        let mut primary = group.remove(0);
        let mut sources: Vec<SourceOption> = Vec::new();
        sources.push(source_option_from_native_app(&primary));
        for alt in &group {
            sources.push(source_option_from_native_app(alt));
            // Always prefer flatpak screenshots — Flathub CDN is reliable,
            // Fedora swcatalog screenshots often have encoding/format issues.
            if alt.source == "flatpak" && !alt.screenshots.is_empty() {
                primary.screenshots = alt.screenshots.clone();
            } else if primary.screenshots.is_empty() && !alt.screenshots.is_empty() {
                primary.screenshots = alt.screenshots.clone();
            }
            if primary.description.is_empty() && !alt.description.is_empty() {
                primary.description = alt.description.clone();
            }
        }
        primary.sources = sources;
        // Installed if any source is installed
        primary.installed = primary.sources.iter().any(|s| s.installed);
        out.insert(key, primary);
    }
    out
}

/// Build the list of available install sources for a given app (used by get_app_by_id).
/// Primary source is always first; alternate (native↔flatpak) is appended if found.
fn build_sources(
    primary: &NativeApp,
    app_id: &str,
    appstream: &HashMap<String, AppInfo>,
    installed_rpm: &HashSet<String>,
    _installed_fp: &HashSet<String>,
) -> Vec<SourceOption> {
    let mut sources = Vec::new();
    // System and user Flatpak installs are two separate `flatpak` installations
    // (/var/lib/flatpak vs ~/.local/share/flatpak) — query each scope
    // separately so the dropdown doesn't show both as installed just because
    // the app is installed in one of them.
    let installed_fp_system = get_installed_flatpaks_scoped("--system");
    let installed_fp_user = get_installed_flatpaks_scoped("--user");

    if primary.source == "flatpak" {
        // Always show both the system and user variant of the primary's own
        // remote — an app on the system remote is on the user remote too
        // (same catalog), never filter one out because of the other.
        let mut sys = source_option_from_native_app(primary);
        sys.installed = fp_installed(&installed_fp_system, &primary.id);
        sys.label = format!("{} (System)", sys.label);
        sources.push(sys);
        let mut usr = source_option_from_native_app(primary);
        usr.installed = fp_installed(&installed_fp_user, &primary.id);
        usr.user_remote = true;
        usr.label = format!("{} (User)", usr.label);
        sources.push(usr);
    } else {
        sources.push(source_option_from_native_app(primary));
    }

    if primary.source == "flatpak" {
        // Look for native alternate: injected stubs, pkg match, or name match
        let pkg = &primary.package_name;
        let native_key = format!("native:{}", pkg);
        let name_lc = primary.name.to_lowercase();
        let alt = appstream.get(&native_key)
            .or_else(|| appstream.values().find(|a| a.source != "flatpak" && a.package_name == *pkg))
            .or_else(|| appstream.values().find(|a| {
                a.source != "flatpak"
                    && a.name.to_lowercase() == name_lc
                    && !a.package_name.is_empty()
                    && !a.pkg_name_guessed
            }));
        if let Some(nat) = alt {
            sources.push(source_option_from_app_info(
                nat,
                installed_rpm.contains(&nat.package_name),
                "",
                false,
            ));
        }
    } else {
        // Native/terra — look for flatpak alternate:
        // 1. Direct "flatpak:{app_id}" key
        // 2. Reverse flatpak-to-rpm lookup: package_name → flatpak_id → "flatpak:{flatpak_id}"
        // 3. Name-based fallback
        let fp_key = format!("flatpak:{}", app_id);
        let name_lc = primary.name.to_lowercase();
        let rpm_to_fp = get_rpm_to_flatpak();
        let fp_via_rpm = rpm_to_fp.get(&primary.package_name)
            .and_then(|fp_id| appstream.get(&format!("flatpak:{}", fp_id)));
        let alt = appstream.get(&fp_key)
            .or(fp_via_rpm)
            .or_else(|| {
                appstream.values().find(|info| {
                    info.source == "flatpak"
                        && info.name.to_lowercase() == name_lc
                        && !info.package_name.is_empty()
                })
            });
        if let Some(fp) = alt {
            let installed_sys = fp_installed(&installed_fp_system, &fp.id);
            let installed_usr = fp_installed(&installed_fp_user, &fp.id);
            let remotes: Vec<&str> = if fp.remotes.is_empty() {
                vec!["flathub"]
            } else {
                fp.remotes.iter().map(|s| s.as_str()).collect()
            };

            for remote in remotes {
                // Always show both the system and user variant of this remote —
                // never filter one out because of the other. If a scope isn't
                // configured yet, the install flow adds it on demand.
                let mut sys_opt = source_option_from_app_info(fp, installed_sys, remote, false);
                sys_opt.label = format!("{} (System)", sys_opt.label);
                sources.push(sys_opt);
                sources.push(source_option_from_app_info(fp, installed_usr, remote, true));
            }
        }
    }

    // Native/RakuOS Linux always first in the dropdown
    sources.sort_by_key(|s| if s.source == "flatpak" { 1u8 } else { 0 });
    sources
}

/// For single-source apps (sources is empty after merge_multi_source), look for an
/// alternate source in the full appstream cache by name and populate sources if found.
/// This handles the case where only one source appears in a given category query.
fn enrich_sources(
    apps: &mut Vec<NativeApp>,
    appstream: &HashMap<String, AppInfo>,
    installed_rpm: &HashSet<String>,
    installed_fp: &HashSet<String>,
) {
    for app in apps.iter_mut() {
        if !app.sources.is_empty() {
            continue; // already merged from merge_multi_source
        }
        let name_lc = app.name.to_lowercase();

        if app.source == "flatpak" {
            // Look for native alternate by name
            let alt = appstream.values().find(|info| {
                info.source != "flatpak"
                    && info.name.to_lowercase() == name_lc
                    && !info.package_name.is_empty()
                    && !info.pkg_name_guessed
            });
            if let Some(nat) = alt {
                let nat_installed = installed_rpm.contains(&nat.package_name);
                let fp_src = source_option_from_native_app(app);
                let nat_src = source_option_from_app_info(nat, nat_installed, "", false);
                app.sources = vec![nat_src, fp_src]; // native first
                app.installed = app.sources.iter().any(|s| s.installed);
            }
        } else {
            // Native — look for flatpak alternate:
            // 1. Direct "flatpak:{id}" key
            // 2. Reverse flatpak-to-rpm lookup by package_name
            // 3. Name-based fallback
            let fp_key = format!("flatpak:{}", app.id);
            let rpm_to_fp = get_rpm_to_flatpak();
            let fp_via_rpm = rpm_to_fp.get(&app.package_name)
                .and_then(|fp_id| appstream.get(&format!("flatpak:{}", fp_id)));
            let alt = appstream.get(&fp_key)
                .or(fp_via_rpm)
                .or_else(|| {
                    appstream.values().find(|info| {
                        info.source == "flatpak"
                            && info.name.to_lowercase() == name_lc
                            && !info.package_name.is_empty()
                    })
                });
            if let Some(fp) = alt {
                let fp_installed = fp_installed(&installed_fp, &fp.id);
                let nat_src = source_option_from_native_app(app);
                let fp_src = source_option_from_app_info(fp, fp_installed, "flathub", false);
                app.sources = vec![nat_src, fp_src];
                app.installed = app.sources.iter().any(|s| s.installed);
            }
        }
    }
}

/// Returns the set of user-scoped flatpak remote names.
#[allow(dead_code)]
fn user_remote_names() -> HashSet<String> {
    let Ok(out) = Command::new("flatpak")
        .args(["remotes", "--columns=name,options"])
        .output()
    else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let name = parts.next()?.trim().to_string();
            let opts = parts.next().unwrap_or("").trim();
            if opts.split(',').any(|o| o.trim() == "user") {
                Some(name)
            } else {
                None
            }
        })
        .collect()
}

/// Human-readable label for an install source.
fn source_label(source: &str, remote: &str, user_remote: bool) -> String {
    match source {
        "native" | "terra" => "RakuOS Linux".to_string(),
        "flatpak" => {
            let base = if remote.is_empty() {
                "Flathub".to_string()
            } else {
                // "fedora-flatpaks" → "Fedora-Flatpaks"
                remote
                    .split('-')
                    .map(|part| {
                        let mut c = part.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("-")
            };
            if user_remote {
                format!("{} (User)", base)
            } else {
                base
            }
        }
        _ => source.to_string(),
    }
}

/// Install a native package via `pkexec dnf5 install <pkg>`.
/// Streams output lines; last line is "__done__<code>".
pub fn install_stream(package_name: &str) -> impl Iterator<Item = String> + '_ {
    run_scenter_stream(&["install", "-y", package_name])
}

/// Remove a native package via `pkexec dnf5 remove <pkg>`.
pub fn remove_stream(package_name: &str) -> impl Iterator<Item = String> + '_ {
    run_scenter_stream(&["remove", "-y", package_name])
}

/// Extract metadata from a local .rpm file using `rpm --queryformat -qp`.
/// Returns a JSON Value compatible with the detail page.
/// Includes `install_action`: "install" | "reinstall" | "upgrade".
pub fn get_local_rpm_info(path: &str) -> serde_json::Value {
    let fmt = "Name: %{NAME}\nVersion: %{VERSION}-%{RELEASE}\nSummary: %{SUMMARY}\nDescription: %{DESCRIPTION}\nURL: %{URL}\nLicense: %{LICENSE}\nSize: %{SIZE}\nArch: %{ARCH}";
    let mut data = std::collections::HashMap::new();
    if let Ok(out) = Command::new("rpm")
        .args(["--queryformat", fmt, "-qp", path])
        .output()
    {
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some((k, v)) = line.split_once(": ") {
                data.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
    }
    let stem = std::path::Path::new(path)
        .file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
    let name    = data.get("name").cloned().unwrap_or_else(|| stem.clone());
    let version = data.get("version").cloned().unwrap_or_default();
    let summary = data.get("summary").cloned().unwrap_or_default();
    let desc    = data.get("description").cloned().unwrap_or_default();
    let url     = data.get("url").cloned().filter(|u| u != "(none)").unwrap_or_default();
    let size: i64 = data.get("size").and_then(|s| s.parse().ok()).unwrap_or(0);
    let already_installed = is_installed(&name);
    let appstream = get_appstream();
    let icon_meta = appstream
        .values()
        .find(|app| app.package_name == name || app.id == name || app.name.eq_ignore_ascii_case(&name));
    let icon_path = icon_meta
        .map(|app| app.icon_path.clone())
        .unwrap_or_default();
    let icon_url = icon_meta
        .map(|app| app.icon_url.clone())
        .unwrap_or_default();

    // Determine installed version and what action is appropriate
    let installed_version = if already_installed {
        Command::new("rpm")
            .args(["-q", "--queryformat", "%{VERSION}-%{RELEASE}", &name])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let install_action = if !already_installed {
        "install"
    } else if version == installed_version {
        "reinstall"
    } else {
        "upgrade"
    };

    serde_json::json!({
        "id":                name,
        "name":              name,
        "version":           version,
        "installed_version": installed_version,
        "install_action":    install_action,
        "summary":           summary,
        "description":       desc,
        "url":               url,
        "size":              size,
        "license":           data.get("license").cloned().unwrap_or_default(),
        "arch":              data.get("arch").cloned().unwrap_or_default(),
        "source":            "native",
        "installed":         already_installed,
        "local_rpm":         path,
        "icon_path":         icon_path,
        "icon_url":          icon_url,
    })
}

/// Install a local .rpm file via `pkexec dnf5 install`.
/// Streams output lines; last line is "__done__<code>".
pub fn install_local_rpm_stream(path: &str) -> impl Iterator<Item = String> {
    run_scenter_stream(&["install", "-y", path])
}

/// Remove (from packages.list) then re-install a local .rpm.
/// Used for reinstall (same version already installed).
/// Streams output lines; last line is "__done__<code>" from the install step.
pub fn reinstall_local_rpm_stream(pkg_name: &str, path: &str) -> impl Iterator<Item = String> {
    let mut lines: Vec<String> = vec!["[Removing previous version...]".to_string()];
    for line in remove_stream(pkg_name) {
        if !line.starts_with("__done__") {
            lines.push(line);
        }
    }
    lines.push("[Installing new version...]".to_string());
    for line in install_local_rpm_stream(path) {
        lines.push(line);
    }
    lines.into_iter()
}

/// Check if a package name is installed via rpm -q.
pub fn is_installed(package_name: &str) -> bool {
    Command::new("rpm")
        .args(["-q", package_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Internals ─────────────────────────────────────────────────────────────────

const PACKAGES_LIST: &str = "/var/lib/software-center/packages.list";
const LOCAL_RPM_LIST: &str = "/var/lib/software-center/packages-rpm.list";

/// Read /var/lib/software-center/packages.list — the user's overlay package list.
/// Returns package names, skipping blank lines and comments.
fn read_packages_list() -> Vec<String> {
    match std::fs::read_to_string(PACKAGES_LIST) {
        Ok(content) => content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Try to find an icon for a locally-installed RPM package.
/// Checks (in order):
///   1. Icon= field from an installed .desktop file via `rpm -ql`
///   2. /usr/share/icons/hicolor/{size}/apps/{name}.png (by package name)
///   3. /usr/share/pixmaps/{name}.{ext}
fn find_local_rpm_icon(pkg: &str) -> String {
    // Get list of installed files for this package
    let files = Command::new("rpm")
        .args(["-ql", pkg])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // Look for an installed .desktop file and extract Icon=
    let mut icon_name = String::new();
    for line in files.lines() {
        let line = line.trim();
        if line.ends_with(".desktop") && line.starts_with("/usr/share/applications/") {
            if let Ok(content) = std::fs::read_to_string(line) {
                for dline in content.lines() {
                    if let Some(val) = dline.strip_prefix("Icon=") {
                        icon_name = val.trim().to_string();
                        break;
                    }
                }
            }
            if !icon_name.is_empty() {
                break;
            }
        }
    }

    // If no icon name found from .desktop, fall back to pkg name
    if icon_name.is_empty() {
        icon_name = pkg.to_string();
    }

    // Search hicolor theme for common sizes (largest first)
    for size in &["256x256", "128x128", "64x64", "48x48", "32x32"] {
        for ext in &["png", "svg", "xpm"] {
            let path = format!("/usr/share/icons/hicolor/{}/apps/{}.{}", size, icon_name, ext);
            if std::path::Path::new(&path).exists() {
                return path;
            }
        }
    }

    // Search pixmaps
    for ext in &["png", "svg", "xpm"] {
        let path = format!("/usr/share/pixmaps/{}.{}", icon_name, ext);
        if std::path::Path::new(&path).exists() {
            return path;
        }
    }

    // Also try hicolor scalable
    let scalable = format!("/usr/share/icons/hicolor/scalable/apps/{}.svg", icon_name);
    if std::path::Path::new(&scalable).exists() {
        return scalable;
    }

    String::new()
}

/// Read /var/lib/software-center/packages-rpm.list — locally cached .rpm installs.
/// Returns package names, skipping blank lines and comments.
fn read_local_rpm_list() -> Vec<String> {
    match std::fs::read_to_string(LOCAL_RPM_LIST) {
        Ok(content) => content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Check if a single package is installed via rpm -q.
fn rpm_is_installed(pkg: &str) -> bool {
    // For explicit arch-qualified names (e.g. "mangohud.i686") use direct rpm -q;
    // --whatprovides doesn't match "name.arch" syntax.
    // For plain names use --whatprovides so virtual provides resolve (e.g. "nodejs" → "nodejs22").
    let args: &[&str] = if pkg.ends_with(".i686") || pkg.ends_with(".x86_64") || pkg.ends_with(".noarch") {
        &["-q", "--quiet", pkg]
    } else {
        &["-q", "--quiet", "--whatprovides", pkg]
    };
    Command::new("rpm")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Return set of ALL installed RPM package names (for installed-status checks in browse/search).
/// Uses rpm -qa — only called for available/search/category, not for the installed list UI.
fn get_installed_packages() -> Result<HashSet<String>> {
    // For available() / search() / get_by_category() we use packages.list if present,
    // so the installed badge is accurate without querying the entire rpm database.
    // Fall back to rpm -qa only when packages.list doesn't exist (non-RakuOS systems).
    let list = read_packages_list();
    if !list.is_empty() {
        let mut resolved = HashSet::new();
        for pkg in &list {
            // Keep the original name (for exact matches in available/search views)
            resolved.insert(pkg.clone());
            // Also resolve the actual provider name (e.g. "nodejs22" for "nodejs")
            if let Ok(out) = Command::new("rpm")
                .args(["-q", "--whatprovides", "--queryformat", "%{NAME}\n", pkg])
                .output()
            {
                for line in String::from_utf8_lossy(&out.stdout).lines() {
                    let name = line.trim();
                    if !name.is_empty() && !name.starts_with("no package") {
                        resolved.insert(name.to_string());
                    }
                }
            }
        }
        return Ok(resolved);
    }
    // Non-RakuOS fallback
    let out = Command::new("rpm")
        .args(["-qa", "--queryformat", "%{NAME}\\n"])
        .output()?;
    let packages = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(String::from)
        .collect();
    Ok(packages)
}

/// Return set of installed Flatpak app IDs.
fn get_installed_flatpaks() -> HashSet<String> {
    let Ok(out) = Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
    else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Return set of Flatpak app IDs installed in a single scope ("--system" or
/// "--user") — used where the System/User install state must be reported
/// separately rather than as a single "installed anywhere" boolean.
fn get_installed_flatpaks_scoped(scope: &str) -> HashSet<String> {
    let Ok(out) = Command::new("flatpak")
        .args(["list", "--app", scope, "--columns=application"])
        .output()
    else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// Return installed Flatpak apps with basic metadata from `flatpak list`.
fn get_installed_flatpaks_with_info() -> Vec<(String, String, String, String, String)> {
    let Ok(out) = Command::new("flatpak")
        .args([
            "list",
            "--app",
            "--columns=application,name,version,description,origin",
        ])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.is_empty() || parts[0].is_empty() {
                return None;
            }
            Some((
                parts[0].to_string(),
                parts.get(1).unwrap_or(&"").to_string(),
                parts.get(2).unwrap_or(&"").to_string(),
                parts.get(3).unwrap_or(&"").to_string(),
                parts.get(4).unwrap_or(&"flathub").to_string(),
            ))
        })
        .collect()
}

fn run_scenter_stream(args: &[&str]) -> impl Iterator<Item = String> {
    // Fedora: install/remove native packages via dnf5 through pkexec.
    // e.g. args = ["install", "-y", "firefox"] → pkexec dnf5 install -y firefox
    let (subcmd, rest) = args.split_first().unwrap_or((&"", &[]));
    let mut full: Vec<&str> = vec!["dnf5", subcmd];
    full.extend_from_slice(rest);

    let mut child = Command::new("pkexec")
        .args(&full)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn pkexec dnf5");

    let stdout = child.stdout.take().unwrap();
    let lines: Vec<String> = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .collect();
    let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);

    lines
        .into_iter()
        .chain(std::iter::once(format!("__done__{}", code)))
}

// Needed for Default impl in get_installed_flatpaks_enriched
impl Default for NativeApp {
    fn default() -> Self {
        NativeApp {
            id: String::new(),
            name: String::new(),
            summary: String::new(),
            description: String::new(),
            version: String::new(),
            package_name: String::new(),
            icon: String::new(),
            icon_url: String::new(),
            icon_path: String::new(),
            categories: Vec::new(),
            keywords: Vec::new(),
            screenshots: Vec::new(),
            developer: String::new(),
            url_homepage: String::new(),
            url_bugtracker: String::new(),
            url_donation: String::new(),
            url_help: String::new(),
            url_faq: String::new(),
            url_vcs_browser: String::new(),
            url_contribute: String::new(),
            license: String::new(),
            source: String::new(),
            installed: false,
            is_addon: false,
            pkg_name_guessed: false,
            update_source: String::new(),
            update_url: String::new(),
            update_pattern: String::new(),
            sources: Vec::new(),
            remotes: Vec::new(),
        }
    }
}
