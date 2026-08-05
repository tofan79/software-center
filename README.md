# software-center

A modern app store for Fedora and derivatives, built with Rust + Qt6/QML.

## Features

- **Flatpak** — browse, install, update, and remove apps from Flathub
- **RPM packages** — install and manage native packages via DNF5
- **AppImages** — download and run AppImages
- **System updates** — check and install system updates
- **Tray daemon** — background daemon (`software-center-tray`) that checks for
  updates and shows the update count in the system tray

## Build

Requires Rust, Cargo, Qt6 (qtbase + qtdeclarative) development packages.

```bash
cargo build --release
```

Run the UI:

```bash
RAKUOS_SOFTWARE_QML_DIR=$PWD/crates/ui-qt/qml ./target/release/software-center
```

The QML frontend is installed to `/usr/share/software-center/qml` when packaged.

## Install from COPR

```bash
sudo dnf copr enable mindset/Mindset-Apps
sudo dnf install software-center
```

## Packaging

RPMs are built automatically from tagged releases via the
[Mindset-Apps](https://github.com/tofan79/Mindset-Apps) COPR workflows.
Create a new tag + release (`v1.0.0`, `v1.0.1`, ...) to trigger a rebuild.

## License

GPL-3.0-or-later
