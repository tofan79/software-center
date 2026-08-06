// software-center-appimages — AppImage management
// Mirrors src/backend/appimages.py

use anyhow::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

// ── Paths ─────────────────────────────────────────────────────────────────────

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()))
}

fn install_dir() -> PathBuf { home().join(".local/share/software-center/appimages") }
fn icon_dir()    -> PathBuf { home().join(".local/share/software-center/appimages/icons") }
fn desktop_dir() -> PathBuf { home().join(".local/share/applications") }
fn preview_icon_dir() -> PathBuf { home().join(".cache/software-center/local-icons") }
fn extract_tmp() -> PathBuf {
    std::env::temp_dir().join("sc-appimage-extract")
}

const DESKTOP_PREFIX: &str = "sc-appimage-";
const GITHUB_API: &str = "https://api.github.com/repos/{owner}/{repo}/releases/latest";
const GITLAB_API: &str = "https://gitlab.com/api/v4/projects/{project}/releases";

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppImage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub description: String,
    pub installed_path: String,
    pub desktop_path: String,
    pub icon_path: String,
    pub categories: Vec<String>,
    /// "github" | "gitlab" | "url" | "none"
    pub update_source: String,
    pub update_url: String,
    pub update_pattern: String,
    pub installed_at: String,
    pub last_checked: String,
    pub source: String,
    pub installed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    pub id: String,
    pub name: String,
    pub current_version: String,
    pub new_version: String,
    pub download_url: String,
    pub icon_path: String,
}

/// Info extracted from an AppImage binary (for display / install confirmation).
#[derive(Debug, Clone, Default)]
pub struct AppImageInfo {
    pub name: String,
    pub version: String,
    pub summary: String,
    pub description: String,
    pub icon_data: Option<Vec<u8>>,
    pub icon_ext: String,
    pub categories: Vec<String>,
    pub update_source: String,
    pub update_url: String,
    pub update_pattern: String,
}

// ── Archive detection ─────────────────────────────────────────────────────────

const ARCHIVE_SUFFIXES: &[&str] = &[
    ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz", ".txz",
    ".tar.zst", ".tzst", ".tar.lz", ".tar.lzma",
    ".zip", ".7z", ".tar",
];

pub fn is_archive(path: &str) -> bool {
    let lower = path.to_lowercase();
    ARCHIVE_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

/// Extract an archive and return path to the AppImage inside.
/// Caller must clean up the returned file's parent temp dir when done.
fn find_appimage_in_archive(archive_path: &std::path::Path) -> Result<PathBuf> {
    let stem = archive_path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("arc");
    let tmp_dir = extract_tmp().join(format!("arc-{}", stem));
    if tmp_dir.exists() {
        std::fs::remove_dir_all(&tmp_dir)?;
    }
    std::fs::create_dir_all(&tmp_dir)?;

    let name = archive_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    if name.ends_with(".zip") {
        let status = Command::new("unzip")
            .args(["-q", &archive_path.to_string_lossy(), "-d", &tmp_dir.to_string_lossy()])
            .status()?;
        if !status.success() {
            return Err(anyhow::anyhow!("unzip failed"));
        }
    } else if name.ends_with(".7z") {
        let status = Command::new("7z")
            .args(["x", &archive_path.to_string_lossy(), &format!("-o{}", tmp_dir.display())])
            .status()?;
        if !status.success() {
            return Err(anyhow::anyhow!("7z failed"));
        }
    } else {
        // tar handles .tar.gz, .tgz, .tar.bz2, .tar.xz, .tar.zst, .tar etc.
        let status = Command::new("tar")
            .args(["xf", &archive_path.to_string_lossy(), "-C", &tmp_dir.to_string_lossy()])
            .status()?;
        if !status.success() {
            return Err(anyhow::anyhow!("tar failed"));
        }
    }

    // Find AppImage files — pick the largest
    let found = find_appimages_in_dir(&tmp_dir);
    if found.is_empty() {
        return Err(anyhow::anyhow!("No AppImage found inside {}", archive_path.display()));
    }
    let best = found.into_iter()
        .max_by_key(|p| p.metadata().map(|m| m.len()).unwrap_or(0))
        .unwrap();
    Ok(best)
}

fn find_appimages_in_dir(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = walkdir(dir) {
        for path in entries {
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            if name.ends_with(".appimage") {
                found.push(path);
            }
        }
    }
    found
}

fn walkdir(dir: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    fn walk(path: &std::path::Path, results: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, results);
                } else {
                    results.push(p);
                }
            }
        }
    }
    walk(dir, &mut results);
    Ok(results)
}

