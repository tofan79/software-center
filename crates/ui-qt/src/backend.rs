// ui-qt/backend.rs — QObject backend exposed to QML

#![allow(non_snake_case)]

use qmetaobject::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;

// ── Cache readiness flag (set by warmCache thread) ───────────────────────────
static CACHE_READY: AtomicBool = AtomicBool::new(false);

// ── Install queue entry ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct InstallQueueEntry {
    pub app_name: String,
    pub id: String,
    pub source: String,
    pub remote: String,
    pub is_remove: bool,
    pub icon_path: String,
    pub icon_url: String,
    pub user_remote: bool,
}

// ── Shared async state ────────────────────────────────────────────────────────

#[derive(Default)]
pub struct SharedState {
    pub running:  AtomicBool,
    /// 0=idle 1=success 2=failed
    pub result:   AtomicI32,
    pub progress: AtomicI32,  // 0-100
}

fn log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center.log")
}

fn detail_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-detail.log")
}

fn addons_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-addons.log")
}

fn unused_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-unused.json")
}

fn repos_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-repos.json")
}

fn runtimes_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-runtimes.json")
}

fn remotes_log_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-remotes.json")
}

fn settings_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home).join(".config/software-center/settings.json")
}



fn show_flag_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-show")
}

fn daemon_cache_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".cache/software-center/daemon-update-cache.json")
}

fn badge_count_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-badge-count")
}

fn append_log(text: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(log_path()) {
        let _ = f.write_all(text.as_bytes());
        let _ = f.write_all(b"\n");
    }
}

/// Which scope (--system or --user) an installed flatpak app/runtime is actually in.
/// Falls back to --system if it isn't found in either (matches prior behavior).
fn flatpak_installed_scope(app_id: &str) -> &'static str {
    let in_scope = |scope: &str| {
        std::process::Command::new("flatpak")
            .args(["list", "--app", "--runtime", scope, "--columns=application"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).lines().any(|l| l == app_id))
            .unwrap_or(false)
    };
    if in_scope("--user") && !in_scope("--system") { "--user" } else { "--system" }
}

// ── Backend QObject ───────────────────────────────────────────────────────────

#[derive(QObject, Default)]
pub struct SoftwareBackend {
    base: qt_base_class!(trait QObject),

    // Navigation
    currentPage: qt_property!(i32; NOTIFY currentPageChanged),
    currentPageChanged: qt_signal!(),

    // Operation state (for showing progress/log area)
    opRunning: qt_property!(bool; NOTIFY opStateChanged),
    opResult:  qt_property!(i32;  NOTIFY opStateChanged),   // 0=idle 1=ok 2=fail
    opProgress: qt_property!(i32; NOTIFY opStateChanged),   // 0-100
    logRevision: qt_property!(i32; NOTIFY logRevisionChanged),
    logRevisionChanged: qt_signal!(),
    opStateChanged: qt_signal!(),

    // Search
    searchQuery: qt_property!(QString; NOTIFY searchQueryChanged),
    searchQueryChanged: qt_signal!(),

    // Cached JSON strings — UI reads these as JS objects via JSON.parse
    homeDataJson:      qt_property!(QString; NOTIFY homeDataChanged),
    homeDataChanged:   qt_signal!(),
    installedJson:     qt_property!(QString; NOTIFY installedChanged),
    installedChanged:  qt_signal!(),
    updatesJson:       qt_property!(QString; NOTIFY updatesChanged),
    updatesChanged:    qt_signal!(),
    searchResultsJson: qt_property!(QString; NOTIFY searchResultsChanged),
    searchResultsChanged: qt_signal!(),
    systemStatusJson:  qt_property!(QString; NOTIFY systemStatusChanged),
    systemStatusChanged: qt_signal!(),
    settingsJson:      qt_property!(QString; NOTIFY settingsChanged),
    settingsChanged:   qt_signal!(),

    // Background update badge count (populated by daemon cache or startup check)
    pendingUpdateCount: qt_property!(i32; NOTIFY pendingUpdateCountChanged),
    pendingUpdateCountChanged: qt_signal!(),

    // Detail-fetch state — separate from install op so they don't interfere.
    detailReady:   qt_property!(bool; NOTIFY detailStateChanged),
    detailStateChanged: qt_signal!(),
    detail_shared: Option<Arc<AtomicBool>>,

    // Addons-fetch state — separate channel, same rationale.
    addonsReady:   qt_property!(bool; NOTIFY addonsStateChanged),
    addonsStateChanged: qt_signal!(),
    addons_shared: Option<Arc<AtomicBool>>,

    // Unused-packages count (Settings → Maintenance) — runs dnf5, off UI thread.
    unusedReady:   qt_property!(bool; NOTIFY unusedStateChanged),
    unusedStateChanged: qt_signal!(),
    unused_shared: Option<Arc<AtomicBool>>,

    // DNF repo list (Settings → Repositories) — runs dnf5, off UI thread.
    reposReady:   qt_property!(bool; NOTIFY reposStateChanged),
    reposStateChanged: qt_signal!(),
    repos_shared: Option<Arc<AtomicBool>>,

    // Installed flatpak runtimes (Installed page) — runs flatpak, off UI thread.
    runtimesReady:   qt_property!(bool; NOTIFY runtimesStateChanged),
    runtimesStateChanged: qt_signal!(),
    runtimes_shared: Option<Arc<AtomicBool>>,

    // Flatpak remotes (Settings → Flatpak Repositories) — runs flatpak, off UI thread.
    remotesReady:   qt_property!(bool; NOTIFY remotesStateChanged),
    remotesStateChanged: qt_signal!(),
    remotes_shared: Option<Arc<AtomicBool>>,

    // Install queue state (banner at window bottom)
    queueCount:           qt_property!(i32;     NOTIFY queueStateChanged),
    queueActiveName:      qt_property!(QString; NOTIFY queueStateChanged),
    queueActiveIconPath:  qt_property!(QString; NOTIFY queueStateChanged),
    queueActiveIconUrl:   qt_property!(QString; NOTIFY queueStateChanged),
    queueIsRemove:        qt_property!(bool;    NOTIFY queueStateChanged),
    queueStateChanged: qt_signal!(),

    // Shared state between Rust threads and Qt
    shared: Option<Arc<SharedState>>,

    // Install/remove queue — main thread only, no Arc needed.
    // NOT a qt_property; managed entirely in Rust.
    install_queue: std::collections::VecDeque<InstallQueueEntry>,
    // Tracks previous opRunning so pollOp can detect the idle transition.
    prev_op_running: bool,
    // True when the currently-running op is an install/remove (vs home load etc).
    op_is_install_remove: bool,

    // ── Navigation ────────────────────────────────────────────────────────────

    navigate: qt_method!(fn navigate(&mut self, page: i32) {
        self.currentPage = page;
        self.currentPageChanged();
    }),

    pollOp: qt_method!(fn pollOp(&mut self) {
        let s = self.get_shared();
        let running  = s.running.load(Ordering::Relaxed);
        let result   = s.result.load(Ordering::Relaxed);
        let progress = s.progress.load(Ordering::Relaxed);

        let changed = self.opRunning != running
            || self.opResult != result
            || self.opProgress != progress;

        if changed {
            self.opRunning  = running;
            self.opResult   = result;
            self.opProgress = progress;
            self.opStateChanged();
        }

        // Detect install/remove completion → dequeue next.
        if self.prev_op_running && !running && self.op_is_install_remove {
            self.op_is_install_remove = false;
            self.dequeue_next_installop();
        }
        self.prev_op_running = running;

        // Poll log file revision
        if let Ok(m) = std::fs::metadata(log_path()) {
            let rev = m.len() as i32;
            if self.logRevision != rev {
                self.logRevision = rev;
                self.logRevisionChanged();
            }
        }
    }),

