// software-center home — Home page data (popular, picks, recently updated, new apps)
// Mirrors src/backend/home.py and src/ui_qt/pages/home.py exactly.
//
// Key design: appstream is loaded ONCE by the caller and passed in, matching
// Python's _appstream_cache pattern (load once, reuse across all sections).

use anyhow::Result;
use scenter_appstream::{AppInfo, get_appstream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

// ── Constants ─────────────────────────────────────────────────────────────────

const CACHE_TTL_SECS: u64 = 6 * 3600;
const PICKS_URL: &str = "https://rakuos.org/api/picks.json";
const UPDATED_FEED: &str = "https://flathub.org/api/v2/feed/recently-updated";
const NEW_FEED: &str = "https://flathub.org/api/v2/feed/new";

// Mirrors Python's _POPULAR_SEED exactly
const POPULAR_SEED: &[&str] = &[
    "com.valvesoftware.Steam",
    "org.mozilla.firefox",
    "com.spotify.Client",
    "com.discordapp.Discord",
    "org.videolan.vlc",
    "com.obsproject.Studio",
    "org.gimp.GIMP",
    "org.inkscape.Inkscape",
    "org.libreoffice.LibreOffice",
    "org.kde.kdenlive",
    "org.blender.Blender",
    "com.google.Chrome",
    "org.signal.Signal",
    "net.lutris.Lutris",
    "com.heroicgameslauncher.hgl",
    "org.freedesktop.Piper",
    "org.gnome.Boxes",
    "io.github.celluloid_player.Celluloid",
    "com.github.tchx84.Flatseal",
    "org.kde.okular",
];

// Mirrors Python's _FALLBACK_PICKS exactly
const FALLBACK_PICKS: &[&str] = &[
    "com.valvesoftware.Steam",
    "com.obsproject.Studio",
    "org.kde.kdenlive",
    "org.gimp.GIMP",
    "net.lutris.Lutris",
    "com.heroicgameslauncher.hgl",
    "org.mozilla.firefox",
    "com.discordapp.Discord",
    "org.blender.Blender",
    "com.github.tchx84.Flatseal",
    "io.github.Faugus.faugus-launcher",
    "org.libreoffice.LibreOffice",
];

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeApp {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub icon: String,
    pub icon_url: String,
    pub icon_path: String,
    pub source: String,
}

impl From<&AppInfo> for HomeApp {
    fn from(a: &AppInfo) -> Self {
        HomeApp {
            id: a.id.clone(),
            name: a.name.clone(),
            summary: a.summary.clone(),
            icon: a.icon.clone(),
            icon_url: a.icon_url.clone(),
            icon_path: a.icon_path.clone(),
            source: a.source.clone(),
        }
    }
}

// ── Public API — takes pre-loaded appstream (call load_appstream() once) ──────

/// Load all home page data. Appstream is loaded once here and shared across
/// all sections — mirrors Python's _appstream_cache pattern exactly.
/// If all section caches are fresh, returns immediately without loading appstream.
pub async fn load_all() -> (Vec<HomeApp>, Vec<HomeApp>, Vec<HomeApp>, Vec<HomeApp>) {
    // Fast path: all caches valid — skip expensive appstream parse entirely
    let picks_c   = cache_path("picks.json");
    let popular_c = cache_path("popular.json");
    let updated_c = cache_path("recently_updated.json");
    let new_c     = cache_path("new_apps.json");

    if cache_valid(&picks_c) && cache_valid(&popular_c)
        && cache_valid(&updated_c) && cache_valid(&new_c)
    {
        if let (Some(picks), Some(popular), Some(updated), Some(new)) = (
            read_cache(&picks_c),
            read_cache(&popular_c),
            read_cache(&updated_c),
            read_cache(&new_c),
        ) {
            return (picks, popular, updated, new);
        }
    }

    // Slow path: load appstream once, then fill missing sections in parallel
    let appstream = get_appstream();

    // popular is sync (no network), compute immediately
    let popular = get_popular(&appstream);

    // picks / updated / new all do network calls — run them in parallel
    let a1 = appstream.clone();
    let a2 = appstream.clone();
    let (picks, updated, new) = tokio::join!(
        async move { get_picks(&*a1).await },
        async move { get_recently_updated(&*a2).await },
        async move { get_new_apps(&*appstream).await },
    );

    (picks, popular, updated, new)
}

/// Editor's picks — tries the upstream picks endpoint with short timeout,
/// falls back to FALLBACK_PICKS.
pub async fn get_picks(appstream: &HashMap<String, AppInfo>) -> Vec<HomeApp> {
    let cache = cache_path("picks.json");
    if cache_valid(&cache) {
        if let Some(apps) = read_cache(&cache) {
            return apps;
        }
    }

    let ids: Vec<String> = match fetch_json(PICKS_URL).await {
        Ok(v) => v
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| FALLBACK_PICKS.iter().map(|s| s.to_string()).collect()),
        Err(_) => FALLBACK_PICKS.iter().map(|s| s.to_string()).collect(),
    };

    let apps = resolve_ids(&ids, appstream, 12);
    write_cache(&cache, &apps);
    apps
}