// ── AppImage metadata extraction ──────────────────────────────────────────────

/// Extract metadata from an AppImage file by running --appimage-extract.
pub fn extract_appimage_info(path: &str) -> AppImageInfo {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return AppImageInfo::default();
    }

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("appimage");
    let extract_dir = extract_tmp().join(stem);

    // Clean previous extraction
    if extract_dir.exists() {
        let _ = std::fs::remove_dir_all(&extract_dir);
    }
    let _ = std::fs::create_dir_all(&extract_dir);

    // Make executable
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o111);
        let _ = std::fs::set_permissions(path, perms);
    }

    // Run --appimage-extract with a 30-second timeout to avoid hanging on
    // AppImages that require FUSE or don't support extraction mode.
    let result = Command::new("timeout")
        .args(["30", &path.to_string_lossy(), "--appimage-extract"])
        .current_dir(&extract_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let squashfs = extract_dir.join("squashfs-root");
    let root = if squashfs.exists() { squashfs } else { extract_dir.clone() };

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let info = match result {
        Ok(_) => parse_extracted(&root, filename),
        Err(_) => AppImageInfo {
            name: stem.to_string(),
            icon_ext: "png".to_string(),
            ..Default::default()
        },
    };

    // Cleanup
    let _ = std::fs::remove_dir_all(&extract_dir);
    info
}

fn parse_extracted(squashfs: &std::path::Path, filename: &str) -> AppImageInfo {
    let mut info = AppImageInfo {
        icon_ext: "png".to_string(),
        ..Default::default()
    };

    // Find .desktop file
    let mut desktop_files: Vec<PathBuf> = squashfs.read_dir()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("desktop"))
        .collect();

    if desktop_files.is_empty() {
        desktop_files = find_files_by_ext(squashfs, "desktop");
    }

    let mut desktop_data = std::collections::HashMap::new();
    if let Some(dfile) = desktop_files.first() {
        desktop_data = parse_desktop_file(dfile);
    }

    let stem = std::path::Path::new(filename).file_stem()
        .and_then(|s| s.to_str()).unwrap_or(filename);
    info.name = desktop_data.get("Name").cloned().unwrap_or_else(|| stem.to_string());
    info.summary = desktop_data.get("Comment").cloned().unwrap_or_default();
    info.description = info.summary.clone();
    info.version = desktop_data.get("X-AppImage-Version").cloned().unwrap_or_default();
    let cats_str = desktop_data.get("Categories").cloned().unwrap_or_default();
    info.categories = cats_str.split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Parse update info
    if let Some(upd) = desktop_data.get("X-AppImage-UpdateInformation") {
        let parsed = parse_update_info(upd);
        info.update_source = parsed.0;
        info.update_url = parsed.1;
        info.update_pattern = parsed.2;
    }

    // Find icon
    let icon_name = desktop_data.get("Icon").cloned().unwrap_or_default();
    let (icon_data, icon_ext) = find_icon(squashfs, &icon_name);
    info.icon_data = icon_data;
    info.icon_ext = icon_ext;

    info
}

fn parse_desktop_file(path: &std::path::Path) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else { return result };
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_entry = true;
            continue;
        }
        if line.starts_with('[') && in_entry {
            break;
        }
        if in_entry && line.contains('=') && !line.starts_with('#') {
            if let Some((key, val)) = line.split_once('=') {
                result.insert(key.trim().to_string(), val.trim().to_string());
            }
        }
    }
    result
}

