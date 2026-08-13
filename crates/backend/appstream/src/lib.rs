// scenter-appstream — AppStream metadata parsing and caching
//
// Reads compressed AppStream XML from /usr/share/swcatalog/xml/*.xml.gz
// and the overrides from /usr/share/software-center/appstream/appstream-overrides.json.
// Mirrors the logic in src/backend/packages.py (_load_appstream).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

/// Convert a Unix epoch timestamp (seconds) to "YYYY-MM-DD" (UTC).
/// Used for `<release timestamp="...">` entries that lack a `date` attribute.
fn epoch_to_date(ts: i64) -> Option<String> {
    let days = ts.div_euclid(86_400);
    civil_from_days(days).map(|(y, m, d)| format!("{y:04}-{m:02}-{d:02}"))
}

/// Convert days since 1970-01-01 to a (year, month, day) civil date (UTC).
/// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(z: i64) -> Option<(i64, i64, i64)> {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    Some((y, m, d))
}

// ── Global in-memory cache ────────────────────────────────────────────────────
// Parsed once on first call, shared across all callers via Arc (no re-parsing).
// Wrapped in a RwLock so reload_appstream() can swap in fresh data (e.g. after
// a Flatpak remote is added/removed) without requiring an app restart.

static APPSTREAM_CACHE: OnceLock<RwLock<Arc<HashMap<String, AppInfo>>>> = OnceLock::new();
/// Reverse of flatpak-to-rpm.json: rpm_name → flatpak_id (lower-cased flatpak id).
static RPM_TO_FLATPAK_CACHE: OnceLock<Arc<HashMap<String, String>>> = OnceLock::new();

/// Returns a reference-counted handle to the shared appstream data.
/// First call parses all XML files; subsequent calls return in nanoseconds.
pub fn get_appstream() -> Arc<HashMap<String, AppInfo>> {
    APPSTREAM_CACHE
        .get_or_init(|| RwLock::new(Arc::new(load_appstream_inner().unwrap_or_default())))
        .read()
        .unwrap()
        .clone()
}

/// Re-parse all AppStream XML files from disk and swap them into the shared
/// cache in place, so the next get_appstream() call (from any UI) sees the
/// update live. Call this after adding/removing/enabling a Flatpak remote.
pub fn reload_appstream() {
    let fresh = Arc::new(load_appstream_inner().unwrap_or_default());
    let lock = APPSTREAM_CACHE.get_or_init(|| RwLock::new(fresh.clone()));
    *lock.write().unwrap() = fresh;
}

/// Returns the reverse of flatpak-to-rpm.json: maps rpm_package_name → flatpak_app_id.
/// Used to find the flatpak counterpart of a native stub by its package name.
pub fn get_rpm_to_flatpak() -> Arc<HashMap<String, String>> {
    RPM_TO_FLATPAK_CACHE
        .get_or_init(|| {
            let map: HashMap<String, String> = load_flatpak_to_rpm()
                .into_iter()
                .map(|(fp_id, rpm)| (rpm, fp_id))
                .collect();
            Arc::new(map)
        })
        .clone()
}

/// Compatibility wrapper — prefer get_appstream() to avoid cloning the map.
pub fn load_appstream() -> Result<HashMap<String, AppInfo>> {
    Ok((*get_appstream()).clone())
}

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub icon: String,
    pub icon_url: String,
    pub icon_path: String,        // resolved local filesystem path (empty if not found)
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub screenshots: Vec<String>,
    pub source: String,           // "native", "flatpak", "terra", etc.
    pub package_name: String,
    pub version: String,
    pub developer: String,
    pub url_homepage: String,
    pub url_bugtracker: String,
    pub url_donation: String,
    pub url_help: String,
    pub url_faq: String,
    pub url_vcs_browser: String,
    pub url_contribute: String,
    pub license: String,
    pub content_rating: String,
    pub is_addon: bool,
    pub extends: String,          // parent app id for addons
    pub pkg_name_guessed: bool,   // true when package_name was guessed from id, not from <pkgname>
    #[serde(default)]
    pub remotes: Vec<String>,     // all flatpak remote names this app id was found in (e.g. ["flathub", "cosmic"]); empty for non-flatpak sources
    #[serde(default)]
    pub component_type: String,   // appstream component type: "desktop", "desktop-application", "font", "codec", ...
    #[serde(default)]
    pub updated: String,          // latest <release date="YYYY-MM-DD"> (empty if none)
}

// ── Paths ─────────────────────────────────────────────────────────────────────

const SWCATALOG_DIR: &str = "/usr/share/swcatalog/xml";
const APPSTREAM_DATA_DIR: &str = "/usr/share/software-center/appstream/data";
const APP_INFO_XMLS: &str = "/usr/share/app-info/xmls";
const APP_INFO_XMLS_CACHE: &str = "/var/cache/app-info/xmls";
const OVERRIDES_PATH: &str = "/usr/share/software-center/appstream/appstream-overrides.json";
const FLATPAK_TO_RPM_PATH: &str = "/usr/share/software-center/appstream/flatpak-to-rpm.json";