    pollDetail: qt_method!(fn pollDetail(&mut self) {
        if let Some(ready) = &self.detail_shared {
            if ready.load(Ordering::Relaxed) {
                self.detail_shared = None;
                self.detailReady = true;
                self.detailStateChanged();
            }
        }
    }),

    readDetail: qt_method!(fn readDetail(&mut self) -> QString {
        std::fs::read_to_string(detail_log_path())
            .unwrap_or_default()
            .into()
    }),

    pollQueue: qt_method!(fn pollQueue(&mut self) {
        // QML calls this on a slower timer to refresh the banner properties.
        // Nothing to do — properties are already updated synchronously when
        // installApp/removeApp enqueue or dequeue_next_installop fires.
        let _ = self;
    }),

    readLog: qt_method!(fn readLog(&mut self) -> QString {
        std::fs::read_to_string(log_path())
            .unwrap_or_default()
            .into()
    }),

    // ── Home page ─────────────────────────────────────────────────────────────

    loadHome: qt_method!(fn loadHome(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result = rt.block_on(async {
                let (picks, popular, updated, new) = scenter_home::load_all().await;
                let mut json = serde_json::json!({
                    "picks":   picks,
                    "popular": popular,
                    "updated": updated,
                    "new":     new,
                });
                // Enrich each card with a live installed flag so the Home page can
                // show an "Installed" badge (fresh status, not the cached one).
                let installed_rpm = scenter_packages::get_installed_packages().unwrap_or_default();
                let installed_fp  = scenter_packages::get_installed_flatpaks();
                let desktops      = scenter_packages::get_installed_desktops();
                let mark_installed = |arr: &mut Vec<serde_json::Value>| {
                    for it in arr.iter_mut() {
                        let src  = it.get("source").and_then(|v| v.as_str()).unwrap_or("");
                        let id   = it.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let bare = id.strip_suffix(".desktop").unwrap_or(id);
                        let pkg  = it.get("package_name").and_then(|v| v.as_str()).unwrap_or("");
                        let installed = if src == "flatpak" {
                            installed_fp.contains(bare)
                        } else {
                            (!pkg.is_empty() && installed_rpm.contains(pkg))
                                || desktops.contains(bare)
                        };
                        it["installed"] = serde_json::Value::Bool(installed);
                    }
                };
                if let Some(arr) = json["picks"].as_array_mut()   { mark_installed(arr); }
                if let Some(arr) = json["popular"].as_array_mut() { mark_installed(arr); }
                if let Some(arr) = json["updated"].as_array_mut() { mark_installed(arr); }
                if let Some(arr) = json["new"].as_array_mut()     { mark_installed(arr); }
                json
            });
            let _ = std::fs::write(
                log_path(),
                serde_json::to_string_pretty(&result).unwrap_or_default(),
            );
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    homeDataLoaded: qt_method!(fn homeDataLoaded(&mut self) -> QString {
        std::fs::read_to_string(log_path()).unwrap_or_default().into()
    }),

    // ── Installed page ────────────────────────────────────────────────────────

    loadInstalled: qt_method!(fn loadInstalled(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let mut all: Vec<serde_json::Value> = Vec::new();

            // Flatpak installed — enriched with AppStream
            if let Ok(apps) = scenter_packages::get_installed_flatpaks_enriched() {
                for a in apps {
                    all.push(serde_json::to_value(a).unwrap_or_default());
                }
            }
            // Native (rpm) installed
            if let Ok(apps) = scenter_packages::get_installed() {
                for a in apps {
                    all.push(serde_json::to_value(a).unwrap_or_default());
                }
            }
            for a in scenter_appimages::get_installed() {
                all.push(serde_json::to_value(a).unwrap_or_default());
            }

            let _ = std::fs::write(
                log_path(),
                serde_json::to_string(&all).unwrap_or_default(),
            );
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    // ── App detail lookup ─────────────────────────────────────────────────────

    loadAppById: qt_method!(fn loadAppById(&mut self, app_id: QString) {
        let app_id = app_id.to_string();
        // Use a dedicated channel so this never interferes with an ongoing install op.
        let ready = Arc::new(AtomicBool::new(false));
        self.detail_shared = Some(ready.clone());
        self.detailReady = false;
        self.detailStateChanged();
        let _ = std::fs::write(detail_log_path(), "");
        std::thread::spawn(move || {
            let result = match scenter_packages::get_app_by_id(&app_id) {
                Ok(Some(app)) => serde_json::to_string(&app).unwrap_or_default(),
                _ => "null".to_string(),
            };
            let _ = std::fs::write(detail_log_path(), &result);
            ready.store(true, Ordering::Relaxed);
        });
    }),

    // ── Screenshot download / cache ───────────────────────────────────────────


    // ── Add-ons lookup (async — get_installed_packages spawns rpm subprocess) ──

    loadAddons: qt_method!(fn loadAddons(&mut self, app_id: QString, source_type: QString) {
        let app_id = app_id.to_string();
        let source_type = source_type.to_string();
        let ready = Arc::new(AtomicBool::new(false));
        self.addons_shared = Some(ready.clone());
        self.addonsReady = false;
        self.addonsStateChanged();
        let _ = std::fs::write(addons_log_path(), "[]");
        std::thread::spawn(move || {
            let json = match scenter_packages::get_addons_for_app(&app_id, &source_type) {
                Ok(addons) => serde_json::to_string(&addons).unwrap_or_else(|_| "[]".into()),
                Err(_) => "[]".into(),
            };
            let _ = std::fs::write(addons_log_path(), &json);
            ready.store(true, Ordering::Relaxed);
        });
    }),

    pollAddons: qt_method!(fn pollAddons(&mut self) {
        if let Some(ready) = &self.addons_shared {
            if ready.load(Ordering::Relaxed) {
                self.addons_shared = None;
                self.addonsReady = true;
                self.addonsStateChanged();
            }
        }
    }),

    readAddons: qt_method!(fn readAddons(&mut self) -> QString {
        std::fs::read_to_string(addons_log_path())
            .unwrap_or_else(|_| "[]".into())
            .into()
    }),

    // ── Installed Flatpak runtimes/add-ons (async) ────────────────────────────

    loadFlatpakRuntimes: qt_method!(fn loadFlatpakRuntimes(&mut self) {
        let ready = Arc::new(AtomicBool::new(false));
        self.runtimes_shared = Some(ready.clone());
        self.runtimesReady = false;
        self.runtimesStateChanged();
        let _ = std::fs::write(runtimes_log_path(), "[]");
        std::thread::spawn(move || {
            let runtimes = scenter_packages::get_installed_flatpak_runtimes();
            let _ = std::fs::write(
                runtimes_log_path(),
                serde_json::to_string(&runtimes).unwrap_or_else(|_| "[]".to_string()),
            );
            ready.store(true, Ordering::Relaxed);
        });
    }),

    pollRuntimes: qt_method!(fn pollRuntimes(&mut self) {
        if let Some(ready) = &self.runtimes_shared {
            if ready.load(Ordering::Relaxed) {
                self.runtimes_shared = None;
                self.runtimesReady = true;
                self.runtimesStateChanged();
            }
        }
    }),

    readRuntimes: qt_method!(fn readRuntimes(&mut self) -> QString {
        std::fs::read_to_string(runtimes_log_path()).unwrap_or_else(|_| "[]".to_string()).into()
    }),

    // ── AppImage settings ─────────────────────────────────────────────────────

    // ── Local file install ────────────────────────────────────────────────────

    /// Store the startup file path (set from argv in main.rs before QML loads).
    startupFilePath: qt_property!(QString; NOTIFY startupFilePathChanged),
    startupFilePathChanged: qt_signal!(),

    /// Read the startup file path from the temp flag written by main.rs.
    /// QML calls this once at Component.onCompleted; clears the flag after reading.
    readStartupFilePath: qt_method!(fn readStartupFilePath(&mut self) -> QString {
        let flag = std::env::temp_dir().join("software-center-open-file");
        if let Ok(path) = std::fs::read_to_string(&flag) {
            let _ = std::fs::remove_file(&flag);
            let path = path.trim().to_string();
            if !path.is_empty() {
                self.startupFilePath = path.clone().into();
                self.startupFilePathChanged();
                return path.into();
            }
        }
        "".into()
    }),

    /// Determine file kind from extension.
    fileKind: qt_method!(fn fileKind(&mut self, path: QString) -> QString {
        let p = path.to_string().to_lowercase();
        if p.ends_with(".rpm")        { return "rpm".into(); }
        if p.ends_with(".flatpak")    { return "flatpak".into(); }
        if p.ends_with(".flatpakref") { return "flatpakref".into(); }
        if p.ends_with(".appimage")   { return "appimage".into(); }
        "unknown".into()
    }),

    /// Get metadata for a local file. Returns JSON.
    getLocalFileInfo: qt_method!(fn getLocalFileInfo(&mut self, path: QString, kind: QString) -> QString {
        let path = path.to_string();
        let kind = kind.to_string();
        let info = match kind.as_str() {
            "rpm"        => scenter_packages::get_local_rpm_info(&path),
            "flatpak"    => scenter_flatpak::get_local_flatpak_info(&path),
            "flatpakref" => scenter_flatpak::get_flatpakref_info(&path),
            "appimage"   => scenter_appimages::get_appimage_info_for_display(&path),
            _            => serde_json::json!({"error": "Unknown file type"}),
        };
        info.to_string().into()
    }),

    /// Install a local file. Uses start_op pattern; poll opRunning/opResult.
    installLocalFile: qt_method!(fn installLocalFile(&mut self, path: QString, kind: QString, action: QString, pkg_name: QString) {
        let path     = path.to_string();
        let kind     = kind.to_string();
        let action   = action.to_string();
        let pkg_name = pkg_name.to_string();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), format!("Installing {}...\n", path));
            let ok = match kind.as_str() {
                "rpm" => {
                    let mut exit_code = 1i32;
                    let iter: Box<dyn Iterator<Item = String> + Send> = if action == "reinstall" {
                        Box::new(scenter_packages::reinstall_local_rpm_stream(&pkg_name, &path))
                    } else {
                        Box::new(scenter_packages::install_local_rpm_stream(&path))
                    };
                    for line in iter {
                        if let Some(code) = line.strip_prefix("__done__") {
                            exit_code = code.trim().parse().unwrap_or(1);
                        } else if !line.is_empty() {
                            append_log(&line);
                        }
                    }
                    exit_code == 0
                }
                "flatpak" => {
                    let mut exit_code = 1i32;
                    for line in scenter_flatpak::install_local_bundle_stream(&path) {
                        if let Some(code) = line.strip_prefix("__done__") {
                            exit_code = code.trim().parse().unwrap_or(1);
                        } else if !line.is_empty() {
                            append_log(&line);
                        }
                    }
                    exit_code == 0
                }
                "flatpakref" => {
                    let mut exit_code = 1i32;
                    for line in scenter_flatpak::install_flatpakref_stream(&path) {
                        if let Some(code) = line.strip_prefix("__done__") {
                            exit_code = code.trim().parse().unwrap_or(1);
                        } else if !line.is_empty() {
                            append_log(&line);
                        }
                    }
                    exit_code == 0
                }
                "appimage" => {
                    let (ok, msg, _) = scenter_appimages::install_appimage(&path);
                    append_log(&msg);
                    ok
                }
                _ => { append_log("Unknown file type"); false }
            };
            shared.result.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    // ── AppImage settings ─────────────────────────────────────────────────────

    saveAppImageSettings: qt_method!(fn saveAppImageSettings(&mut self, id: QString, update_source: QString, update_url: QString, update_pattern: QString, allow_prerelease: bool) -> QString {
        let (ok, msg) = scenter_appimages::save_settings(
            &id.to_string(),
            &update_source.to_string(),
            &update_url.to_string(),
            &update_pattern.to_string(),
            allow_prerelease,
        );
        serde_json::json!({ "ok": ok, "msg": msg }).to_string().into()
    }),

    // ── Search ────────────────────────────────────────────────────────────────

    search: qt_method!(fn search(&mut self, query: QString) {
        let query = query.to_string();
        self.searchQuery = query.clone().into();
        self.searchQueryChanged();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let mut results: Vec<serde_json::Value> = Vec::new();

            if let Ok(apps) = scenter_packages::search(&query) {
                for a in apps { results.push(serde_json::to_value(a).unwrap_or_default()); }
            }

            let _ = std::fs::write(
                log_path(),
                serde_json::to_string(&results).unwrap_or_default(),
            );
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    loadCategory: qt_method!(fn loadCategory(&mut self, category: QString) {
        let category = category.to_string();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let mut results: Vec<serde_json::Value> = Vec::new();
            if let Ok(apps) = scenter_packages::get_by_category(&category) {
                for a in apps { results.push(serde_json::to_value(a).unwrap_or_default()); }
            }
            let _ = std::fs::write(
                log_path(),
                serde_json::to_string(&results).unwrap_or_default(),
            );
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    loadSource: qt_method!(fn loadSource(&mut self, source: QString) {
        let source = source.to_string();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let mut results: Vec<serde_json::Value> = Vec::new();
            if let Ok(apps) = scenter_packages::get_by_source(&source) {
                for a in apps { results.push(serde_json::to_value(a).unwrap_or_default()); }
            }
            let _ = std::fs::write(
                log_path(),
                serde_json::to_string(&results).unwrap_or_default(),
            );
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    // ── Updates ───────────────────────────────────────────────────────────────

    checkUpdates: qt_method!(fn checkUpdates(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), "Checking for updates...\n");

            // Check packages via dnf5
            let pkg_updates: Vec<serde_json::Value> = scenter_updates::enrich_package_updates(
                scenter_updates::check_packages_script(),
            );

            // Use get_all_updates() so flatpak entries are icon-enriched via the
            // AppStream cache (same path as the installed page).
            let fp_updates: Vec<serde_json::Value> = scenter_flatpak::get_all_updates()
                .into_iter()
                .filter_map(|f| serde_json::to_value(f).ok())
                .collect();

            let total = pkg_updates.len() + fp_updates.len();

            let result = serde_json::json!({
                "packages":        pkg_updates,
                "flatpak":         fp_updates,
                "appimages":       [],
                "total": total,
            });

            let result_json = serde_json::to_string_pretty(&result).unwrap_or_default();
            let _ = std::fs::write(log_path(), &result_json);

            // Write to daemon cache so the tray and update page share the same data
            let cache_path = daemon_cache_path();
            if let Some(parent) = cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&cache_path, &result_json);

            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    rebootSystem: qt_method!(fn rebootSystem(&mut self) {
        std::thread::spawn(|| {
            scenter_updates::schedule_reboot();
        });
    }),

    upgradePackages: qt_method!(fn upgradePackages(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), "Upgrading packages...\n");
            let mut exit_code = 1i32;
            for line in scenter_updates::upgrade_packages_stream() {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else {
                    // Parse [N/M] progress from dnf5 transaction output
                    if let Some(pct) = parse_install_progress(&line) {
                        shared.progress.store(pct, Ordering::Relaxed);
                    }
                    if !line.is_empty() { append_log(&line); }
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    upgradePackage: qt_method!(fn upgradePackage(&mut self, name: QString) {
        let name = name.to_string();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), format!("Upgrading package {}...\n", name));
            let mut exit_code = 1i32;
            for line in scenter_updates::upgrade_single_package_stream(&name) {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else {
                    if let Some(pct) = parse_install_progress(&line) {
                        shared.progress.store(pct, Ordering::Relaxed);
                    }
                    if !line.is_empty() { append_log(&line); }
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    // ── Repositories page ─────────────────────────────────────────────────────

    /// List all dnf repositories (enabled + disabled) as JSON array.
    /// [{"id","name","enabled","kind","owner","project"}]
    loadRepos: qt_method!(fn loadRepos(&mut self) {
        let ready = Arc::new(AtomicBool::new(false));
        self.repos_shared = Some(ready.clone());
        self.reposReady = false;
        self.reposStateChanged();
        let _ = std::fs::write(repos_log_path(), "[]");
        std::thread::spawn(move || {
            let repos: Vec<serde_json::Value> = scenter_updates::list_dnf_repos()
                .into_iter()
                .map(|r| serde_json::json!({
                    "id":      r.id,
                    "name":    r.name,
                    "enabled": r.enabled,
                    "kind":    r.kind,
                    "owner":   r.owner,
                    "project": r.project,
                }))
                .collect();
            let _ = std::fs::write(
                repos_log_path(),
                serde_json::to_string(&repos).unwrap_or_else(|_| "[]".to_string()),
            );
            ready.store(true, Ordering::Relaxed);
        });
    }),

    pollRepos: qt_method!(fn pollRepos(&mut self) {
        if let Some(ready) = &self.repos_shared {
            if ready.load(Ordering::Relaxed) {
                self.repos_shared = None;
                self.reposReady = true;
                self.reposStateChanged();
            }
        }
    }),

    readRepos: qt_method!(fn readRepos(&mut self) -> QString {
        std::fs::read_to_string(repos_log_path()).unwrap_or_else(|_| "[]".to_string()).into()
    }),

    /// List orphaned (unused dependency) packages as JSON array of names.
    /// Async — `dnf5 -q repoquery --unneeded` runs on a worker thread.
    loadUnusedPackages: qt_method!(fn loadUnusedPackages(&mut self) {
        let ready = Arc::new(AtomicBool::new(false));
        self.unused_shared = Some(ready.clone());
        self.unusedReady = false;
        self.unusedStateChanged();
        let _ = std::fs::write(unused_log_path(), "[]");
        std::thread::spawn(move || {
            let _ = std::fs::write(
                unused_log_path(),
                serde_json::to_string(&scenter_updates::list_unused_packages())
                    .unwrap_or_else(|_| "[]".to_string()),
            );
            ready.store(true, Ordering::Relaxed);
        });
    }),

    pollUnused: qt_method!(fn pollUnused(&mut self) {
        if let Some(ready) = &self.unused_shared {
            if ready.load(Ordering::Relaxed) {
                self.unused_shared = None;
                self.unusedReady = true;
                self.unusedStateChanged();
            }
        }
    }),

    readUnused: qt_method!(fn readUnused(&mut self) -> QString {
        std::fs::read_to_string(unused_log_path()).unwrap_or_else(|_| "[]".to_string()).into()
    }),

    /// Enable/disable any dnf repo by id. COPR repos route through the dnf5
    /// copr plugin (owner/project), system repos via config-manager.
    setRepoEnabled: qt_method!(fn setRepoEnabled(&mut self, id: QString, enabled: bool) {
        let id = id.to_string();
        let enabled = enabled;
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let owner_project = repo_owner_project(&id);
            let msg = format!("{} repository {}...\n", if enabled { "Enabling" } else { "Disabling" }, id);
            let _ = std::fs::write(log_path(), msg);
            let mut exit_code = 1i32;
            let iter: Box<dyn Iterator<Item = String> + Send> = if id.starts_with("copr:") {
                if owner_project.is_empty() {
                    append_log("Cannot parse COPR owner/project");
                    shared.result.store(2, Ordering::Relaxed);
                    shared.running.store(false, Ordering::Relaxed);
                    return;
                }
                if enabled {
                    Box::new(scenter_updates::enable_copr_stream(&owner_project))
                } else {
                    Box::new(scenter_updates::disable_copr_stream(&owner_project))
                }
            } else {
                Box::new(scenter_updates::set_repo_enabled_stream(&id, enabled))
            };
            for line in iter {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else if !line.is_empty() {
                    append_log(&line);
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    /// Add/enable a COPR repo from the UI ("owner/project"). Runs pkexec.
    addCopr: qt_method!(fn addCopr(&mut self, owner_project: QString) {
        let owner_project = owner_project.to_string().trim().to_string();
        if owner_project.is_empty() || !owner_project.contains('/') {
            let _ = std::fs::write(log_path(), "Invalid COPR spec. Use owner/project (e.g. tofan79/software-center).\n");
            return;
        }
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), format!("Adding COPR {}...\n", owner_project));
            let mut exit_code = 1i32;
            for line in scenter_updates::enable_copr_stream(&owner_project) {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else if !line.is_empty() {
                    append_log(&line);
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    /// Remove a COPR repo entirely (deletes its .repo file). Runs pkexec.
    removeCopr: qt_method!(fn removeCopr(&mut self, owner_project: QString) {
        let owner_project = owner_project.to_string().trim().to_string();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), format!("Removing COPR {}...\n", owner_project));
            let mut exit_code = 1i32;
            for line in scenter_updates::remove_copr_stream(&owner_project) {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else if !line.is_empty() {
                    append_log(&line);
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    /// Clear the dnf metadata cache. Runs pkexec.
    cleanDnfCache: qt_method!(fn cleanDnfCache(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), "Cleaning dnf cache...\n");
            let mut exit_code = 1i32;
            for line in scenter_updates::clean_dnf_stream() {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else if !line.is_empty() {
                    append_log(&line);
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    /// Remove unused/orphan packages (dnf autoremove). Runs pkexec.
    removeUnusedPackages: qt_method!(fn removeUnusedPackages(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), "Removing unused packages (dnf autoremove)...\n");
            let mut exit_code = 1i32;
            for line in scenter_updates::autoremove_stream() {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else if !line.is_empty() {
                    append_log(&line);
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    /// Remove unused Flatpak runtimes/extensions. Global cache cleanup.
    cleanFlatpakUnused: qt_method!(fn cleanFlatpakUnused(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let _ = std::fs::write(log_path(), "Removing unused Flatpak runtimes...\n");
            let mut exit_code = 1i32;
            for line in scenter_flatpak::clean_unused_stream() {
                if let Some(code) = line.strip_prefix("__done__") {
                    exit_code = code.trim().parse().unwrap_or(1);
                } else if !line.is_empty() {
                    append_log(&line);
                }
            }
            shared.result.store(if exit_code == 0 { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    /// Remove orphaned AppImage files (stale downloads, unreferenced binaries,
    /// sidecars whose binary is gone). Quick local op, no root.
    cleanAppImageCache: qt_method!(fn cleanAppImageCache(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let (_count, msg) = scenter_appimages::cleanup_orphans();
            let _ = std::fs::write(log_path(), format!("{}\n", msg));
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    // Update a Flatpak from the Updates page WITHOUT entering the install/remove
    // queue. Mirrors installApp's flatpak commands but runs as a plain streaming
    // op (like upgradePackage), so flatpak updates show inline progress on the
    // updates page and never appear in the queue banner.
    //   source "flatpak":        "__upgrade_all__" → update all; else install a ref
    //   source "flatpak-update": update one app ("--app id") or runtime ("id//branch")
    upgradeFlatpak: qt_method!(fn upgradeFlatpak(&mut self, id: QString, source: QString, remote: QString) {
        let id = id.to_string();
        let source = source.to_string();
        let remote = remote.to_string();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};
            let _ = std::fs::write(log_path(), format!("Updating Flatpak {}...\n", id));

            let mut cmd = Command::new("flatpak");
            match source.as_str() {
                "flatpak" if id == "__upgrade_all__" => { cmd.args(["update", "--noninteractive", "-y"]); }
                "flatpak" => {
                    let r = if remote.is_empty() { "flathub" } else { remote.as_str() };
                    let scope = if scenter_flatpak::is_user_remote(r) { "--user" } else { "--system" };
                    cmd.args(["install", scope, "--noninteractive", "-y", r, &id]);
                }
                _ => {
                    // "flatpak-update": runtime refs carry "//branch" (no --app); plain
                    // app ids use --app so runtimes aren't touched.
                    if id.contains("//") {
                        cmd.args(["update", "--noninteractive", "-y", &id]);
                    } else {
                        cmd.args(["update", "--app", "--noninteractive", "-y", &id]);
                    }
                }
            }

            match cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
                Ok(mut child) => {
                    let stderr = child.stderr.take();
                    let stderr_thread = stderr.map(|s| {
                        std::thread::spawn(move || { for _ in BufReader::new(s).lines() {} })
                    });
                    // Fake-crawl progress from 2→95 while the process runs.
                    let shared_crawl = Arc::clone(&shared);
                    let crawl = std::thread::spawn(move || {
                        loop {
                            std::thread::sleep(std::time::Duration::from_millis(200));
                            if !shared_crawl.running.load(Ordering::Relaxed) { break; }
                            let cur = shared_crawl.progress.load(Ordering::Relaxed);
                            if cur < 95 { shared_crawl.progress.store((cur + 1).min(95), Ordering::Relaxed); }
                        }
                    });
                    if let Some(stdout) = child.stdout.take() {
                        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                            if !line.is_empty() {
                                append_log(&line);
                                if let Some(pct) = parse_install_progress(&line) {
                                    let cur = shared.progress.load(Ordering::Relaxed);
                                    if pct > cur { shared.progress.store(pct, Ordering::Relaxed); }
                                }
                            }
                        }
                    }
                    if let Some(t) = stderr_thread { t.join().ok(); }
                    let success = child.wait().map(|s| s.success()).unwrap_or(false);
                    shared.result.store(if success { 1 } else { 2 }, Ordering::Relaxed);
                    shared.running.store(false, Ordering::Relaxed);
                    crawl.join().ok();
                }
                Err(e) => {
                    append_log(&e.to_string());
                    shared.result.store(2, Ordering::Relaxed);
                    shared.running.store(false, Ordering::Relaxed);
                }
            }
        });
    }),


    // ── Install / Remove ─────────────────────────────────────────────────────

    installApp: qt_method!(fn installApp(&mut self, id: QString, source: QString, remote: QString,
                                          app_name: QString, icon_path_hint: QString, icon_url_hint: QString,
                                          user_remote: bool) {
        let id = id.to_string();
        let source = source.to_string();
        let remote = remote.to_string();

        let (display, ip, iu) = resolve_app_display_info(
            &id,
            app_name.to_string(),
            icon_path_hint.to_string(),
            icon_url_hint.to_string(),
        );

        // If an install/remove is already running, enqueue for later.
        if self.op_is_install_remove || self.opRunning {
            self.install_queue.push_back(InstallQueueEntry {
                app_name: display,
                id,
                source,
                remote,
                is_remove: false,
                icon_path: ip,
                icon_url: iu,
                user_remote,
            });
            self.queueCount = self.install_queue.len() as i32;
            self.queueStateChanged();
            return;
        }

        self.op_is_install_remove = true;
        self.queueActiveName = display.into();
        self.queueActiveIconPath = ip.into();
        self.queueActiveIconUrl = iu.into();
        self.queueIsRemove = false;
        self.queueStateChanged();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            use std::process::{Command, Stdio};
            let _ = std::fs::write(log_path(), format!("Installing {}...\n", id));

            let (child, ok) = match source.as_str() {
                "flatpak" => {
                    let mut cmd = Command::new("flatpak");
                    if id == "__upgrade_all__" {
                        cmd.args(["update", "--noninteractive", "-y"]);
                    } else {
                        // Fresh install from detail page (or //branch runtime install).
                        // Scope comes from the user's explicit System/User dropdown
                        // choice, not from how the remote happens to be configured —
                        // the same remote name can exist in both scopes.
                        let r = if remote.is_empty() { "flathub" } else { remote.as_str() };
                        let scope = if user_remote { "--user" } else { "--system" };
                        cmd.args(["install", scope, "--noninteractive", "-y", r, &id]);
                    }
                    let c = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
                    match c {
                        Ok(child) => (Some(child), true),
                        Err(e)    => { append_log(&e.to_string()); (None, false) }
                    }
                }
                "flatpak-update" => {
                    // Individual update from the updates page.
                    // Runtime patch updates arrive as "app_id//branch" — no --app flag.
                    // App updates arrive as plain "app_id" — use --app to avoid touching runtimes.
                    let mut cmd = Command::new("flatpak");
                    if id.contains("//") {
                        cmd.args(["update", "--noninteractive", "-y", &id]);
                    } else {
                        cmd.args(["update", "--app", "--noninteractive", "-y", &id]);
                    }
                    let c = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
                    match c {
                        Ok(child) => (Some(child), true),
                        Err(e)    => { append_log(&e.to_string()); (None, false) }
                    }
                }
                _ => {
                    // Resolve AppStream ID → RPM package name
                    let appstream = scenter_appstream::get_appstream();
                    let pkg = appstream.get(&id)
                        .or_else(|| appstream.get(&format!("native:{}", id)))
                        .map(|a| a.package_name.clone())
                        .filter(|p| !p.is_empty())
                        .unwrap_or_else(|| id.clone());
                    drop(appstream);
                    let c = Command::new("pkexec")
                        .args(["dnf5", "install", "-y", &pkg])
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn();
                    match c {
                        Ok(child) => (Some(child), true),
                        Err(e)    => { append_log(&e.to_string()); (None, false) }
                    }
                }
            };

            if let Some(mut child) = child {
                // Drain stdout and parse progress; drain stderr concurrently.
                let stdout = child.stdout.take();
                let stderr = child.stderr.take();
                let stderr_thread = stderr.map(|s| {
                    std::thread::spawn(move || { for _ in BufReader::new(s).lines() {} })
                });
                // Fake-crawl thread: advances progress from 2→95 while process runs.
                let shared_crawl = Arc::clone(&shared);
                let crawl = std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        if !shared_crawl.running.load(Ordering::Relaxed) { break; }
                        let cur = shared_crawl.progress.load(Ordering::Relaxed);
                        if cur < 95 {
                            // Real progress parsing may have jumped ahead; only advance.
                            let next = (cur + 1).min(95);
                            shared_crawl.progress.store(next, Ordering::Relaxed);
                        }
                    }
                });
                if let Some(stdout) = stdout {
                    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                        if !line.is_empty() {
                            append_log(&line);
                            if let Some(pct) = parse_install_progress(&line) {
                                let cur = shared.progress.load(Ordering::Relaxed);
                                if pct > cur { shared.progress.store(pct, Ordering::Relaxed); }
                            }
                        }
                    }
                }
                if let Some(t) = stderr_thread { t.join().ok(); }
                let success = child.wait().map(|s| s.success()).unwrap_or(false);
                shared.result.store(if success { 1 } else { 2 }, Ordering::Relaxed);
                // Signal done before joining crawl so it sees running=false and exits.
                shared.running.store(false, Ordering::Relaxed);
                crawl.join().ok();
                return;
            } else {
                shared.result.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            }
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    removeApp: qt_method!(fn removeApp(&mut self, id: QString, source: QString,
                                        app_name: QString, icon_path_hint: QString, icon_url_hint: QString) {
        let id = id.to_string();
        let source = source.to_string();

        let (display, ip, iu) = resolve_app_display_info(
            &id,
            app_name.to_string(),
            icon_path_hint.to_string(),
            icon_url_hint.to_string(),
        );

        // If an install/remove is already running, enqueue for later.
        if self.op_is_install_remove || self.opRunning {
            self.install_queue.push_back(InstallQueueEntry {
                app_name: display,
                id,
                source,
                remote: String::new(),
                is_remove: true,
                icon_path: ip,
                icon_url: iu,
                user_remote: false,
            });
            self.queueCount = self.install_queue.len() as i32;
            self.queueStateChanged();
            return;
        }

        self.op_is_install_remove = true;
        self.queueActiveName = display.into();
        self.queueActiveIconPath = ip.into();
        self.queueActiveIconUrl = iu.into();
        self.queueIsRemove = true;
        self.queueStateChanged();
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            use std::process::{Command, Stdio};
            use std::io::{BufRead, BufReader};
            let _ = std::fs::write(log_path(), format!("Removing {}...\n", id));

            // Fake-crawl: advance progress 2→95 while remove runs
            let shared_crawl = Arc::clone(&shared);
            let crawl = std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    if !shared_crawl.running.load(Ordering::Relaxed) { break; }
                    let cur = shared_crawl.progress.load(Ordering::Relaxed);
                    if cur < 95 {
                        shared_crawl.progress.store((cur + 1).min(95), Ordering::Relaxed);
                    }
                }
            });

            let ok = match source.as_str() {
                "flatpak" | "flatpak-runtime" => {
                    // No scope flag defaults flatpak to --system, so a flatpak
                    // installed only in the user scope silently fails to
                    // remove — detect which scope it's actually in first.
                    let scope = flatpak_installed_scope(&id);
                    let mut cmd = Command::new("flatpak");
                    let mut args = vec!["uninstall", scope, "--noninteractive", "-y"];
                    if source == "flatpak-runtime" { args.push("--force-remove"); }
                    args.push(&id);
                    match cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
                        Ok(mut child) => {
                            let stdout = child.stdout.take();
                            let stderr = child.stderr.take();
                            let stderr_t = stderr.map(|s| {
                                std::thread::spawn(move || { for _ in BufReader::new(s).lines() {} })
                            });
                            if let Some(out) = stdout {
                                for line in BufReader::new(out).lines().map_while(Result::ok) {
                                    if !line.is_empty() { append_log(&line); }
                                }
                            }
                            if let Some(t) = stderr_t { t.join().ok(); }
                            child.wait().map(|s| s.success()).unwrap_or(false)
                        }
                        Err(e) => { append_log(&e.to_string()); false }
                    }
                }
                "appimage" => {
                    let (ok, msg) = scenter_appimages::uninstall(&id);
                    append_log(&msg);
                    ok
                }
                _ => {
                    let appstream = scenter_appstream::get_appstream();
                    let pkg = appstream.get(&id)
                        .or_else(|| appstream.get(&format!("native:{}", id)))
                        .map(|a| a.package_name.clone())
                        .filter(|p| !p.is_empty())
                        .unwrap_or_else(|| id.clone());
                    drop(appstream);
                    match Command::new("pkexec")
                        .args(["dnf5", "remove", "-y", &pkg])
                        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
                    {
                        Ok(mut child) => {
                            let stdout = child.stdout.take();
                            let stderr = child.stderr.take();
                            let stderr_t = stderr.map(|s| {
                                std::thread::spawn(move || { for _ in BufReader::new(s).lines() {} })
                            });
                            if let Some(out) = stdout {
                                for line in BufReader::new(out).lines().map_while(Result::ok) {
                                    if !line.is_empty() { append_log(&line); }
                                }
                            }
                            if let Some(t) = stderr_t { t.join().ok(); }
                            child.wait().map(|s| s.success()).unwrap_or(false)
                        }
                        Err(e) => { append_log(&e.to_string()); false }
                    }
                }
            };

            shared.result.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
            crawl.join().ok();
        });
    }),

    // ── System status ─────────────────────────────────────────────────────────

    loadSystemStatus: qt_method!(fn loadSystemStatus(&mut self) {
        self.start_op();
        let shared = self.get_shared();
        std::thread::spawn(move || {
            let status = scenter_updates::get_system_status();
            let result = serde_json::json!({
                "os":      status.os,
                "version": status.version,
                "kernel":  status.kernel,
                "error":   status.error,
            });
            let json = serde_json::to_string(&result).unwrap_or_default();
            let _ = std::fs::write(log_path(), &json);
            shared.result.store(1, Ordering::Relaxed);
            shared.running.store(false, Ordering::Relaxed);
        });
    }),

    // ── Flatpak remote management ─────────────────────────────────────────────

    loadFlatpakRemotes: qt_method!(fn loadFlatpakRemotes(&mut self) {
        let ready = Arc::new(AtomicBool::new(false));
        self.remotes_shared = Some(ready.clone());
        self.remotesReady = false;
        self.remotesStateChanged();
        let _ = std::fs::write(remotes_log_path(), "{}");
        std::thread::spawn(move || {
            let remotes = scenter_flatpak::get_remotes();
            let has_flathub = scenter_flatpak::has_flathub();
            let has_flathub_system = scenter_flatpak::has_flathub_scoped(true);
            let has_flathub_user = scenter_flatpak::has_flathub_scoped(false);
            let has_cosmic_welcome = scenter_flatpak::has_cosmic_welcome();
            let has_cosmic_remote_system = scenter_flatpak::has_cosmic_remote_scoped(true);
            let has_cosmic_remote_user = scenter_flatpak::has_cosmic_remote_scoped(false);
            let json = serde_json::json!({
                "remotes": remotes,
                "has_flathub": has_flathub,
                "has_flathub_system": has_flathub_system,
                "has_flathub_user": has_flathub_user,
                "has_cosmic_welcome": has_cosmic_welcome,
                "has_cosmic_remote_system": has_cosmic_remote_system,
                "has_cosmic_remote_user": has_cosmic_remote_user,
            })
                .to_string();
            let _ = std::fs::write(remotes_log_path(), json);
            ready.store(true, Ordering::Relaxed);
        });
    }),

    pollRemotes: qt_method!(fn pollRemotes(&mut self) {
        if let Some(ready) = &self.remotes_shared {
            if ready.load(Ordering::Relaxed) {
                self.remotes_shared = None;
                self.remotesReady = true;
                self.remotesStateChanged();
            }
        }
    }),

    readRemotes: qt_method!(fn readRemotes(&mut self) -> QString {
        std::fs::read_to_string(remotes_log_path()).unwrap_or_else(|_| "{}".to_string()).into()
    }),

    addFlatpakRemote: qt_method!(fn addFlatpakRemote(&mut self, name: QString, url: QString, system: bool) -> QString {
        let (ok, msg) = scenter_flatpak::add_remote(&name.to_string(), &url.to_string(), system);
        serde_json::json!({ "ok": ok, "msg": msg }).to_string().into()
    }),

    addFlathub: qt_method!(fn addFlathub(&mut self, system: bool) -> QString {
        let (ok, msg) = scenter_flatpak::add_flathub(system);
        serde_json::json!({ "ok": ok, "msg": msg }).to_string().into()
    }),

    addCosmicRemote: qt_method!(fn addCosmicRemote(&mut self, system: bool) -> QString {
        let (ok, msg) = scenter_flatpak::add_cosmic_remote(system);
        serde_json::json!({ "ok": ok, "msg": msg }).to_string().into()
    }),

    removeFlatpakRemote: qt_method!(fn removeFlatpakRemote(&mut self, name: QString, system: bool) -> QString {
        let (ok, msg) = scenter_flatpak::remove_remote(&name.to_string(), system);
        serde_json::json!({ "ok": ok, "msg": msg }).to_string().into()
    }),

    setFlatpakRemoteEnabled: qt_method!(fn setFlatpakRemoteEnabled(&mut self, name: QString, enabled: bool, system: bool) -> QString {
        let (ok, msg) = scenter_flatpak::set_remote_enabled(&name.to_string(), enabled, system);
        serde_json::json!({ "ok": ok, "msg": msg }).to_string().into()
    }),

    // ── Settings ─────────────────────────────────────────────────────────────

    loadSettings: qt_method!(fn loadSettings(&mut self) -> QString {
        std::fs::read_to_string(settings_path())
            .unwrap_or_else(|_| serde_json::json!({
                "update_interval": 1440,
                "auto_check_packages": true,
                "auto_check_flatpak": true,
                "auto_check_appimages": true,
                "auto_update": false,
            }).to_string())
            .into()
    }),

    saveSettings: qt_method!(fn saveSettings(&mut self, json: QString) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, json.to_string());
    }),

    // ── Startup helpers ───────────────────────────────────────────────────────

    // Pre-warm the appstream cache in a background thread so first searches
    // and category loads are instant.
    warmCache: qt_method!(fn warmCache(&mut self) {
        std::thread::spawn(|| {
            let _ = scenter_appstream::get_appstream();
            CACHE_READY.store(true, Ordering::Release);
            log::info!("AppStream cache warmed.");
        });
    }),

    isCacheReady: qt_method!(fn isCacheReady(&mut self) -> bool {
        CACHE_READY.load(Ordering::Acquire)
    }),

    // Returns true while a dnf5 upgrade operation is running.
    // QML polls this on a timer and disables install buttons for repo packages.
    isOverlayUpdateRunning: qt_method!(fn isOverlayUpdateRunning(&mut self) -> bool {
        scenter_updates::is_upgrade_running()
    }),

    // Returns true when running in a live ISO environment (liveuser session).
    // Used by QML to hide the System page from the sidebar.
    isLiveEnvironment: qt_method!(fn isLiveEnvironment(&mut self) -> bool {
        std::env::var("USER").unwrap_or_default() == "liveuser"
            || std::path::Path::new("/run/live").exists()
    }),

    // Check if main.rs wrote a "start hidden" flag (launched with --tray).
    // Returns true once (deletes the flag) so QML knows to stay hidden at startup.
    readStartHidden: qt_method!(fn readStartHidden(&mut self) -> bool {
        let flag = std::env::temp_dir().join("software-center-start-hidden");
        if flag.exists() {
            let _ = std::fs::remove_file(&flag);
            return true;
        }
        false
    }),

    // Check if the tray daemon wrote a "show window" flag.
    // Returns true once (deletes the flag) so QML can show + activate the window.
    checkShowRequest: qt_method!(fn checkShowRequest(&mut self) -> bool {
        let flag = show_flag_path();
        if flag.exists() {
            let _ = std::fs::remove_file(&flag);
            return true;
        }
        false
    }),

    // Check if the tray daemon wrote a "quit" flag.
    // Returns true once (deletes the flag) so QML can call Qt.quit().
    checkQuitRequest: qt_method!(fn checkQuitRequest(&mut self) -> bool {
        let flag = std::env::temp_dir().join("software-center-quit");
        if flag.exists() {
            let _ = std::fs::remove_file(&flag);
            return true;
        }
        false
    }),

    // Read cached update count written by the daemon's last background check.
    // Populates pendingUpdateCount so the UI can show a badge without
    // blocking on a fresh check.
    loadDaemonUpdateCache: qt_method!(fn loadDaemonUpdateCache(&mut self) {
        let path = daemon_cache_path();
        if let Ok(json) = std::fs::read_to_string(&path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json) {
                let count = val["total"].as_i64().unwrap_or(0) as i32;
                if self.pendingUpdateCount != count {
                    self.pendingUpdateCount = count;
                    self.pendingUpdateCountChanged();
                }
            }
        }
    }),

    // Update the local badge property and notify the tray daemon. This is used
    // after rows disappear so the tray badge tracks completed updates immediately.
    setPendingUpdateCount: qt_method!(fn setPendingUpdateCount(&mut self, count: i32) {
        let count = count.max(0);
        if self.pendingUpdateCount != count {
            self.pendingUpdateCount = count;
            self.pendingUpdateCountChanged();
        }

        let _ = std::fs::write(badge_count_path(), count.to_string());

        let cache_path = daemon_cache_path();
        if let Ok(json) = std::fs::read_to_string(&cache_path) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&json) {
                val["total"] = serde_json::json!(count);
                if let Some(parent) = cache_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(
                    &cache_path,
                    serde_json::to_string_pretty(&val).unwrap_or_default(),
                );
            }
        }
    }),

    // Return the raw JSON from the daemon's update cache, or "" if not yet
    // written (daemon still running its first check).
    loadUpdatesCache: qt_method!(fn loadUpdatesCache(&mut self) -> QString {
        std::fs::read_to_string(daemon_cache_path())
            .unwrap_or_default()
            .into()
    }),

    // Rewrite the daemon cache as "up to date" after a full update queue.
    // Keeps the badge and re-navigation from showing stale rows.
    clearUpdatesCache: qt_method!(fn clearUpdatesCache(&mut self) {
        let empty = serde_json::json!({
            "packages":  [],
            "flatpak":   [],
            "appimages": [],
            "total":     0,
        });
        let json = serde_json::to_string_pretty(&empty).unwrap_or_default();
        if let Some(parent) = daemon_cache_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(daemon_cache_path(), json);
        let _ = std::fs::write(badge_count_path(), "0");
        if self.pendingUpdateCount != 0 {
            self.pendingUpdateCount = 0;
            self.pendingUpdateCountChanged();
        }
    }),

    // Remove a single completed update from the daemon cache and drop the badge
    // count accordingly. kind = "packages" (key "name") or "flatpak" (key "app_id").
    pruneCacheEntry: qt_method!(fn pruneCacheEntry(&mut self, kind: QString, key: QString, value: QString) {
        let kind = kind.to_string();
        let key = key.to_string();
        let value = value.to_string();
        let cache_path = daemon_cache_path();
        if let Ok(json) = std::fs::read_to_string(&cache_path) {
            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(arr) = val.get_mut(&kind).and_then(|v| v.as_array_mut()) {
                    arr.retain(|item| item[key.as_str()] != serde_json::Value::String(value.clone()));
                }
                let total: usize =
                    val["packages"].as_array().map(|a| a.len()).unwrap_or(0)
                    + val["flatpak"].as_array().map(|a| a.len()).unwrap_or(0)
                    + val["appimages"].as_array().map(|a| a.len()).unwrap_or(0);
                val["total"] = serde_json::json!(total);
                let _ = std::fs::write(
                    &cache_path,
                    serde_json::to_string_pretty(&val).unwrap_or_default(),
                );
                let _ = std::fs::write(badge_count_path(), total.to_string());
                if self.pendingUpdateCount != total as i32 {
                    self.pendingUpdateCount = total as i32;
                    self.pendingUpdateCountChanged();
                }
            }
        }
    }),

    // Returns true if the daemon wrote a check-trigger flag (and consumes it).
    // The UI calls this on a timer and runs checkUpdates() when true.
    checkDaemonTrigger: qt_method!(fn checkDaemonTrigger(&mut self) -> bool {
        let path = std::env::temp_dir().join("software-center-check-requested");
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            true
        } else {
            false
        }
    }),
}

impl SoftwareBackend {
    fn get_shared(&mut self) -> Arc<SharedState> {
        if self.shared.is_none() {
            self.shared = Some(Arc::new(SharedState::default()));
        }
        self.shared.as_ref().unwrap().clone()
    }

    fn start_op(&mut self) {
        let s = self.get_shared();
        s.running.store(true, Ordering::Relaxed);
        s.result.store(0, Ordering::Relaxed);
        s.progress.store(2, Ordering::Relaxed);  // start at 2 so bar is immediately visible
        self.opRunning  = true;
        self.opResult   = 0;
        self.opProgress = 2;
        self.opStateChanged();
        let _ = std::fs::write(log_path(), "");
        self.logRevision = 0;
        self.logRevisionChanged();
    }

    /// Pop the next queued install/remove and start it, or clear banner if empty.
    fn dequeue_next_installop(&mut self) {
        if let Some(entry) = self.install_queue.pop_front() {
            let display = entry.app_name.clone();
            let is_remove = entry.is_remove;
            self.queueCount = self.install_queue.len() as i32;
            self.queueActiveName = display.clone().into();
            self.queueActiveIconPath = entry.icon_path.clone().into();
            self.queueActiveIconUrl = entry.icon_url.clone().into();
            self.queueIsRemove = is_remove;
            self.queueStateChanged();
            if is_remove {
                self.removeApp(entry.id.clone().into(), entry.source.clone().into(),
                               display.clone().into(), entry.icon_path.clone().into(), entry.icon_url.clone().into());
            } else {
                self.installApp(entry.id.clone().into(), entry.source.clone().into(), entry.remote.clone().into(),
                                display.clone().into(), entry.icon_path.clone().into(), entry.icon_url.clone().into(),
                                entry.user_remote);
            }
        } else {
            // Queue drained — hide banner.
            self.queueCount = 0;
            self.queueActiveName = QString::default();
            self.queueActiveIconPath = QString::default();
            self.queueActiveIconUrl = QString::default();
            self.queueIsRemove = false;
            self.queueStateChanged();
        }
    }
}

/// Returns (display_name, icon_path, icon_url) from the appstream cache.
fn lookup_app_info(id: &str) -> (String, String, String) {
    let cache = scenter_appstream::get_appstream();
    if let Some(app) = cache.get(id) {
        (app.name.clone(), app.icon_path.clone(), app.icon_url.clone())
    } else {
        // Fallback: last dot-segment of the ID as a best-guess name
        let name = id.split('.').last().unwrap_or(id).to_string();
        (name, String::new(), String::new())
    }
}

/// Prefer caller-supplied display info; fill gaps from AppStream cache.
fn resolve_app_display_info(
    id: &str,
    hint_name: String,
    hint_icon_path: String,
    hint_icon_url: String,
) -> (String, String, String) {
    // If the caller already gave us everything, skip the cache lookup entirely.
    if !hint_name.is_empty() && (!hint_icon_path.is_empty() || !hint_icon_url.is_empty()) {
        return (hint_name, hint_icon_path, hint_icon_url);
    }
    let (cache_name, cache_path, cache_url) = lookup_app_info(id);
    let name = if hint_name.is_empty() { cache_name } else { hint_name };
    let ip   = if hint_icon_path.is_empty() { cache_path } else { hint_icon_path };
    let iu   = if hint_icon_url.is_empty()  { cache_url  } else { hint_icon_url  };
    (name, ip, iu)
}

/// Parse [N/M] or "N of M" lines from dnf5/flatpak install output → 0-100.
fn parse_install_progress(line: &str) -> Option<i32> {
    let trimmed = line.trim();
    // [N/M] bracket format (dnf5, flatpak)
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed[start..].find(']') {
            let inner = &trimmed[start + 1..start + end];
            if let Some(slash) = inner.find('/') {
                let n: f64 = inner[..slash].trim().parse().ok()?;
                let total: f64 = inner[slash + 1..].trim().parse().ok()?;
                if total > 0.0 {
                    return Some(((n / total) * 100.0).min(100.0) as i32);
                }
            }
        }
    }
    // "N of M" format
    if let Some(pos) = line.find(" of ") {
        let n_str = line[..pos].trim().split_whitespace().last().unwrap_or("");
        let m_str = line[pos + 4..].trim().split_whitespace().next().unwrap_or("");
        if let (Ok(n), Ok(m)) = (n_str.parse::<f64>(), m_str.parse::<f64>()) {
            if m > 0.0 {
                return Some(((n / m) * 100.0).min(100.0) as i32);
            }
        }
    }
    None
}

/// Extract "owner/project" from a COPR repo id like
/// "copr:copr.fedorainfracloud.org:owner:project".
fn repo_owner_project(id: &str) -> String {
    let mut parts = id.splitn(4, ':');
    let _prefix = parts.next();
    let _host = parts.next();
    match (parts.next(), parts.next()) {
        (Some(owner), Some(project)) => format!("{}/{}", owner, project),
        _ => String::new(),
    }
}