fn find_icon(squashfs: &std::path::Path, icon_name: &str) -> (Option<Vec<u8>>, String) {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // .DirIcon is the standard AppImage icon
    let diricon = squashfs.join(".DirIcon");
    if diricon.exists() {
        candidates.push(diricon);
    }

    // Named icon in root
    if !icon_name.is_empty() {
        for ext in &["png", "svg", "xpm"] {
            let p = squashfs.join(format!("{}.{}", icon_name, ext));
            if p.exists() {
                candidates.push(p);
            }
        }
    }

    // Search usr/share/icons
    if !icon_name.is_empty() {
        let icons_dir = squashfs.join("usr/share/icons");
        if icons_dir.exists() {
            for p in find_files_matching(&icons_dir, icon_name) {
                candidates.push(p);
            }
        }
        let pixmaps_dir = squashfs.join("usr/share/pixmaps");
        if pixmaps_dir.exists() {
            for p in find_files_matching(&pixmaps_dir, icon_name) {
                candidates.push(p);
            }
        }
    }

    // Fallback: any png in root
    if candidates.is_empty() {
        candidates = squashfs.read_dir()
            .into_iter().flatten().flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
            .take(1)
            .collect();
    }

    for candidate in &candidates {
        if let Ok(data) = std::fs::read(candidate) {
            let fallback_ext = candidate.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_string();
            let ext = icon_extension_from_data(&data, &fallback_ext);
            return (Some(data), ext);
        }
    }
    (None, "png".to_string())
}

fn icon_extension_from_data(data: &[u8], fallback: &str) -> String {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png".to_string()
    } else if data.starts_with(b"\xff\xd8\xff") {
        "jpg".to_string()
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        "webp".to_string()
    } else {
        let sniff_len = data.len().min(512);
        let sniff = String::from_utf8_lossy(&data[..sniff_len]).to_lowercase();
        if sniff.contains("<svg") || sniff.contains("<?xml") {
            "svg".to_string()
        } else if sniff.contains("xpm") {
            "xpm".to_string()
        } else {
            fallback.to_string()
        }
    }
}

fn find_files_by_ext(dir: &std::path::Path, ext: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    fn walk(path: &std::path::Path, ext: &str, found: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, ext, found);
                } else if p.extension().and_then(|e| e.to_str()) == Some(ext) {
                    found.push(p);
                }
            }
        }
    }
    walk(dir, ext, &mut found);
    found
}

fn find_files_matching(dir: &std::path::Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    fn walk(path: &std::path::Path, name: &str, found: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, name, found);
                } else if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    if stem == name {
                        found.push(p);
                    }
                }
            }
        }
    }
    walk(dir, name, &mut found);
    found
}

/// Parse X-AppImage-UpdateInformation field.
/// Returns (update_source, update_url, update_pattern).
fn parse_update_info(update_info: &str) -> (String, String, String) {
    let none = ("none".to_string(), String::new(), String::new());
    if update_info.is_empty() || update_info.to_lowercase() == "none" {
        return none;
    }
    let parts: Vec<&str> = update_info.split('|').collect();
    if parts[0].starts_with("gh-releases") {
        // gh-releases-zsync|owner|repo|tag|pattern
        if parts.len() >= 4 {
            let owner = parts[1];
            let repo = parts[2];
            let pattern = parts.get(4)
                .map(|p| p.replace(".zsync", ""))
                .unwrap_or_else(|| "*.AppImage".to_string());
            let url = GITHUB_API
                .replace("{owner}", owner)
                .replace("{repo}", repo);
            return ("github".to_string(), url, pattern);
        }
    } else if parts[0].starts_with("gl-releases") {
        // gl-releases-zsync|namespace/repo|tag|pattern
        if parts.len() >= 3 {
            let project = urlencoded(parts[1]);
            let pattern = parts.get(3)
                .map(|p| p.replace(".zsync", ""))
                .unwrap_or_else(|| "*.AppImage".to_string());
            let url = GITLAB_API.replace("{project}", &project);
            return ("gitlab".to_string(), url, pattern);
        }
    } else if parts[0] == "zsync" && parts.len() >= 2 {
        let url = parts[1].replace(".zsync", "");
        let pattern = std::path::Path::new(parts[1])
            .file_name().and_then(|n| n.to_str())
            .unwrap_or("*.AppImage")
            .replace(".zsync", "");
        return ("url".to_string(), url, pattern);
    }
    none
}

