// scenter-updates — System update management via dnf5
// Traditional (non-atomic) Fedora: package updates through dnf5 only.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemStatus {
    pub os: String,
    pub version: String,
    pub kernel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateInfo {
    pub available: bool,
    /// True when a package upgrade is currently running.
    pub upgrade_running: bool,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Get basic system info: OS release name/version and running kernel.
/// Replaces the bootc image status of the original RakuOS code.
pub fn get_system_status() -> SystemStatus {
    let read_os_release = |key: &str| -> String {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|c| {
                c.lines()
                    .find(|l| l.starts_with(&format!("{key}=")))
                    .and_then(|l| l.split_once('=').map(|(_, v)| v.trim_matches('"').to_string()))
            })
            .unwrap_or_default()
    };
    let kernel = Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    SystemStatus {
        os: read_os_release("NAME"),
        version: read_os_release("VERSION"),
        kernel,
        error: None,
    }
}

/// Returns true if a dnf5 operation is currently running (dnf5 process alive).
pub fn is_upgrade_running() -> bool {
    let out = Command::new("pgrep")
        .arg("dnf5")
        .output()
        .ok();
    match out {
        Some(o) if o.status.success() => {
            // pgrep succeeded → at least one matching process found.
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .any(|pid| pid != std::process::id().to_string())
        }
        _ => false,
    }
}

