// software-center — Qt6/QML software center frontend

mod backend;
use backend::SoftwareBackend;
use qmetaobject::{QmlEngine, QString};

// Qt6 static helpers previously provided by qt_helpers.cpp. libQt6Core and
// libQt6Gui are already linked via qmetaobject, so we call the stable mangled
// C++ symbols directly — no cc/C++ build step needed.
extern "C" {
    #[link_name = "_ZN16QCoreApplication14addLibraryPathERK7QString"]
    fn qt_add_library_path(path: *const QString);
    #[link_name = "_ZN15QGuiApplication18setDesktopFileNameERK7QString"]
    fn qt_set_desktop_file_name(name: *const QString);
}

fn set_qt_app_properties() {
    unsafe {
        // Ensure Qt can find image format plugins (WebP, AVIF, etc.)
        qt_add_library_path(&QString::from("/usr/lib64/qt6/plugins"));
        qt_add_library_path(&QString::from("/usr/lib/qt6/plugins"));
        // Associate the window with its desktop entry (taskbar grouping/icon).
        qt_set_desktop_file_name(&QString::from("software-center"));
    }
}

pub fn pid_file() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    std::path::PathBuf::from(home).join(".cache/software-center/software-ui.pid")
}

fn show_flag_path() -> std::path::PathBuf {
    std::env::temp_dir().join("software-center-show")
}

/// Returns true if a prior instance is already running (PID file + /proc check).
fn instance_already_running() -> bool {
    let Some(pid) = std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    else {
        return false;
    };
    // Verify the PID actually belongs to our process, not a recycled PID
    let cmdline = std::fs::read_to_string(format!("/proc/{}/cmdline", pid))
        .unwrap_or_default();
    cmdline.contains("software-center")
}

fn write_pid_file() {
    let path = pid_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
}

/// Locate the daemon binary: prefer a sibling in the same directory as this
/// binary (covers dev builds in target/debug/), fall back to PATH.
fn daemon_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .join("software-center-tray");
        if sibling.exists() {
            return sibling;
        }
    }
    std::path::PathBuf::from("software-center-tray")
}

fn ensure_daemon_running() {
    // pgrep -x fails for names >15 chars on Linux; use -f to match full cmdline.
    // Anchor at end-of-line so shells whose command line merely mentions the
    // binary name are not mistaken for a running daemon, and suppress stdout
    // (matching PIDs) so it doesn't leak into our own log output.
    let running = std::process::Command::new("pgrep")
        .args(["-f", "software-center-tray$"])
        .stdout(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !running {
        log::info!("Starting software-center-tray daemon...");
        let _ = std::process::Command::new(daemon_binary()).spawn();
    }
}

fn main() {
    env_logger::init();
    std::env::set_var("QML_XHR_ALLOW_FILE_READ", "1");

    // Collect any file path passed via MIME type association (%f in .desktop)
    // Skip "--tray" and other flag arguments; take the first non-flag arg.
    // Must be parsed BEFORE the single-instance check so we can forward it.
    let startup_file: Option<String> = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'));

    let startup_flag = std::env::temp_dir().join("software-center-open-file");

    // Single-instance guard: if already running, signal it to show (and open
    // the file if one was passed) then exit.
    if instance_already_running() {
        if let Some(ref path) = startup_file {
            let _ = std::fs::write(&startup_flag, path);
        }
        let _ = std::fs::write(show_flag_path(), "1");
        return;
    }

    write_pid_file();
    ensure_daemon_running();

    // If launched with --tray, write a flag so QML starts the window hidden.
    if std::env::args().any(|a| a == "--tray") {
        let _ = std::fs::write(
            std::env::temp_dir().join("software-center-start-hidden"),
            "1",
        );
    }

    qmetaobject::qml_register_type::<SoftwareBackend>(
        c"org.softwarecenter.Software",
        1, 0,
        c"SoftwareBackend",
    );

    // Write the startup file path to a temp file so QML can read it at startup
    // (QmlEngine doesn't support passing context properties easily before load).
    if let Some(ref path) = startup_file {
        let _ = std::fs::write(&startup_flag, path);
    } else {
        let _ = std::fs::remove_file(&startup_flag);
    }

    let mut engine = QmlEngine::new();
    set_qt_app_properties();
    // Route Qt/QML messages (warnings, errors, console.log) through env_logger
    // so they surface in the app log instead of vanishing on some setups.
    qmetaobject::init_qt_to_rust();

    let qml_dir = std::env::var("RAKUOS_SOFTWARE_QML_DIR")
        .unwrap_or_else(|_| "/usr/share/software-center/qml".to_string());
    let qml_dir = std::fs::canonicalize(&qml_dir)
        .unwrap_or_else(|_| std::path::PathBuf::from(&qml_dir));
    engine.load_file(format!("file://{}/main.qml", qml_dir.display()).into());
    engine.exec();

    // Clean up on exit
    let _ = std::fs::remove_file(pid_file());
}