fn urlencoded(s: &str) -> String {
    s.chars().map(|c| match c {
        '/' => "%2F".to_string(),
        _ => c.to_string(),
    }).collect()
}

// ── Install / Uninstall ───────────────────────────────────────────────────────

/// Return all installed AppImages by reading sidecar JSON files.
pub fn get_installed() -> Vec<AppImage> {
    let dir = install_dir();
    if !dir.exists() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut apps: Vec<AppImage> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| {
            std::fs::read_to_string(e.path())
                .ok()
                .and_then(|s| serde_json::from_str::<AppImage>(&s).ok())
        })
        .map(|mut a| { a.installed = true; a.source = "appimage".to_string(); a })
        .collect();
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// Check if an AppImage is installed by id.
pub fn is_installed(id: &str) -> bool {
    install_dir().join(format!("{}.json", id)).exists()
}

/// Install an AppImage from a local file path (also accepts archives).
/// Returns (success, message, installed AppImage).
pub fn install_appimage(src_path: &str) -> (bool, String, Option<AppImage>) {
    let src = std::path::Path::new(src_path);
    if !src.exists() {
        return (false, format!("File not found: {}", src_path), None);
    }

    // If archive, extract AppImage from it first
    let arc_tmp: Option<PathBuf>;
    let actual_src: PathBuf;
    if is_archive(src_path) {
        match find_appimage_in_archive(src) {
            Ok(extracted) => {
                arc_tmp = Some(extracted.parent().unwrap().to_path_buf());
                actual_src = extracted;
            }
            Err(e) => return (false, format!("Could not extract AppImage from archive: {}", e), None),
        }
    } else {
        arc_tmp = None;
        actual_src = src.to_path_buf();
    }

    let result = do_install(&actual_src);

    // Cleanup archive temp dir
    if let Some(tmp) = arc_tmp {
        let _ = std::fs::remove_dir_all(&tmp);
    }

    result
}