/// Check for available package updates via `dnf5 check-update`.
/// Read-only — runs without root. Returns a Vec of raw JSON values, one per
/// updatable package: {"name", "current_version", "available_version", "repo"}.
pub fn check_packages_script() -> Vec<serde_json::Value> {
    let installed_map = match installed_evr_map() {
        Some(m) => m,
        None => return Vec::new(),
    };

    let out = Command::new("dnf5")
        .args(["-q", "check-update"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    // Exit 0 = updates available, 1 = none. Anything else is a real error.
    if !matches!(out.status.code(), Some(0) | Some(1)) {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let mut updates = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 {
            continue;
        }
        let name_arch = cols[0];
        let new_evr = cols[1];
        let repo = cols[2];
        if repo == "installed" || !new_evr.contains('-') {
            continue;
        }
        let name = strip_arch(name_arch);
        if name.is_empty() {
            continue;
        }
        updates.push(serde_json::json!({
            "name": name,
            "current_version": installed_map.get(name).cloned().unwrap_or_default(),
            "available_version": new_evr,
            "repo": repo,
        }));
    }

    updates.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    updates
}

/// Stream output from `pkexec dnf5 upgrade -y`.
/// Yields log lines then "__done__<exit_code>".
pub fn upgrade_packages_stream() -> impl Iterator<Item = String> {
    run_stream_owned(vec![
        "pkexec".into(),
        "dnf5".into(),
        "upgrade".into(),
        "-y".into(),
    ])
}

/// Stream output from upgrading a single package by name.
pub fn upgrade_single_package_stream(name: &str) -> impl Iterator<Item = String> {
    run_stream_owned(vec![
        "pkexec".into(),
        "dnf5".into(),
        "upgrade".into(),
        "-y".into(),
        name.to_string(),
    ])
}

/// Schedule a system reboot. Returns (success, error_message).
pub fn schedule_reboot() -> (bool, String) {
    match Command::new("systemctl").arg("reboot").output() {
        Ok(o) if o.status.success() => (true, String::new()),
        Ok(o) => (false, String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => (false, e.to_string()),
    }
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Build a name → EVR map of all installed packages from `dnf5 -q list installed`.
fn installed_evr_map() -> Option<HashMap<String, String>> {
    let out = Command::new("dnf5")
        .args(["-q", "list", "installed"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut map = HashMap::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 {
            continue;
        }
        let name = strip_arch(cols[0]);
        if name.is_empty() {
            continue;
        }
        map.insert(name.to_string(), cols[1].to_string());
    }
    Some(map)
}

/// Strip the architecture suffix from a "name.arch" string. Accepts multi-dot
/// names (e.g. "python3.12-tkinter.x86_64" → "python3.12-tkinter").
fn strip_arch(name_arch: &str) -> &str {
    let known = ["x86_64", "i686", "aarch64", "armv7hl", "armv6hl", "ppc64le", "s390x", "noarch", "i386"];
    for arch in known {
        if let Some(base) = name_arch.strip_suffix(&format!(".{arch}")) {
            return base;
        }
    }
    name_arch
}

// ── AppStream enrichment for package update lists ─────────────────────────────

/// Enrich a list of raw package update JSON objects (from `check_packages_script`)
/// with icon_path, icon_url, and display_name sourced from the AppStream catalog
/// and the installed-packages list — the same sources used by the app catalog page.
pub fn enrich_package_updates(raw: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let appstream = scenter_appstream::get_appstream();
    let mut meta: HashMap<String, (String, String, String)> = HashMap::new();

    // Seed from installed-packages list — already has fully resolved icons/names.
    for app in scenter_packages::get_installed().unwrap_or_default() {
        insert_pkg_meta(&mut meta, &app.package_name, &app.icon_path, &app.icon_url, &app.name);
        insert_pkg_meta(&mut meta, &app.id, &app.icon_path, &app.icon_url, &app.name);
    }

    // Supplement/override with the full AppStream catalog keyed by package_name + id.
    for app in appstream.values().filter(|a| !a.package_name.is_empty()) {
        let friendly = if !app.name.is_empty() && app.name != app.package_name {
            app.name.clone()
        } else {
            String::new()
        };
        insert_pkg_meta(&mut meta, &app.package_name, &app.icon_path, &app.icon_url, &friendly);
        insert_pkg_meta(&mut meta, &app.id, &app.icon_path, &app.icon_url, &friendly);
    }

    raw.into_iter()
        .map(|mut update| {
            // A package is treated as a GUI "Application" (vs. a library/system
            // dependency) if it has AppStream/desktop-file metadata.
            let mut is_gui = false;
            if let Some(name) = update["name"].as_str().map(ToString::to_string) {
                if let Some((icon_path, icon_url, display_name)) = meta.get(&name) {
                    is_gui = true;
                    if !icon_path.is_empty() {
                        update["icon_path"] = icon_path.clone().into();
                    }
                    if !icon_url.is_empty() {
                        update["icon_url"] = icon_url.clone().into();
                    }
                    if !display_name.is_empty() {
                        update["display_name"] = display_name.clone().into();
                    }
                }
            }
            update["gui"] = is_gui.into();
            update
        })
        .collect()
}

fn insert_pkg_meta(
    map: &mut HashMap<String, (String, String, String)>,
    key: &str,
    icon_path: &str,
    icon_url: &str,
    display_name: &str,
) {
    if key.is_empty() {
        return;
    }
    let entry = map
        .entry(key.to_string())
        .or_insert_with(|| (icon_path.to_string(), icon_url.to_string(), display_name.to_string()));
    if entry.0.is_empty() && entry.1.is_empty() {
        entry.0 = icon_path.to_string();
        entry.1 = icon_url.to_string();
    } else if entry.0.is_empty() && !icon_path.is_empty() {
        entry.0 = icon_path.to_string();
    }
    if !display_name.is_empty() && display_name != key {
        if entry.2.is_empty() || entry.2 == key {
            entry.2 = display_name.to_string();
        }
    }
}

// ── Shared cache helpers ─────────────────────────────────────────────────────

pub fn daemon_cache_path() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".cache/software-center/daemon-update-cache.json")
}

/// Remove a completed update entry from the cache, then clear the cache file
/// entirely if no updates remain. Call after each individual update succeeds.
///
/// `section` is the top-level JSON array key ("packages", "flatpak", ...).
/// `id_key` is the field within each entry that holds the item identity
/// ("name" for packages, "app_id" for flatpak, etc.). `id_value` is the actual
/// value to match.
pub fn prune_cache_entry(section: &str, id_key: &str, id_value: &str) {
    let path = daemon_cache_path();
    let Ok(raw) = std::fs::read_to_string(&path) else { return };
    let Ok(mut cache) = serde_json::from_str::<serde_json::Value>(&raw) else { return };

    if let Some(arr) = cache.get_mut(section).and_then(|v| v.as_array_mut()) {
        arr.retain(|entry| {
            entry.get(id_key)
                .and_then(|v| v.as_str())
                .map(|v| v != id_value)
                .unwrap_or(true)
        });
    }

    // Recalculate total.
    let total: usize = ["packages", "flatpak", "appimages"]
        .iter()
        .map(|k| cache[k].as_array().map(|a| a.len()).unwrap_or(0))
        .sum::<usize>();

    if total == 0 {
        // No updates left — delete the cache so the page shows "up to date".
        let _ = std::fs::remove_file(&path);
        return;
    }

    cache["total"] = serde_json::json!(total);
    if let Ok(out) = serde_json::to_string_pretty(&cache) {
        let _ = std::fs::write(&path, out);
    }
}

/// Clear the cache file completely (e.g. after "Update All" completes).
pub fn clear_updates_cache() {
    let _ = std::fs::remove_file(daemon_cache_path());
}

// ── PTY streaming ─────────────────────────────────────────────────────────────

/// Strip ANSI/VT100 escape sequences from raw bytes so PTY output is clean text.
fn strip_ansi(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'\x1b' {
            i += 1;
            if i >= input.len() { break; }
            match input[i] {
                b'[' => {
                    // CSI sequence: ESC [ <params> <letter>
                    i += 1;
                    while i < input.len() && !input[i].is_ascii_alphabetic() { i += 1; }
                    if i < input.len() { i += 1; }
                }
                b']' => {
                    // OSC sequence: ESC ] ... BEL  or  ESC ] ... ESC backslash
                    i += 1;
                    while i < input.len() {
                        if input[i] == b'\x07' { i += 1; break; }
                        if input[i] == b'\x1b' && i + 1 < input.len() && input[i + 1] == b'\\' {
                            i += 2; break;
                        }
                        i += 1;
                    }
                }
                _ => { i += 1; }
            }
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

/// Drain `reader`, stripping ANSI codes and splitting on `\r` or `\n`.
/// Handles EIO (errno 5) as normal EOF so PTY master reads terminate cleanly.
fn drain_reader<R: std::io::Read>(reader: R, tx: std::sync::mpsc::Sender<String>, strip: bool) {
    let mut reader = reader;
    let mut buf = [0u8; 4096];
    let mut pending = String::new();

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let bytes = if strip { strip_ansi(&buf[..n]) } else { buf[..n].to_vec() };
                pending.push_str(&String::from_utf8_lossy(&bytes));
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
                pending = pending[start..].to_string();
            }
            Err(e) if e.raw_os_error() == Some(libc::EIO) => break, // PTY slave closed
            Err(_) => break,
        }
    }

    let seg = pending.trim();
    if !seg.is_empty() {
        let _ = tx.send(seg.to_string());
    }
}

/// Spawn `cmd` inside a PTY so the child process sees a real terminal (isatty → true).
/// Falls back to paired pipes if PTY allocation fails.
fn run_stream_owned(cmd: Vec<String>) -> impl Iterator<Item = String> {
    use std::os::unix::io::FromRawFd;
    use std::process::Stdio;
    use std::sync::mpsc;

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
            // Dup slave for stdout and stderr; stdin gets the original fd.
            let slave_out = unsafe { libc::dup(slave_raw) };
            let slave_err = unsafe { libc::dup(slave_raw) };

            if slave_out >= 0 && slave_err >= 0 {
                use std::os::unix::process::CommandExt;
                let mut builder = Command::new(&cmd[0]);
                builder.args(&cmd[1..]);
                unsafe {
                    builder
                        .stdin(Stdio::from_raw_fd(slave_raw))
                        .stdout(Stdio::from_raw_fd(slave_out))
                        .stderr(Stdio::from_raw_fd(slave_err))
                        // New session so PTY slave becomes the controlling terminal.
                        .pre_exec(|| {
                            libc::setsid();
                            Ok(())
                        });
                }

                match builder.spawn() {
                    Ok(mut child) => {
                        // Close slave fds in the parent — the child has its own copies.
                        // If we leave them open the PTY master never gets EIO when the
                        // child exits, so drain_reader blocks forever.
                        unsafe {
                            libc::close(slave_raw);
                            libc::close(slave_out);
                            libc::close(slave_err);
                        }
                        let master = unsafe { std::fs::File::from_raw_fd(master_raw) };
                        let tx2 = tx.clone();
                        let reader = std::thread::spawn(move || drain_reader(master, tx2, true));
                        let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
                        reader.join().ok();
                        let _ = tx.send(format!("__done__{}", code));
                        return;
                    }
                    Err(e) => {
                        unsafe { libc::close(master_raw); }
                        let _ = tx.send(format!("Error: {e}"));
                        let _ = tx.send("__done__1".to_string());
                        return;
                    }
                }
            } else {
                unsafe {
                    if slave_out >= 0 { libc::close(slave_out); }
                    if slave_err >= 0 { libc::close(slave_err); }
                    libc::close(slave_raw);
                    libc::close(master_raw);
                }
            }
        }

        // ── Pipe fallback (PTY unavailable) ──────────────────────────────────
        let mut child = match Command::new(&cmd[0])
            .args(&cmd[1..])
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
        let t1 = std::thread::spawn(move || drain_reader(stdout, tx_out, false));
        let tx_err = tx.clone();
        let t2 = std::thread::spawn(move || drain_reader(stderr, tx_err, false));
        t1.join().ok();
        t2.join().ok();
        let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
        let _ = tx.send(format!("__done__{}", code));
    });

    rx.into_iter()
}