/// Popular apps from POPULAR_SEED resolved against local AppStream (no network).
pub fn get_popular(appstream: &HashMap<String, AppInfo>) -> Vec<HomeApp> {
    let cache = cache_path("popular.json");
    if cache_valid(&cache) {
        if let Some(apps) = read_cache(&cache) {
            return apps;
        }
    }

    let ids: Vec<String> = POPULAR_SEED.iter().map(|s| s.to_string()).collect();
    let apps = resolve_ids(&ids, appstream, 20);
    write_cache(&cache, &apps);
    apps
}

/// Recently updated from Flathub RSS. Returns empty on network failure.
pub async fn get_recently_updated(appstream: &HashMap<String, AppInfo>) -> Vec<HomeApp> {
    let cache = cache_path("recently_updated.json");
    if cache_valid(&cache) {
        if let Some(apps) = read_cache(&cache) {
            return apps;
        }
    }

    let apps = fetch_feed_apps(UPDATED_FEED, appstream).await;
    write_cache(&cache, &apps);
    apps
}

/// New apps from Flathub RSS. Returns empty on network failure.
pub async fn get_new_apps(appstream: &HashMap<String, AppInfo>) -> Vec<HomeApp> {
    let cache = cache_path("new_apps.json");
    if cache_valid(&cache) {
        if let Some(apps) = read_cache(&cache) {
            return apps;
        }
    }

    let apps = fetch_feed_apps(NEW_FEED, appstream).await;
    write_cache(&cache, &apps);
    apps
}

// ── ID resolution (mirrors Python's _resolve_apps) ───────────────────────────

fn resolve_ids(ids: &[String], appstream: &HashMap<String, AppInfo>, limit: usize) -> Vec<HomeApp> {
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for id in ids {
        if result.len() >= limit {
            break;
        }
        if let Some(app) = resolve_one(id, appstream) {
            if !seen.contains(&app.id) {
                seen.insert(app.id.clone());
                result.push(HomeApp::from(app));
            }
        }
    }
    result
}

/// Case-insensitive ID lookup with .desktop fallback — mirrors Python's _resolve_apps.
fn resolve_one<'a>(id: &str, appstream: &'a HashMap<String, AppInfo>) -> Option<&'a AppInfo> {
    let clean = id.trim_end_matches(".desktop");
    let clean_lc = clean.to_lowercase();

    for candidate in &[
        clean_lc.clone(),
        format!("{}.desktop", clean_lc),
        format!("flatpak:{}", clean_lc),
        id.to_string(),
        format!("{}.desktop", id),
        format!("flatpak:{}", id),
    ] {
        if let Some(a) = appstream.get(candidate.as_str()) {
            return Some(a);
        }
    }

    // Last-resort linear scan (case-insensitive id match)
    appstream.values().find(|a| {
        a.id.trim_end_matches(".desktop").to_lowercase() == clean_lc
    })
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".cache/software-center")
}

fn cache_path(name: &str) -> PathBuf {
    cache_dir().join(name)
}

fn cache_valid(path: &PathBuf) -> bool {
    path.metadata()
        .and_then(|m| m.modified())
        .map(|t| {
            SystemTime::now()
                .duration_since(t)
                .unwrap_or(Duration::MAX)
                .as_secs()
                < CACHE_TTL_SECS
        })
        .unwrap_or(false)
}

fn read_cache(path: &PathBuf) -> Option<Vec<HomeApp>> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn write_cache(path: &PathBuf, data: &Vec<HomeApp>) {
    let _ = std::fs::create_dir_all(cache_dir());
    if let Ok(s) = serde_json::to_string(data) {
        let _ = std::fs::write(path, s);
    }
}

async fn fetch_json(url: &str) -> Result<serde_json::Value> {
    let resp = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Software-Center/1.0")
        .timeout(Duration::from_secs(3))
        .send()
        .await?
        .json()
        .await?;
    Ok(resp)
}

async fn fetch_feed_apps(url: &str, appstream: &HashMap<String, AppInfo>) -> Vec<HomeApp> {
    let Ok(resp) = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "Software-Center/1.0")
        .timeout(Duration::from_secs(8))
        .send()
        .await
    else {
        return Vec::new();
    };

    let Ok(text) = resp.text().await else {
        return Vec::new();
    };

    // Feed is served as one long line — iterate every /apps/ occurrence
    // (mirroring Python's _parse_feed which uses ElementTree)
    let mut ids: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for chunk in text.split("/apps/").skip(1) {
        // App ID ends at the first char that isn't alphanumeric / . - _
        let end = chunk
            .find(|c: char| !c.is_alphanumeric() && c != '.' && c != '-' && c != '_')
            .unwrap_or(chunk.len());
        let id = chunk[..end].trim();
        if id.contains('.') && !id.is_empty() && seen.insert(id.to_string()) {
            ids.push(id.to_string());
            if ids.len() >= 20 {
                break;
            }
        }
    }

    resolve_ids(&ids, appstream, 20)
}

/// Refresh all caches — called by tray daemon on schedule.
pub async fn refresh_all() {
    let appstream = get_appstream();
    get_picks(&appstream).await;
    get_recently_updated(&appstream).await;
    get_new_apps(&appstream).await;
    get_popular(&appstream);
}