fn do_install(src: &std::path::Path) -> (bool, String, Option<AppImage>) {
    // Ensure directories
    for dir in &[install_dir(), icon_dir(), desktop_dir()] {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return (false, format!("Failed to create directory: {}", e), None);
        }
    }

    let info = extract_appimage_info(&src.to_string_lossy());
    let name = if info.name.is_empty() {
        src.file_stem().and_then(|s| s.to_str()).unwrap_or("app").to_string()
    } else {
        info.name.clone()
    };

    // Sanitize id
    let app_id = sanitize_id(&name);

    // Copy AppImage to install dir
    let dest_name = format!("{}.AppImage", app_id);
    let dest_path = install_dir().join(&dest_name);
    if let Err(e) = std::fs::copy(src, &dest_path) {
        return (false, format!("Failed to copy AppImage: {}", e), None);
    }

    // Make executable
    if let Ok(meta) = std::fs::metadata(&dest_path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o111);
        let _ = std::fs::set_permissions(&dest_path, perms);
    }

    // Save icon
    let icon_path_str = if let Some(icon_data) = &info.icon_data {
        let ext = if info.icon_ext.is_empty() { "png" } else { &info.icon_ext };
        let icon_path = icon_dir().join(format!("{}.{}", app_id, ext));
        if std::fs::write(&icon_path, icon_data).is_ok() {
            icon_path.to_string_lossy().to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Write .desktop file
    let desktop_path = write_desktop(&app_id, &name, &dest_path.to_string_lossy(), &icon_path_str, &info);

    // Write sidecar JSON
    let now = chrono_now();
    let app = AppImage {
        id: app_id.clone(),
        name: name.clone(),
        version: info.version.clone(),
        summary: info.summary.clone(),
        description: info.description.clone(),
        installed_path: dest_path.to_string_lossy().to_string(),
        desktop_path,
        icon_path: icon_path_str,
        categories: info.categories.clone(),
        update_source: info.update_source.clone(),
        update_url: info.update_url.clone(),
        update_pattern: info.update_pattern.clone(),
        installed_at: now,
        last_checked: String::new(),
        source: "appimage".to_string(),
        installed: true,
    };

    let sidecar_path = install_dir().join(format!("{}.json", app_id));
    if let Ok(json) = serde_json::to_string_pretty(&app) {
        let _ = std::fs::write(&sidecar_path, json);
    }

    let _ = Command::new("update-desktop-database").arg(desktop_dir()).output();

    (true, format!("{} installed successfully.", name), Some(app))
}

/// Uninstall an AppImage — removes binary, icon, desktop file, and sidecar.
pub fn uninstall(id: &str) -> (bool, String) {
    let sidecar_path = install_dir().join(format!("{}.json", id));
    let Ok(content) = std::fs::read_to_string(&sidecar_path) else {
        return (false, format!("AppImage '{}' not found.", id));
    };
    let Ok(app) = serde_json::from_str::<AppImage>(&content) else {
        return (false, "Failed to read sidecar.".to_string());
    };

    if !app.installed_path.is_empty() {
        let _ = std::fs::remove_file(&app.installed_path);
    }
    if !app.icon_path.is_empty() {
        let _ = std::fs::remove_file(&app.icon_path);
    }
    let desktop = desktop_dir().join(format!("{}{}.desktop", DESKTOP_PREFIX, id));
    let _ = std::fs::remove_file(&desktop);
    let _ = std::fs::remove_file(&sidecar_path);
    let _ = Command::new("update-desktop-database").arg(desktop_dir()).output();

    (true, format!("{} uninstalled.", app.name))
}

/// Save update settings for an installed AppImage (update_source, update_url, update_pattern).
pub fn save_settings(id: &str, update_source: &str, update_url: &str, update_pattern: &str) -> (bool, String) {
    let sidecar_path = install_dir().join(format!("{}.json", id));
    let Ok(content) = std::fs::read_to_string(&sidecar_path) else {
        return (false, format!("AppImage '{}' not found.", id));
    };
    let Ok(mut app) = serde_json::from_str::<AppImage>(&content) else {
        return (false, "Failed to read sidecar.".to_string());
    };
    app.update_source  = update_source.to_string();
    app.update_url     = update_url.to_string();
    app.update_pattern = update_pattern.to_string();
    match serde_json::to_string_pretty(&app) {
        Ok(json) => match std::fs::write(&sidecar_path, json) {
            Ok(_)  => (true,  "Settings saved.".to_string()),
            Err(e) => (false, format!("Write error: {}", e)),
        },
        Err(e) => (false, format!("Serialize error: {}", e)),
    }
}

fn write_desktop(app_id: &str, name: &str, exec_path: &str, icon_path: &str, info: &AppImageInfo) -> String {
    let cats = if info.categories.is_empty() {
        "Utility;".to_string()
    } else {
        info.categories.join(";") + ";"
    };
    let icon = if icon_path.is_empty() { "application-x-executable" } else { icon_path };
    let content = format!(
        "[Desktop Entry]\nName={}\nComment={}\nExec=\"{}\" %U\nIcon={}\nTerminal=false\nType=Application\nCategories={}\nStartupNotify=true\nX-SoftwareCenter-AppImage=true\nX-SoftwareCenter-AppImage-ID={}\n",
        name, info.summary, exec_path, icon, cats, app_id
    );
    let desktop_path = desktop_dir().join(format!("{}{}.desktop", DESKTOP_PREFIX, app_id));
    let _ = std::fs::write(&desktop_path, &content);
    if let Ok(meta) = std::fs::metadata(&desktop_path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = std::fs::set_permissions(&desktop_path, perms);
    }
    desktop_path.to_string_lossy().to_string()
}

// ── File info for display (file association) ──────────────────────────────────

/// Called when user opens an AppImage or archive containing one.
/// Returns info suitable for a detail/confirmation page.
pub fn get_appimage_info_for_display(path: &str) -> serde_json::Value {
    let src = std::path::Path::new(path);
    let arc_tmp: Option<PathBuf>;
    let actual: PathBuf;

    if is_archive(path) {
        match find_appimage_in_archive(src) {
            Ok(extracted) => {
                arc_tmp = Some(extracted.parent().unwrap().to_path_buf());
                actual = extracted;
            }
            Err(e) => {
                return serde_json::json!({
                    "name": src.file_stem().and_then(|s| s.to_str()).unwrap_or(""),
                    "source": "appimage",
                    "installed": false,
                    "src_path": path,
                    "_error": e.to_string(),
                });
            }
        }
    } else {
        arc_tmp = None;
        actual = src.to_path_buf();
    }

    let info = extract_appimage_info(&actual.to_string_lossy());
    let name = if info.name.is_empty() {
        actual.file_stem().and_then(|s| s.to_str()).unwrap_or("app").to_string()
    } else {
        info.name.clone()
    };
    let app_id = sanitize_id(&name);
    let icon_path = save_preview_icon(&app_id, &info);

    let result = serde_json::json!({
        "id":             app_id,
        "name":           name,
        "version":        info.version,
        "summary":        info.summary,
        "description":    info.description,
        "categories":     info.categories,
        "update_source":  info.update_source,
        "update_url":     info.update_url,
        "update_pattern": info.update_pattern,
        "source":         "appimage",
        "installed":      false,
        "src_path":       actual.to_string_lossy().as_ref(),
        "archive_path":   if arc_tmp.is_some() { path } else { "" },
        "icon_path":      icon_path,
        "icon_url":       "",
    });

    if let Some(tmp) = arc_tmp {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result
}

fn save_preview_icon(app_id: &str, info: &AppImageInfo) -> String {
    let Some(icon_data) = &info.icon_data else {
        return String::new();
    };
    let ext = if info.icon_ext.is_empty() {
        "png"
    } else {
        info.icon_ext.as_str()
    };
    let _ = std::fs::create_dir_all(preview_icon_dir());
    let icon_path = preview_icon_dir().join(format!("{app_id}.{ext}"));
    if std::fs::write(&icon_path, icon_data).is_ok() {
        icon_path.to_string_lossy().to_string()
    } else {
        String::new()
    }
}

// ── Update checking ───────────────────────────────────────────────────────────

/// Check all installed AppImages for updates (sync, calls async internally).
pub fn check_updates() -> Vec<UpdateResult> {
    let apps = get_installed();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let mut results = Vec::new();
    for app in &apps {
        if let Some(upd) = rt.block_on(check_single_update(app)) {
            // Update last_checked in sidecar
            let mut updated = app.clone();
            updated.last_checked = chrono_now();
            let sidecar = install_dir().join(format!("{}.json", app.id));
            if let Ok(json) = serde_json::to_string_pretty(&updated) {
                let _ = std::fs::write(&sidecar, json);
            }
            results.push(upd);
        }
    }
    results
}

/// Check a single AppImage for an update. Returns Some(UpdateResult) if available.
pub async fn check_update(app: &AppImage) -> Option<UpdateResult> {
    check_single_update(app).await
}

async fn check_single_update(app: &AppImage) -> Option<UpdateResult> {
    match app.update_source.as_str() {
        "github" => check_github_update(app).await,
        "gitlab" => check_gitlab_update(app).await,
        "url"    => check_url_update(app).await,
        _        => None,
    }
}

async fn check_github_update(app: &AppImage) -> Option<UpdateResult> {
    let api_url = normalize_github_url(&app.update_url);
    let resp = reqwest::Client::new()
        .get(&api_url)
        .header("User-Agent", "Software-Center/1.0")
        .header("Accept", "application/vnd.github+json")
        .send().await.ok()?
        .json::<serde_json::Value>().await.ok()?;

    let new_version = resp["tag_name"].as_str()?.trim_start_matches('v').to_string();
    if !version_newer(&new_version, &app.version) {
        return None;
    }

    let pattern = glob_to_regex(&app.update_pattern);
    let asset = resp["assets"].as_array()?.iter()
        .find(|a| a["name"].as_str().map(|n| pattern.is_match(n)).unwrap_or(false))?;

    Some(UpdateResult {
        id: app.id.clone(),
        name: app.name.clone(),
        current_version: app.version.clone(),
        new_version,
        download_url: asset["browser_download_url"].as_str().unwrap_or("").to_string(),
        icon_path: app.icon_path.clone(),
    })
}

async fn check_gitlab_update(app: &AppImage) -> Option<UpdateResult> {
    let api_url = normalize_gitlab_url(&app.update_url);
    let releases = reqwest::Client::new()
        .get(&api_url)
        .header("User-Agent", "Software-Center/1.0")
        .send().await.ok()?
        .json::<serde_json::Value>().await.ok()?;

    let latest = releases.as_array()?.first()?;
    let new_version = latest["tag_name"].as_str()?.trim_start_matches('v').to_string();
    if !version_newer(&new_version, &app.version) {
        return None;
    }

    let pattern = glob_to_regex(&app.update_pattern);
    let links = latest["assets"]["links"].as_array()?;
    let asset = links.iter().find(|a| {
        let url = a["url"].as_str().unwrap_or("");
        let fname = url.rsplit('/').next().unwrap_or("");
        pattern.is_match(fname)
    })?;

    Some(UpdateResult {
        id: app.id.clone(),
        name: app.name.clone(),
        current_version: app.version.clone(),
        new_version,
        download_url: asset["url"].as_str().unwrap_or("").to_string(),
        icon_path: app.icon_path.clone(),
    })
}

async fn check_url_update(app: &AppImage) -> Option<UpdateResult> {
    let client = reqwest::Client::new();
    let resp = client.head(&app.update_url)
        .header("User-Agent", "Software-Center/1.0")
        .send().await.ok()?;

    let etag = resp.headers().get("etag")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let last_mod = resp.headers().get("last-modified")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Read stored values from sidecar
    let sidecar_path = install_dir().join(format!("{}.json", app.id));
    let sidecar: serde_json::Value = std::fs::read_to_string(&sidecar_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let stored_etag = sidecar["_url_etag"].as_str().unwrap_or("").to_string();
    let stored_mod  = sidecar["_url_last_modified"].as_str().unwrap_or("").to_string();

    let has_update = (!etag.is_empty() && etag != stored_etag)
        || (!last_mod.is_empty() && last_mod != stored_mod);

    if has_update {
        // Update sidecar with new values
        let mut updated: serde_json::Map<String, serde_json::Value> = sidecar
            .as_object()
            .cloned()
            .unwrap_or_default();
        if !etag.is_empty() {
            updated.insert("_url_etag".to_string(), serde_json::Value::String(etag));
        }
        if !last_mod.is_empty() {
            updated.insert("_url_last_modified".to_string(), serde_json::Value::String(last_mod));
        }
        if let Ok(json) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&sidecar_path, json);
        }
        return Some(UpdateResult {
            id: app.id.clone(),
            name: app.name.clone(),
            current_version: app.version.clone(),
            new_version: "new version".to_string(),
            download_url: app.update_url.clone(),
            icon_path: app.icon_path.clone(),
        });
    }
    None
}

// ── Update install stream ─────────────────────────────────────────────────────

/// Download and install an AppImage update. Yields status lines, last is "__done__<code>".
pub fn update_appimage_stream(app_id: &str, download_url: &str) -> impl Iterator<Item = String> {
    let app_id = app_id.to_string();
    let download_url = download_url.to_string();

    let sidecar_path = install_dir().join(format!("{}.json", app_id));
    let Ok(content) = std::fs::read_to_string(&sidecar_path) else {
        return vec![format!("Error: {} not installed.", app_id), "__done__1".to_string()].into_iter();
    };
    let Ok(mut app) = serde_json::from_str::<AppImage>(&content) else {
        return vec!["Error: Failed to read sidecar.".to_string(), "__done__1".to_string()].into_iter();
    };

    let name = app.name.clone();
    let old_path = app.installed_path.clone();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    let lines = rt.block_on(async {
        let mut out: Vec<String> = Vec::new();
        out.push(format!("Downloading update for {}...", name));

        let tmp_path = install_dir().join(format!("{}.AppImage.tmp", app_id));
        let client = reqwest::Client::new();
        let resp = match client.get(&download_url)
            .header("User-Agent", "Software-Center/1.0")
            .send().await
        {
            Ok(r) => r,
            Err(e) => {
                out.push(format!("Error: {}", e));
                out.push("__done__1".to_string());
                return out;
            }
        };

        let total = resp.content_length().unwrap_or(0);
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                out.push(format!("Error: {}", e));
                out.push("__done__1".to_string());
                return out;
            }
        };
        if total > 0 {
            out.push("DOWNLOAD:100".to_string());
        }
        out.push("Download complete.".to_string());

        if let Err(e) = std::fs::write(&tmp_path, &bytes) {
            out.push(format!("Error writing file: {}", e));
            out.push("__done__1".to_string());
            return out;
        }

        // Replace old AppImage
        if !old_path.is_empty() {
            let _ = std::fs::remove_file(&old_path);
        }
        let new_path = install_dir().join(format!("{}.AppImage", app_id));
        if let Err(e) = std::fs::rename(&tmp_path, &new_path) {
            out.push(format!("Error: {}", e));
            out.push("__done__1".to_string());
            return out;
        }
        if let Ok(meta) = std::fs::metadata(&new_path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = std::fs::set_permissions(&new_path, perms);
        }

        out.push("Extracting updated metadata...".to_string());
        let info = extract_appimage_info(&new_path.to_string_lossy());
        let new_version = if info.version.is_empty() { app.version.clone() } else { info.version.clone() };

        app.version = new_version.clone();
        app.installed_path = new_path.to_string_lossy().to_string();
        app.last_checked = chrono_now();

        if let Some(icon_data) = &info.icon_data {
            let ext = if info.icon_ext.is_empty() { "png" } else { &info.icon_ext };
            let icon_path = icon_dir().join(format!("{}.{}", app_id, ext));
            let _ = std::fs::write(&icon_path, icon_data);
        }

        if let Ok(json) = serde_json::to_string_pretty(&app) {
            let _ = std::fs::write(&sidecar_path, json);
        }

        out.push(format!("{} updated to {}.", name, new_version));
        out.push("__done__0".to_string());
        out
    });

    lines.into_iter()
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn sanitize_id(name: &str) -> String {
    let id: String = name.to_lowercase().chars().map(|c| {
        if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }
    }).collect();
    id.trim_matches('-').to_string()
}