// Icon search directories — matched to Python's ICON_DIRS list
const STATIC_ICON_DIRS: &[&str] = &[
    "/usr/share/software-center/appstream/icons",
    "/usr/share/swcatalog/icons/fedora/64x64",
    "/usr/share/swcatalog/icons/fedora/128x128",
    "/var/lib/flatpak/appstream/flathub/x86_64/active/icons/128x128",
    "/var/lib/flatpak/appstream/flathub/x86_64/active/icons/64x64",
    "/var/lib/flatpak/appstream/fedora-flatpaks/x86_64/active/icons/64x64",
    "/usr/share/app-info/icons",
    "/usr/share/icons/hicolor/256x256/apps",
    "/usr/share/icons/hicolor/scalable/apps",
    "/usr/share/icons/hicolor/128x128/apps",
    "/usr/share/icons/hicolor/64x64/apps",
    "/usr/share/icons/hicolor/48x48/apps",
    "/usr/share/pixmaps",
];

// Version-specific icon dirs — built at runtime from /etc/os-release
fn versioned_icon_dirs(fedora_ver: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    // RPM Fusion
    for repo in &["rpmfusion-free", "rpmfusion-nonfree"] {
        dirs.push(format!("/usr/share/swcatalog/icons/{}-{}/64x64", repo, fedora_ver));
    }
    // Terra
    let terra_suffixes = ["", "-mesa", "-nvidia", "-extras", "-multimedia"];
    for suffix in &terra_suffixes {
        for size in &["64x64", "128x128"] {
            dirs.push(format!(
                "/usr/share/swcatalog/icons/terra{}{}/{}",
                fedora_ver, suffix, size
            ));
        }
    }
    dirs
}

fn get_fedora_version() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("VERSION_ID=") {
                return rest.trim_matches('"').to_string();
            }
        }
    }
    "44".to_string()
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns true if a path looks like an AppStream XML file (.xml or .xml.gz).
fn is_appstream_file(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".xml.gz") || name.ends_with(".xml")
}

/// Recursively scan a directory tree for AppStream XML files (.xml and .xml.gz).
/// Stops recursing at `max_depth` to avoid traversing huge trees.
fn scan_appstream_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth == 0 || !dir.exists() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_appstream_files(&p, out, depth - 1);
        } else if is_appstream_file(&p) {
            out.push(p);
        }
    }
}

/// Internal: parse all AppStream XML files from disk. Called at most once.
fn load_appstream_inner() -> Result<HashMap<String, AppInfo>> {
    let mut apps: HashMap<String, AppInfo> = HashMap::new();
    let mut xml_files: Vec<PathBuf> = Vec::new();

    // Bundled custom entries (highest priority, plain .xml)
    scan_appstream_files(Path::new(APPSTREAM_DATA_DIR), &mut xml_files, 3);

    // System swcatalog (native + terra; mix of .xml and .xml.gz)
    scan_appstream_files(Path::new(SWCATALOG_DIR), &mut xml_files, 2);

    // app-info dirs
    scan_appstream_files(Path::new(APP_INFO_XMLS), &mut xml_files, 2);
    scan_appstream_files(Path::new(APP_INFO_XMLS_CACHE), &mut xml_files, 2);

    // Per-package metainfo (installed packages that ship their own AppStream XML
    // but aren't in the swcatalog, e.g. packages from COPR like lact, rpcs3)
    scan_appstream_files(Path::new("/usr/share/metainfo"), &mut xml_files, 1);
    scan_appstream_files(Path::new("/usr/share/appdata"), &mut xml_files, 1);

    // Flatpak AppStream: system-wide (flathub/x86_64/active/ is 3 levels deep)
    scan_appstream_files(Path::new("/var/lib/flatpak/appstream"), &mut xml_files, 5);

    // Flatpak AppStream: per-user
    if let Ok(home) = std::env::var("HOME") {
        let user_fp = PathBuf::from(home).join(".local/share/flatpak/appstream");
        scan_appstream_files(&user_fp, &mut xml_files, 5);
    }

    // Prefer .xml.gz over .xml when both exist for the same base name
    // (flatpak ships both; prefer the gz to avoid double-parsing)
    let xml_files = dedup_prefer_gz(xml_files);

    for path in &xml_files {
        if let Err(e) = parse_catalog_file(path, &mut apps) {
            log::warn!("Failed to parse {:?}: {}", path, e);
        }
    }

    // Apply overrides
    apply_overrides(&mut apps);

    // Inject native stubs from flatpak-to-rpm.json:
    // Many apps (Firefox, VLC, etc.) have no native RPM AppStream on Fedora but
    // have rich Flatpak AppStream — inject a native stub so users can install
    // the RPM with full metadata (name, icon, description, screenshots).
    inject_native_stubs(&mut apps);

    // Resolve icon paths
    let fedora_ver = get_fedora_version();
    let terra_dirs = versioned_icon_dirs(&fedora_ver);
    let home_icon_cache = std::env::var("HOME")
        .map(|h| format!("{}/.cache/software-center/icons", h))
        .unwrap_or_default();

    let app_ids: Vec<String> = apps.keys().cloned().collect();
    for id in app_ids {
        let app = apps.get_mut(&id).unwrap();
        if app.icon_path.is_empty() {
            app.icon_path = resolve_icon_path(
                app,
                &home_icon_cache,
                &terra_dirs,
            );
        }
        // Synthesize icon_url fallback for apps without a local icon:
        //   org.kde.*  → apps.kde.org SVG (mirrors Python _FetchThread logic)
        //   Flathub-only Flatpak → Flathub CDN PNG (predictable URL pattern)
        // Non-flathub remotes are skipped: their app IDs don't exist on the
        // Flathub CDN, so a synthesized URL would 404.
        if app.icon_path.is_empty() && app.icon_url.is_empty() {
            let clean_id = app.id.trim_start_matches("flatpak:").trim_end_matches(".desktop");
            if clean_id.starts_with("org.kde.") {
                app.icon_url = format!("https://apps.kde.org/app-icons/{}.svg", clean_id);
            } else if app.source == "flatpak"
                && (app.remotes.is_empty()
                    || app.remotes.iter().any(|r| r.eq_ignore_ascii_case("flathub")))
            {
                // Flathub CDN: /repo/appstream/x86_64/icons/128x128/{id}.png
                app.icon_url = format!(
                    "https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{}.png",
                    clean_id
                );
            }
        }
    }

    log::info!(
        "AppStream: loaded {} apps from {} files",
        apps.len(),
        xml_files.len()
    );
    Ok(apps)
}

