// daemon/tray.rs — System tray via D-Bus StatusNotifierItem (ksni)
// Works on KDE Plasma, GNOME (with AppIndicator extension), COSMIC.

use ksni::Tray;
use std::sync::mpsc;

use crate::DaemonMsg;

// ── Status ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub enum TrayStatus {
    #[default]
    Checking,
    UpToDate,
    Available(usize),
    RebootRequired,
}

// ── DE detection ──────────────────────────────────────────────────────────────

/// COSMIC is detected by the presence of its app-list config file rather than
/// relying on XDG_CURRENT_DESKTOP, which may not be set in all launch paths.
fn is_cosmic() -> bool {
    std::path::Path::new(
        "/usr/share/cosmic/com.system76.CosmicAppList/v1/favorites"
    ).exists()
}

// ── Badge drawing ─────────────────────────────────────────────────────────────

const BADGE_BASE_PNGS: &[(u32, &str)] = &[
    (48, "/usr/share/icons/AdwaitaLegacy/48x48/legacy/system-software-update.png"),
    (32, "/usr/share/icons/AdwaitaLegacy/32x32/legacy/system-software-update.png"),
    (22, "/usr/share/icons/AdwaitaLegacy/22x22/legacy/system-software-update.png"),
];

#[rustfmt::skip]
const DIGIT_FONT: [[u8; 15]; 11] = [
    [1,1,1, 1,0,1, 1,0,1, 1,0,1, 1,1,1], // 0
    [0,1,0, 1,1,0, 0,1,0, 0,1,0, 1,1,1], // 1
    [1,1,1, 0,0,1, 1,1,1, 1,0,0, 1,1,1], // 2
    [1,1,1, 0,0,1, 0,1,1, 0,0,1, 1,1,1], // 3
    [1,0,1, 1,0,1, 1,1,1, 0,0,1, 0,0,1], // 4
    [1,1,1, 1,0,0, 1,1,1, 0,0,1, 1,1,1], // 5
    [1,1,1, 1,0,0, 1,1,1, 1,0,1, 1,1,1], // 6
    [1,1,1, 0,0,1, 0,1,0, 0,1,0, 0,1,0], // 7
    [1,1,1, 1,0,1, 1,1,1, 1,0,1, 1,1,1], // 8
    [1,1,1, 1,0,1, 1,1,1, 0,0,1, 1,1,1], // 9
    [0,1,0, 0,1,0, 0,1,0, 0,0,0, 0,1,0], // !
];

fn fill_circle(img: &mut image::RgbaImage, cx: i32, cy: i32, r: i32, color: [u8; 4]) {
    let (w, h) = img.dimensions();
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                let x = cx + dx; let y = cy + dy;
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    img.put_pixel(x as u32, y as u32, image::Rgba(color));
                }
            }
        }
    }
}

fn draw_digit(img: &mut image::RgbaImage, digit_idx: usize, cx: i32, cy: i32, color: [u8; 4]) {
    let bitmap = &DIGIT_FONT[digit_idx.min(10)];
    for row in 0..5i32 {
        for col in 0..3i32 {
            if bitmap[(row * 3 + col) as usize] == 1 {
                let x = cx - 1 + col; let y = cy - 2 + row;
                let (w, h) = img.dimensions();
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                    img.put_pixel(x as u32, y as u32, image::Rgba(color));
                }
            }
        }
    }
}

fn make_badge_icons(count: usize) -> Vec<ksni::Icon> {
    let mut icons = Vec::new();
    for (_size, png_path) in BADGE_BASE_PNGS {
        let Some(data) = std::fs::read(png_path).ok() else { continue };
        let Some(mut img) = image::load_from_memory_with_format(&data, image::ImageFormat::Png)
            .ok().map(|i| i.to_rgba8()) else { continue };
        let (w, _) = img.dimensions();
        let r  = (w as i32 / 5).max(4);
        let cx = w as i32 - r - 1;
        let cy = r + 1;
        fill_circle(&mut img, cx, cy, r, [227, 57, 53, 255]);
        draw_digit(&mut img, if count < 10 { count } else { 10 }, cx, cy, [255, 255, 255, 255]);
        let (w, h) = img.dimensions();
        let data: Vec<u8> = img.into_raw().chunks(4)
            .flat_map(|c| [c[3], c[0], c[1], c[2]]).collect();
        icons.push(ksni::Icon { width: w as i32, height: h as i32, data });
    }
    icons
}


// ── Tray ─────────────────────────────────────────────────────────────────────

pub struct SoftwareTray {
    pub status: TrayStatus,
    pub tx: mpsc::SyncSender<DaemonMsg>,
}

impl Tray for SoftwareTray {
    fn id(&self) -> String { "org.softwarecenter.Software".into() }

    fn category(&self) -> ksni::Category { ksni::Category::ApplicationStatus }

    fn icon_name(&self) -> String {
        match &self.status {
            TrayStatus::UpToDate      => "emblem-ok-symbolic",
            TrayStatus::Available(_)  => "software-update-available-symbolic",
            TrayStatus::RebootRequired => "system-reboot-symbolic",
            _                         => "process-stop-symbolic",
        }.into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        match &self.status {
            TrayStatus::Available(count) => make_badge_icons(*count),
            _                            => Vec::new(),
        }
    }

    fn title(&self) -> String {
        match &self.status {
            TrayStatus::Available(count) => format!(
                "Software Center — {} update{} available",
                count, if *count != 1 { "s" } else { "" }
            ),
            TrayStatus::RebootRequired => "Software Center — Reboot Required".into(),
            _ => "Software Center".into(),
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: "system-software-update".into(),
            icon_pixmap: Vec::new(),
            title: self.title(),
            description: String::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if is_cosmic() {
            let _ = self.tx.send(DaemonMsg::OpenUi);
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Open Software Center".into(),
                icon_name: "system-software-install".into(),
                activate: Box::new(|t: &mut Self| { let _ = t.tx.send(DaemonMsg::OpenUi); }),
                ..Default::default()
            }.into(),
            StandardItem {
                label: "Check for Updates".into(),
                icon_name: "view-refresh".into(),
                activate: Box::new(|t: &mut Self| { let _ = t.tx.send(DaemonMsg::CheckNow); }),
                ..Default::default()
            }.into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| { let _ = t.tx.send(DaemonMsg::Quit); }),
                ..Default::default()
            }.into(),
        ]
    }
}

pub fn spawn(tx: mpsc::SyncSender<DaemonMsg>) -> anyhow::Result<ksni::Handle<SoftwareTray>> {
    let service = ksni::TrayService::new(SoftwareTray { status: TrayStatus::Checking, tx });
    let handle = service.handle();
    service.spawn();
    Ok(handle)
}