fn normalize_github_url(url: &str) -> String {
    if url.contains("api.github.com") {
        return url.to_string();
    }
    if let Some(caps) = Regex::new(r"https?://github\.com/([^/]+)/([^/]+)")
        .ok()
        .and_then(|re| re.captures(url))
    {
        return GITHUB_API
            .replace("{owner}", &caps[1])
            .replace("{repo}", &caps[2]);
    }
    url.to_string()
}

fn normalize_gitlab_url(url: &str) -> String {
    if url.contains("/api/v4/") {
        return url.to_string();
    }
    if let Some(caps) = Regex::new(r"https?://gitlab\.com/([^/]+/[^/]+)")
        .ok()
        .and_then(|re| re.captures(url))
    {
        let project = urlencoded(&caps[1]);
        return GITLAB_API.replace("{project}", &project);
    }
    url.to_string()
}

fn version_newer(new: &str, current: &str) -> bool {
    if current.is_empty() { return true; }
    if new == current { return false; }
    let parse = |v: &str| -> Vec<u64> {
        Regex::new(r"\d+").ok()
            .map(|re| re.find_iter(v).filter_map(|m| m.as_str().parse().ok()).collect())
            .unwrap_or_default()
    };
    parse(new) > parse(current)
}

fn glob_to_regex(pattern: &str) -> Regex {
    let escaped = regex::escape(pattern).replace(r"\*", ".*");
    Regex::new(&format!("(?i)^{}$", escaped))
        .unwrap_or_else(|_| Regex::new(".*").unwrap())
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Simple ISO-8601-ish: YYYY-MM-DDTHH:MM:SSZ
    let s = secs;
    let sec = s % 60; let s = s / 60;
    let min = s % 60; let s = s / 60;
    let hour = s % 24; let mut days = s / 24;
    let mut year = 1970u64;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let dy = if leap { 366 } else { 365 };
        if days < dy { break; }
        days -= dy;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31u64, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u64;
    for &md in &month_days {
        if days < md { break; }
        days -= md;
        month += 1;
    }
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, days + 1, hour, min, sec)
}
