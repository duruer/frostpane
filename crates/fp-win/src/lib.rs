//! Windows platform services. Not part of the coverage gate.
//!
//! Spec: core-app 2.2 (single instance + second-launch event), 2.5 battery
//! read, 2.6 (taskbar theme, tray icon spot), 2.10 (launch at login, opt-in),
//! 2.13 (data folder for running.json / crash log); desktop-native 2 + 3.6
//! (monitor enumeration, DPI, display ids via QueryDisplayConfig), 3.5.1
//! (WinEvent hooks), 3.5.3 (raw attributes -> `fp_tracker::WindowInfo`),
//! 3.5.6 (taskbar shelves), 3.7 (session/suspend/display-off notifications,
//! battery saver, reduced motion), 3.10 helpers (cursor, root window),
//! 3.12 (IDesktopWallpaper), weather-sources 2.5 (time-zone id, GeoID latitude).
//!
//! [`Platform`] is the seam the app talks to; [`windows_impl::Win`] is the
//! only implementation today.

use fp_core::clock::{Clock, WallTime};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use std::path::{Path, PathBuf};

pub mod logic;
#[cfg(windows)]
pub mod windows_impl;

pub use logic::{SleepState, SystemEvent};

/// Battery facts from the OS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerStatus {
    pub on_battery: bool,
    pub has_battery: bool,
    /// Battery saver or on battery (desktop-native 3.7 `SavingPower`).
    pub saving: bool,
}

/// OS services used by the app. Everything here is called on the UI thread.
pub trait Platform {
    fn monitors(&self) -> Vec<MonitorInfo>;
    /// `%LOCALAPPDATA%\Frostpane`.
    fn data_dir(&self) -> PathBuf;
    /// Two-letter UI language code.
    fn os_language(&self) -> String;
    fn power(&self) -> PowerStatus;
    fn reduced_motion(&self) -> bool;
    /// Cursor in virtual-desktop pixels.
    fn cursor(&self) -> Option<(i32, i32)>;
    fn left_button_down(&self) -> bool;
    fn right_button_down(&self) -> bool;
    /// One-item context menu at `at` (virtual-desktop pixels). Blocks until it
    /// closes; true when the item was chosen. A new menu is built each call.
    fn context_menu(&self, label: &str, at: (i32, i32)) -> bool;
    /// Top-level windows in z-order, raw attributes for `fp_tracker::filter`.
    fn windows(&self) -> Vec<fp_tracker::WindowInfo>;
    /// Bottom, non-auto-hide taskbar rects in pixels.
    fn taskbar_shelves(&self) -> Vec<PixelRect>;
    /// Windows time-zone id (e.g. "W. Europe Standard Time").
    fn time_zone_id(&self) -> String;
    /// Corner radius of app windows (8 on Windows 11, else 0).
    fn window_corner(&self) -> f64;
}

/// Monotonic + wall clock from the OS.
pub struct SystemClock {
    start: std::time::Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self { start: std::time::Instant::now() }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    fn wall(&self) -> WallTime {
        let unix =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        WallTime::new(unix, utc_offset_s())
    }
}

#[cfg(windows)]
fn utc_offset_s() -> i32 {
    windows_impl::utc_offset_s()
}

#[cfg(not(windows))]
fn utc_offset_s() -> i32 {
    0
}

/// One monitor as seen by the wallpaper service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallpaperScreen {
    pub id: String,
    pub width: u32,
    pub height: u32,
}

/// Shell integration used by the UI and the app. Errors are swallowed; the
/// return values say what happened (core-app 2.10, 2.15, 2.6; desktop-native 3.12;
/// weather-sources 2.5).
pub trait Shell {
    /// Opens `https:` and `ms-settings:` links; everything else is ignored.
    fn open_url(&self, url: &str);
    /// Run value present and not disabled in Windows.
    fn login_is_on(&self) -> bool;
    /// Disabled by the user or policy in Windows settings.
    fn login_blocked(&self) -> bool;
    /// Turns launch-at-login on or off. `false` means Windows blocks it.
    fn login_set(&self, on: bool) -> bool;
    /// Opens the Windows startup-apps settings page.
    fn show_startup_apps(&self);
    /// The taskbar uses the light theme (tray icon should be dark).
    fn taskbar_light(&self) -> bool;
    /// Windows shows this exe's tray icon on the taskbar, not in the overflow
    /// (core-app 2.6 `IconOnTaskbar`). Defaults to true when unknown.
    fn tray_icon_promoted(&self) -> bool;
    /// `rect` when it lies inside the `Shell_TrayWnd` rect, else `None`
    /// (core-app 2.6 `IconSpot`).
    fn tray_icon_spot(&self, rect: fp_core::geom::PixelRect) -> Option<fp_core::geom::PixelRect>;
    /// Nation-level latitude of the user's region, if known.
    fn region_latitude(&self) -> Option<f64>;
    fn wallpaper_screens(&self) -> Vec<WallpaperScreen>;
    /// Same wallpaper on every monitor.
    fn wallpaper_set(&self, path: &Path) -> bool;
    /// `(monitor id, file)` pairs.
    fn wallpaper_set_each(&self, items: &[(String, PathBuf)]) -> bool;
    fn wallpaper_current(&self) -> Option<PathBuf>;
}

/// Copies `src` into `<dir>/wallpapers/` under a fresh name so the OS never
/// holds the original (day-and-night switching). Returns the new path.
pub fn stage_wallpaper(dir: &Path, src: &Path, tag: &str) -> std::io::Result<PathBuf> {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let folder = dir.join("wallpapers");
    std::fs::create_dir_all(&folder)?;
    let safe: String = tag.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    let safe = if safe.is_empty() { "wallpaper".to_string() } else { safe };
    let dst = folder.join(format!("{safe}.{ext}"));
    std::fs::copy(src, &dst)?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_runs() {
        let c = SystemClock::default();
        assert!(c.now() >= 0.0);
        assert!(c.wall().unix > 1_600_000_000);
    }

    #[test]
    fn clock_is_monotonic() {
        let c = SystemClock::default();
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }

    #[test]
    fn stage_wallpaper_copies() {
        let dir = std::env::temp_dir().join(format!("fp-win-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.jpg");
        std::fs::write(&src, b"x").unwrap();
        let out = stage_wallpaper(&dir, &src, "day/1").unwrap();
        assert_eq!(out.file_name().and_then(|n| n.to_str()), Some("day1.jpg"));
        assert!(out.parent().unwrap().ends_with("wallpapers"));
        assert_eq!(std::fs::read(&out).unwrap(), b"x");
        assert!(stage_wallpaper(&dir, &dir.join("missing.png"), "n").is_err());
        // A tag with nothing usable still gives a visible file name.
        let out = stage_wallpaper(&dir, &src, "../..").unwrap();
        assert_eq!(out.file_name().and_then(|n| n.to_str()), Some("wallpaper.jpg"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
