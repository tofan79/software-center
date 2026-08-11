# Software Center

Modern app store for Fedora and derivatives, written in Rust with a Qt6/QML frontend and a background tray daemon.

[![Release](https://img.shields.io/github/v/release/tofan79/software-center?sort=semver)](https://github.com/tofan79/software-center/releases)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)
[![COPR](https://img.shields.io/badge/COPR-mindset%2FMindset--Apps-brightgreen)](https://copr.fedorainfracloud.org/coprs/mindset/Mindset-Apps/)

> Derived from [rakuos-software](https://gitlab.com/rakuos/packages/rakuos/rakuos-software)
> (GPL-3.0-or-later). Stripped retired features (webapps, Plasma/KNS add-ons,
> distrobox, bootc, firmware, reviews), rebranded as software-center, and
> repackaged for COPR.

## Features

| Area | Description |
|------|-------------|
| **Flatpak** | Browse, install, update, and remove apps from Flathub (system & user scope) |
| **RPM packages** | Search across all enabled DNF repos (Fedora, COPR, Terra, RPM Fusion, …) and install native packages |
| **AppImages** | Download, verify (ELF/arch), and run AppImages with atomic update + rollback |
| **System updates** | Check for and install system updates from the UI or the tray |
| **Tray daemon** | `software-center-tray` — background update checks with the count shown in the system tray |
| **Search & browse** | Cross-repo search, categories, per-source install (DNF / Flathub System / Flathub User) |
| **Maintenance** | List unused packages, clear DNF cache, manage repositories & COPRs |

## Install from COPR

```bash
sudo dnf copr enable mindset/Mindset-Apps
sudo dnf install software-center
```

## Build from source

Requirements: Rust, Cargo, and Qt6 dev packages (`qt6-qtbase-devel`,
`qt6-qtdeclarative-devel`).

```bash
cargo build --release
```

Run the UI from the source tree:

```bash
RAKUOS_SOFTWARE_QML_DIR=$PWD/crates/ui-qt/qml ./target/release/software-center
```

When packaged, the QML frontend is installed to `/usr/share/software-center/qml`.

### Workspace layout

```
crates/
├── backend/
│   ├── appimages    # AppImage fetch, verify, update, rollback
│   ├── appstream    # AppStream catalog parsing + icon resolution
│   ├── flatpak      # Flatpak operations (install/remove/update/remotes)
│   ├── home         # Home-page curated content (picks, popular, new)
│   ├── packages     # DNF/RPM queries, repoquery cache, install/remove
│   └── updates      # check-update, unused packages, repo management
├── daemon           # system-tray update-check daemon (software-center-tray)
└── ui-qt            # Qt6/QML frontend (software-center)
```

## Packaging & releases

RPMs are built automatically from tagged releases via the
[Mindset-Apps](https://github.com/tofan79/Mindset-Apps) COPR workflows.

To release a new version:

1. Bump `version` in `Cargo.toml` and add a `%changelog` entry in
   `resources/software-center.spec`.
2. Commit, tag (`v1.0.x`), and push.
3. Create a GitHub release for the tag.
4. Trigger the `software-center` workflow in
   [Mindset-Apps](https://github.com/tofan79/Mindset-Apps/actions) to build the
   COPR RPM.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