/// When a directory ships both `appstream.xml` and `appstream.xml.gz`,
/// keep only the `.gz` to avoid parsing the same data twice.
fn dedup_prefer_gz(files: Vec<PathBuf>) -> Vec<PathBuf> {
    use std::collections::HashSet;
    // Collect all .gz base paths (without the .gz suffix) for fast lookup
    let gz_bases: HashSet<PathBuf> = files
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gz"))
        .filter_map(|p| {
            // "appstream.xml.gz" → strip ".gz" → "appstream.xml"
            let s = p.to_str()?;
            Some(PathBuf::from(s.strip_suffix(".gz")?))
        })
        .collect();

    files
        .into_iter()
        .filter(|p| {
            // If this is a plain .xml and a .gz version exists, skip it
            let ext = p.extension().and_then(|e| e.to_str());
            if ext == Some("xml") && gz_bases.contains(p.as_path()) {
                return false;
            }
            true
        })
        .collect()
}

/// Scan `/var/lib/flatpak/app/<app_id>/x86_64/stable/*/files/share/app-info/icons/flatpak/`
/// for `<app_id>.desktop.png` or `<app_id>.png` in any size directory.
/// The deploy hash subdirectory changes per install so we enumerate it.
fn find_flatpak_app_icon(app_id: &str) -> Option<String> {
    // Flatpak deploy dirs use the bare ID (no .desktop suffix)
    let bare_id = app_id.trim_end_matches(".desktop");
    let deploy_base = format!("/var/lib/flatpak/app/{}/x86_64/stable", bare_id);
    let deploy_dir = Path::new(&deploy_base);
    if !deploy_dir.exists() {
        return None;
    }
    let Ok(hashes) = std::fs::read_dir(deploy_dir) else { return None };
    for hash_entry in hashes.flatten() {
        let icon_base = hash_entry.path()
            .join("files/share/app-info/icons/flatpak");
        if !icon_base.exists() {
            continue;
        }
        let Ok(sizes) = std::fs::read_dir(&icon_base) else { continue };
        for size_entry in sizes.flatten() {
            let size_dir = size_entry.path();
            // Try <id>.desktop.png first, then plain <id>.png
            for filename in &[
                format!("{}.desktop.png", app_id),
                format!("{}.png", app_id),
            ] {
                let p = size_dir.join(filename);
                if p.exists() {
                    return Some(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

/// Resolve the local icon path for an app, checking all icon dirs.
/// Returns empty string if not found.
pub fn resolve_icon_path(app: &AppInfo, home_icon_cache: &str, dynamic_dirs: &[String]) -> String {
    let app_id = &app.id;
    let source = &app.source;

    // For Flatpak apps, icon is always named {app_id}.png
    // Check in the Flatpak icon cache dirs first
    if source == "flatpak" {
        let candidates = [
            format!("/var/lib/flatpak/appstream/flathub/x86_64/active/icons/128x128/{}.png", app_id),
            format!("/var/lib/flatpak/appstream/flathub/x86_64/active/icons/64x64/{}.png", app_id),
            format!("/var/lib/flatpak/appstream/fedora-flatpaks/x86_64/active/icons/64x64/{}.png", app_id),
        ];
        for p in &candidates {
            if Path::new(p).exists() {
                return p.clone();
            }
        }
        // Check per-app deploy directory (handles apps not yet in appstream cache,
        // e.g. freshly installed or with .desktop.png suffix like EasyEffects)
        if let Some(p) = find_flatpak_app_icon(app_id) {
            return p;
        }
        // Check home icon cache (downloaded from Flathub API)
        if !home_icon_cache.is_empty() {
            let cached = format!("{}/{}.png", home_icon_cache, app_id);
            if Path::new(&cached).exists() {
                return cached;
            }
        }
    }

    // For all sources, try the icon field (filename or absolute path)
    if !app.icon.is_empty() {
        // Absolute path (e.g. from <icon type="local"> in metainfo) — use directly if it exists
        if app.icon.starts_with('/') && Path::new(&app.icon).exists() {
            return app.icon.clone();
        }
        let icon = &app.icon;
        let stem = icon.trim_end_matches(".png")
            .trim_end_matches(".svg")
            .trim_end_matches(".xpm");

        // Build search dirs: static + terra
        let mut search_dirs: Vec<&str> = STATIC_ICON_DIRS.to_vec();
        let terra_refs: Vec<&str> = dynamic_dirs.iter().map(|s| s.as_str()).collect();
        search_dirs.extend_from_slice(&terra_refs);

        for dir in &search_dirs {
            for candidate in &[
                icon.clone(),
                format!("{}.png", stem),
                format!("{}.svg", stem),
            ] {
                let path = format!("{}/{}", dir, candidate);
                if Path::new(&path).exists() {
                    return path;
                }
            }
        }
    }

    // Home icon cache fallback
    if !home_icon_cache.is_empty() {
        let cached = format!("{}/{}.png", home_icon_cache, app_id);
        if Path::new(&cached).exists() {
            return cached;
        }
    }

    // Native/terra fallback: apps without an explicit <icon> tag (e.g. COPR packages)
    // Try the package_name or the last segment of the app ID as the icon stem.
    // This mirrors what the .desktop Icon= field usually contains.
    if matches!(source.as_str(), "native" | "terra" | "local-rpm") {
        let clean_id = app_id.trim_end_matches(".desktop");
        let stem = if !app.package_name.is_empty() {
            app.package_name.as_str()
        } else {
            clean_id.split('.').next_back().unwrap_or(clean_id)
        };
        if !stem.is_empty() {
            let mut search_dirs: Vec<&str> = STATIC_ICON_DIRS.to_vec();
            let terra_refs: Vec<&str> = dynamic_dirs.iter().map(|s| s.as_str()).collect();
            search_dirs.extend_from_slice(&terra_refs);
            for dir in &search_dirs {
                for ext in &["png", "svg", "xpm"] {
                    let path = format!("{}/{}.{}", dir, stem, ext);
                    if Path::new(&path).exists() {
                        return path;
                    }
                }
            }
        }
    }

    String::new()
}

/// Load the flatpak-to-rpm mapping (Flatpak app id → RPM package name).
pub fn load_flatpak_to_rpm() -> HashMap<String, String> {
    let path = Path::new(FLATPAK_TO_RPM_PATH);
    if !path.exists() {
        return HashMap::new();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Extract the Flatpak remote name from an AppStream file path, e.g.
/// "/var/lib/flatpak/appstream/flathub/x86_64/active/appstream.xml.gz" → "flathub"
/// "$HOME/.local/share/flatpak/appstream/cosmic/x86_64/active/appstream.xml.gz" → "cosmic"
/// Returns empty string if the path doesn't match the expected flatpak layout
/// (e.g. swcatalog/app-info files that merely mention "flatpak" in their name).
fn derive_flatpak_remote(path: &Path) -> String {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    for (i, c) in comps.iter().enumerate() {
        if *c == "appstream" && i > 0 && comps[i - 1] == "flatpak" {
            if let Some(remote) = comps.get(i + 1) {
                return remote.to_string();
            }
        }
    }
    String::new()
}

/// Merge in any remotes already recorded under `key` so multi-remote
/// membership (e.g. an app present in both "flathub" and "cosmic") isn't
/// lost when a later file for the same app id overwrites the entry.
fn merge_remotes(apps: &HashMap<String, AppInfo>, key: &str, app: &mut AppInfo) {
    if let Some(ex) = apps.get(key) {
        for r in &ex.remotes {
            if !app.remotes.contains(r) {
                app.remotes.push(r.clone());
            }
        }
    }
}

fn parse_catalog_file(path: &Path, apps: &mut HashMap<String, AppInfo>) -> Result<()> {
    use flate2::read::GzDecoder;
    use quick_xml::Reader;
    use quick_xml::events::Event;

    // Determine source from the full path (not just filename — flatpak ships
    // a generic "appstream.xml.gz" that only reveals its origin via directory)
    let path_str = path.to_string_lossy().to_lowercase();
    let source = if path_str.contains("flathub") || path_str.contains("flatpak") {
        "flatpak"
    } else if path_str.contains("terra") {
        "terra"
    } else {
        "native"
    };
    let remote = if source == "flatpak" {
        derive_flatpak_remote(path)
    } else {
        String::new()
    };

    let file = File::open(path)?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let mut reader = if file_name.ends_with(".xml.gz") {
        let gz = GzDecoder::new(BufReader::new(file));
        Reader::from_reader(BufReader::new(Box::new(gz) as Box<dyn std::io::Read>))
    } else {
        // Plain .xml
        Reader::from_reader(BufReader::new(Box::new(file) as Box<dyn std::io::Read>))
    };
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut current: Option<AppInfo> = None;
    let mut in_id = false;
    let mut in_name = false;
    let mut in_summary = false;
    let mut in_description = false;
    let mut in_pkgname = false;
    let mut in_developer = false;
    let mut in_developer_block = false; // true while inside <developer>...</developer> (newer format)
    let mut in_category = false;
    let mut in_keyword = false;
    let mut in_icon_cached = false;
    let mut in_icon_remote = false;
    let mut in_screenshot_image = false;
    let mut current_url_type: Option<String> = None;
    let mut in_extends = false;
    let mut in_releases = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name().as_ref() {
                    b"component" => {
                        let kind = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| {
                                String::from_utf8_lossy(&a.value).to_lowercase()
                            })
                            .unwrap_or_default();
                        current = Some(AppInfo {
                            source: source.to_string(),
                            is_addon: kind == "addon",
                            component_type: kind.clone(),
                            remotes: if remote.is_empty() { Vec::new() } else { vec![remote.clone()] },
                            ..Default::default()
                        });
                    }
                    b"id" if current.is_some() => in_id = true,
                    b"name" if current.is_some() => {
                        let lang_ok = !e.attributes().any(|a| {
                            a.map(|a| a.key.as_ref() == b"xml:lang").unwrap_or(false)
                        });
                        if in_developer_block {
                            // <name> inside <developer> → developer name, not app name
                            in_developer = lang_ok;
                        } else {
                            in_name = lang_ok;
                        }
                    }
                    b"summary" if current.is_some() => {
                        in_summary = !e.attributes().any(|a| {
                            a.map(|a| a.key.as_ref() == b"xml:lang").unwrap_or(false)
                        });
                    }
                    b"description" if current.is_some() => {
                        in_description = !e.attributes().any(|a| {
                            a.map(|a| a.key.as_ref() == b"xml:lang").unwrap_or(false)
                        });
                    }
                    b"pkgname" if current.is_some() => in_pkgname = true,
                    b"developer" if current.is_some() => {
                        // Newer AppStream format: <developer><name>...</name></developer>
                        // Don't set in_developer here; the inner <name> will do it.
                        in_developer_block = true;
                    }
                    b"developer_name" if current.is_some() => {
                        // Older AppStream format: <developer_name>...</developer_name>
                        in_developer = !e.attributes().any(|a| {
                            a.map(|a| a.key.as_ref() == b"xml:lang").unwrap_or(false)
                        });
                    }
                    b"icon" if current.is_some() => {
                        // "cached" and "stock" both provide a local icon name/file to search for.
                        // "local" provides an absolute path — treat the same way.
                        in_icon_cached = e.attributes().any(|a| {
                            a.map(|a| {
                                a.key.as_ref() == b"type" && matches!(
                                    a.value.as_ref(),
                                    b"cached" | b"stock" | b"local"
                                )
                            })
                            .unwrap_or(false)
                        });
                        in_icon_remote = e.attributes().any(|a| {
                            a.map(|a| {
                                a.key.as_ref() == b"type" && a.value.as_ref() == b"remote"
                            })
                            .unwrap_or(false)
                        });
                    }
                    b"category" if current.is_some() => in_category = true,
                    b"keyword" if current.is_some() => {
                        let lang_ok = !e.attributes().any(|a| {
                            a.map(|a| a.key.as_ref() == b"xml:lang").unwrap_or(false)
                        });
                        in_keyword = lang_ok;
                    }
                    b"image" if current.is_some() => {
                        // Only capture "source" (full-res) images — skip thumbnails.
                        // AppStream ships one type="source" + multiple type="thumbnail"
                        // entries per screenshot; thumbnails are lower-res dupes.
                        let img_type = e.attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| String::from_utf8_lossy(&a.value).to_lowercase());
                        in_screenshot_image = matches!(
                            img_type.as_deref(),
                            None | Some("source")
                        );
                    }
                    b"url" if current.is_some() => {
                        current_url_type = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"type")
                            .map(|a| String::from_utf8_lossy(&a.value).to_lowercase());
                    }
                    b"extends" if current.is_some() => in_extends = true,
                    b"releases" if current.is_some() => in_releases = true,
                    b"release" if current.is_some() && in_releases => {
                        // <release date="YYYY-MM-DD" .../> or <release timestamp="<epoch>"/>
                        // — keep the newest date only (releases may be unordered).
                        let attrs: Vec<_> = e.attributes().filter_map(|a| a.ok()).collect();
                        let mut new_date = attrs.iter()
                            .find(|a| a.key.as_ref() == b"date")
                            .map(|a| String::from_utf8_lossy(&a.value).to_string());
                        if new_date.is_none() {
                            if let Some(ts) = attrs.iter()
                                .find(|a| a.key.as_ref() == b"timestamp")
                                .and_then(|a| std::str::from_utf8(&a.value).ok())
                                .and_then(|s| s.parse::<i64>().ok())
                            {
                                new_date = epoch_to_date(ts);
                            }
                        }
                        if let Some(app) = current.as_mut() {
                            if let Some(d) = new_date {
                                if d > app.updated {
                                    app.updated = d;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if let Some(app) = current.as_mut() {
                    if in_id && app.id.is_empty() {
                        app.id = text;
                        in_id = false;
                    } else if in_name && app.name.is_empty() {
                        app.name = text;
                        in_name = false;
                    } else if in_summary && app.summary.is_empty() {
                        app.summary = text;
                        in_summary = false;
                    } else if in_description && app.description.is_empty() {
                        app.description = text;
                        in_description = false;
                    } else if in_pkgname && app.package_name.is_empty() {
                        // Strip packaging transition suffixes so the base installable
                        // package name is used:
                        //   "zed-rename-zeditor"    → "zed"  (DNF5 rename transition)
                        //   "zed-cli-compat-zfs"    → "zed"  (CLI/ZFS compat subpackage)
                        app.package_name = if let Some(pos) = text.find("-rename-") {
                            text[..pos].to_string()
                        } else if let Some(pos) = text.find("-cli-compat-") {
                            text[..pos].to_string()
                        } else {
                            text
                        };
                        in_pkgname = false;
                    } else if in_developer && app.developer.is_empty() {
                        app.developer = text;
                        in_developer = false;
                    } else if in_icon_cached && app.icon.is_empty() {
                        app.icon = text;
                        in_icon_cached = false;
                    } else if in_icon_remote && app.icon_url.is_empty() {
                        app.icon_url = text;
                        in_icon_remote = false;
                    } else if in_category {
                        app.categories.push(text);
                        in_category = false;
                    } else if in_keyword {
                        app.keywords.push(text);
                        in_keyword = false;
                    } else if in_screenshot_image && !text.is_empty() {
                        // Only add http URLs (not local paths)
                        if text.starts_with("http") {
                            app.screenshots.push(text);
                        }
                        in_screenshot_image = false;
                    } else if let Some(url_type) = current_url_type.take() {
                        set_app_url(app, &url_type, text);
                    } else if in_extends && app.extends.is_empty() {
                        app.extends = text;
                        in_extends = false;
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == b"developer" {
                    in_developer_block = false;
                } else if e.name().as_ref() == b"releases" {
                    in_releases = false;
                } else if e.name().as_ref() == b"component" {
                    if let Some(mut app) = current.take() {
                        if !app.id.is_empty() {
                            // For Flatpak apps: icon is always {app_id}.png
                            if app.source == "flatpak" {
                                app.icon = format!("{}.png", app.id);
                                if app.package_name.is_empty() {
                                    app.package_name = app.id.clone();
                                }
                            } else if app.package_name.is_empty() {
                                // Native with no <pkgname> — guess from last segment of id
                                let clean = app.id.trim_end_matches(".desktop");
                                let guess = clean.split('.').next_back().unwrap_or(clean).to_lowercase();
                                app.package_name = guess;
                                app.pkg_name_guessed = true;
                            }
                            // Capture explicit package_name from any existing entry
                            // before borrowing apps mutably. Installed packages ship
                            // metainfo without <pkgname>; the swcatalog has the explicit
                            // name. Whichever is processed last would otherwise clobber
                            // the explicit name with a guessed one.
                            let existing_explicit_pkg: Option<(String, String)> = apps.get(&app.id)
                                .filter(|ex| !ex.pkg_name_guessed && !ex.package_name.is_empty())
                                .map(|ex| (ex.package_name.clone(), ex.source.clone()));

                            let existing = apps.get(&app.id);
                            match existing {
                                None => {
                                    apps.insert(app.id.clone(), app);
                                }
                                Some(ex) if ex.source != "flatpak" && app.source == "flatpak" => {
                                    // Native already primary — enrich it with flatpak metadata
                                    // and preserve flatpak under its own key.
                                    let app_id = app.id.clone();
                                    let ex = apps.get_mut(&app_id).unwrap();
                                    // Always prefer flatpak screenshots (Flathub CDN reliable;
                                    // Fedora swcatalog screenshots often have encoding issues).
                                    if !app.screenshots.is_empty() {
                                        ex.screenshots = app.screenshots.clone();
                                    }
                                    if ex.icon.is_empty() && !app.icon.is_empty() {
                                        ex.icon = app.icon.clone();
                                    }
                                    fill_missing_app_links(ex, &app);
                                    let fp_key = format!("flatpak:{}", app_id);
                                    merge_remotes(apps, &fp_key, &mut app);
                                    apps.insert(fp_key, app);
                                }
                                Some(ex) if ex.source == "flatpak" && app.source != "flatpak" => {
                                    // Flatpak was stored first; native arrived later.
                                    // Promote native to primary, move flatpak to its own key.
                                    let fp_key = format!("flatpak:{}", app.id);
                                    let mut old_fp = apps.remove(&app.id).unwrap();
                                    apps.insert(app.id.clone(), app);
                                    merge_remotes(apps, &fp_key, &mut old_fp);
                                    apps.insert(fp_key, old_fp);
                                }
                                _ => {
                                    // Same source type conflict — last writer wins for display
                                    // fields, but never downgrade an explicit package_name to a
                                    // guessed one, never lose categories/icon that the existing
                                    // entry has (installed-package metainfo often omits both),
                                    // and never lose track of other remotes this app id was
                                    // already found in (e.g. present in both "flathub" and
                                    // "cosmic") — merge remotes rather than overwrite.
                                    if app.pkg_name_guessed {
                                        if let Some((pkg, _)) = existing_explicit_pkg {
                                            app.package_name = pkg;
                                            app.pkg_name_guessed = false;
                                        }
                                    }
                                    if let Some(ex) = apps.get(&app.id) {
                                        if app.categories.is_empty() && !ex.categories.is_empty() {
                                            app.categories = ex.categories.clone();
                                        }
                                        if app.icon.is_empty() && !ex.icon.is_empty() {
                                            app.icon = ex.icon.clone();
                                        }
                                    }
                                    let id = app.id.clone();
                                    merge_remotes(apps, &id, &mut app);
                                    apps.insert(id, app);
                                }
                            }
                        }
                    }
                }
                in_id = false;
                in_name = false;
                in_summary = false;
                in_description = false;
                in_pkgname = false;
                in_developer = false;
                in_category = false;
                in_keyword = false;
                in_icon_cached = false;
                in_icon_remote = false;
                in_screenshot_image = false;
                current_url_type = None;
                in_extends = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                log::warn!("XML parse error in {:?}: {}", path, e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(())
}

fn set_app_url(app: &mut AppInfo, url_type: &str, text: String) {
    if text.is_empty() {
        return;
    }
    match url_type {
        "homepage" if app.url_homepage.is_empty() => app.url_homepage = text,
        "bugtracker" if app.url_bugtracker.is_empty() => app.url_bugtracker = text,
        "donation" if app.url_donation.is_empty() => app.url_donation = text,
        "help" if app.url_help.is_empty() => app.url_help = text,
        "faq" if app.url_faq.is_empty() => app.url_faq = text,
        "vcs-browser" if app.url_vcs_browser.is_empty() => app.url_vcs_browser = text,
        "contribute" if app.url_contribute.is_empty() => app.url_contribute = text,
        _ => {}
    }
}

fn fill_missing_app_links(target: &mut AppInfo, source: &AppInfo) {
    if target.url_homepage.is_empty() {
        target.url_homepage = source.url_homepage.clone();
    }
    if target.url_bugtracker.is_empty() {
        target.url_bugtracker = source.url_bugtracker.clone();
    }
    if target.url_donation.is_empty() {
        target.url_donation = source.url_donation.clone();
    }
    if target.url_help.is_empty() {
        target.url_help = source.url_help.clone();
    }
    if target.url_faq.is_empty() {
        target.url_faq = source.url_faq.clone();
    }
    if target.url_vcs_browser.is_empty() {
        target.url_vcs_browser = source.url_vcs_browser.clone();
    }
    if target.url_contribute.is_empty() {
        target.url_contribute = source.url_contribute.clone();
    }
}

/// Inject native install stubs for apps in flatpak-to-rpm.json.
/// On Fedora, many popular apps (Firefox, VLC, etc.) have rich Flatpak AppStream
/// but sparse or missing native RPM AppStream. This borrows the Flatpak metadata
/// and presents a native install option alongside the Flatpak one.
fn inject_native_stubs(apps: &mut HashMap<String, AppInfo>) {
    let fp_to_rpm = load_flatpak_to_rpm();
    if fp_to_rpm.is_empty() {
        return;
    }

    let mut stubs: Vec<(String, AppInfo)> = Vec::new();
    // Flatpak entries that need to be re-keyed to "flatpak:{id}" prefix
    let mut rekeys: Vec<(String, String)> = Vec::new(); // (old_key, new_key)

    for (fp_id_lower, rpm_name) in &fp_to_rpm {
        // Find the flatpak entry by case-insensitive ID match
        let fp_id_lc = fp_id_lower.to_lowercase();
        let fp_entry = apps.iter().find(|(_, a)| {
            a.source == "flatpak" && a.id.to_lowercase() == fp_id_lc
        }).map(|(k, a)| (k.clone(), a.clone()));

        let Some((fp_key, fp_entry)) = fp_entry else { continue };

        // Always re-key the flatpak to "flatpak:{id}" so build_sources can find it
        // regardless of whether a native entry already exists.
        let prefixed_key = format!("flatpak:{}", fp_entry.id);
        if fp_key != prefixed_key && !apps.contains_key(&prefixed_key) {
            rekeys.push((fp_key, prefixed_key));
        }

        // Skip stub creation if a native entry already exists with this package_name.
        // If so, confirm the mapping is explicit (not guessed) so the entry stays
        // browseable and visible in build_sources name-based fallback.
        let already_exists = apps.values().any(|a| {
            a.source != "flatpak" && a.package_name == *rpm_name
        });
        if already_exists {
            if let Some(nat) = apps.values_mut().find(|a| {
                a.source != "flatpak" && a.package_name == *rpm_name && a.pkg_name_guessed
            }) {
                nat.pkg_name_guessed = false;
            }
            continue;
        }

        // Build the native stub from Flatpak metadata
        let mut stub = fp_entry.clone();
        stub.source = "native".to_string();
        stub.package_name = rpm_name.clone();
        stub.id = rpm_name.clone();
        stub.pkg_name_guessed = false;
        stubs.push((format!("native:{}", rpm_name), stub));
    }

    for (key, stub) in stubs {
        apps.insert(key, stub);
    }
    for (old_key, new_key) in rekeys {
        if let Some(fp) = apps.remove(&old_key) {
            apps.insert(new_key, fp);
        }
    }
}

fn apply_overrides(apps: &mut HashMap<String, AppInfo>) {
    let path = Path::new(OVERRIDES_PATH);
    if !path.exists() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else { return };
    let Ok(overrides): Result<HashMap<String, serde_json::Value>, _> =
        serde_json::from_str(&content)
    else {
        return;
    };

    for (id, patch) in overrides {
        // Skip comment keys
        if id.starts_with('_') {
            continue;
        }
        // Match both plain id and flatpak: prefixed key
        for key in [id.clone(), format!("flatpak:{}", id)] {
            if let Some(app) = apps.get_mut(&key) {
                if let Some(name) = patch.get("name").and_then(|v| v.as_str()) {
                    app.name = name.to_string();
                }
                if let Some(summary) = patch.get("summary").and_then(|v| v.as_str()) {
                    app.summary = summary.to_string();
                }
                if let Some(icon) = patch.get("icon").and_then(|v| v.as_str()) {
                    app.icon = icon.to_string();
                }
                if let Some(icon_url) = patch.get("icon_url").and_then(|v| v.as_str()) {
                    app.icon_url = icon_url.to_string();
                }
                if let Some(src) = patch.get("source").and_then(|v| v.as_str()) {
                    app.source = src.to_string();
                }
                if let Some(pkg) = patch.get("package_name").and_then(|v| v.as_str()) {
                    app.package_name = pkg.to_string();
                }
                apply_url_overrides(app, &patch);
                if let Some(cats) = patch.get("categories").and_then(|v| v.as_array()) {
                    app.categories = cats
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
            } else if key == id {
                // Create a new entry from override if not already present
                let mut app = AppInfo {
                    id: id.clone(),
                    ..Default::default()
                };
                if let Some(name) = patch.get("name").and_then(|v| v.as_str()) {
                    app.name = name.to_string();
                }
                if let Some(summary) = patch.get("summary").and_then(|v| v.as_str()) {
                    app.summary = summary.to_string();
                }
                if let Some(src) = patch.get("source").and_then(|v| v.as_str()) {
                    app.source = src.to_string();
                }
                if let Some(pkg) = patch.get("package_name").and_then(|v| v.as_str()) {
                    app.package_name = pkg.to_string();
                }
                apply_url_overrides(&mut app, &patch);
                if let Some(cats) = patch.get("categories").and_then(|v| v.as_array()) {
                    app.categories = cats
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if !app.source.is_empty() || !app.name.is_empty() {
                    apps.insert(id.clone(), app);
                }
            }
        }
    }
}

fn apply_url_overrides(app: &mut AppInfo, patch: &serde_json::Value) {
    if let Some(url) = patch.get("url_homepage").and_then(|v| v.as_str()) {
        app.url_homepage = url.to_string();
    }
    if let Some(url) = patch.get("url_bugtracker").and_then(|v| v.as_str()) {
        app.url_bugtracker = url.to_string();
    }
    if let Some(url) = patch.get("url_donation").and_then(|v| v.as_str()) {
        app.url_donation = url.to_string();
    }
    if let Some(url) = patch.get("url_help").and_then(|v| v.as_str()) {
        app.url_help = url.to_string();
    }
    if let Some(url) = patch.get("url_faq").and_then(|v| v.as_str()) {
        app.url_faq = url.to_string();
    }
    if let Some(url) = patch.get("url_vcs_browser").and_then(|v| v.as_str()) {
        app.url_vcs_browser = url.to_string();
    }
    if let Some(url) = patch.get("url_contribute").and_then(|v| v.as_str()) {
        app.url_contribute = url.to_string();
    }
}
