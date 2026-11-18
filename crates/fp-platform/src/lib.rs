//! Platform seam of Frostpane: the traits every OS backend implements, the
//! types they share and the platform-free logic around them. No OS calls.
//!
//! Spec: core-app 2.2 (single instance + second-launch event), 2.5 battery
//! read, 2.6 (taskbar theme, tray icon spot), 2.10 (launch at login, opt-in),
//! 2.13 (data folder for running.json / crash log); desktop-native 2 + 3.6
//! (monitor enumeration, DPI), 3.5.1 (window hooks), 3.5.3 (raw attributes ->
//! `fp_tracker::WindowInfo`), 3.5.6 (taskbar shelves), 3.7 (session/suspend/
//! display-off notifications, battery saver, reduced motion), 3.10 helpers
//! (cursor, root window), 3.12 (wallpaper), weather-sources 2.5 (time-zone id,
//! region latitude).
//!
//! [`Platform`] and [`Shell`] are the seams the app talks to; [`SystemEvents`],
//! [`Driver`] and [`InstanceGuard`] cover the UI-thread services;
//! [`overlay::OverlayBackend`] and [`sprites::SpriteBackend`] are the window
//! backends of `fp-overlay` and `fp-layered`. `fp-win` implements all of them
//! for Windows; [`null`] holds the stand-ins for platforms without a backend.

use fp_core::clock::{Clock, WallTime};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_tracker::HookEvent;
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

pub mod coords;
pub mod events;
pub mod null;
pub mod overlay;
pub mod select;
pub mod sprites;

pub use events::{SleepState, SystemEvent};

/// wgpu backends of this target: DX12 on Windows, Metal on macOS, Vulkan
/// with a GL fallback elsewhere.
pub const NATIVE_BACKENDS: wgpu::Backends = if cfg!(windows) {
    wgpu::Backends::DX12
} else if cfg!(target_os = "macos") {
    wgpu::Backends::METAL
} else {
    wgpu::Backends::VULKAN.union(wgpu::Backends::GL)
};

/// A guard that keeps an OS resource alive until it is dropped (timers,
/// hooks, notification registrations).
pub type Guard = Box<dyn Any>;

/// Everything the app needs from one object on the UI thread.
pub trait Services: Platform + Shell + SystemEvents + Driver {}

impl<T: Platform + Shell + SystemEvents + Driver> Services for T {}

/// What a session cannot do (shown as notices; porting design section 7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// No weather overlay.
    pub no_overlay: bool,
    /// No foreign window geometry: snow only on the ground and taskbar.
    pub no_window_geometry: bool,
    /// Only some foreign windows are seen (X11 clients under XWayland).
    pub foreign_windows_partial: bool,
    /// No native context menu (use an egui card).
    pub no_native_menu: bool,
    /// No global cursor or mouse buttons.
    pub no_global_cursor: bool,
    /// Cards cannot be placed or kept on top.
    pub no_card_placement: bool,
}

impl Limits {
    /// A session served by the [`null`] backends.
    pub const NULL: Limits = Limits {
        no_overlay: true,
        no_window_geometry: true,
        foreign_windows_partial: false,
        no_native_menu: true,
        no_global_cursor: true,
        no_card_placement: false,
    };
}

/// One platform's backends, as a backend crate hands them to the app.
pub struct Parts<O: overlay::OverlayBackend> {
    pub services: std::rc::Rc<dyn Services>,
    pub sprites: Box<dyn sprites::SpriteBackend>,
    pub overlay: O,
    pub limits: Limits,
}

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
    /// Process setup that must run before any window exists (Windows: DPI
    /// awareness).
    fn init_process(&self) {}
    /// Local time minus UTC in seconds, daylight saving included.
    fn utc_offset_s(&self) -> i32;
    /// Milliseconds since the system booted (start-at-login check).
    fn uptime_ms(&self) -> u64;
    /// The pixel belongs to a window of this process.
    fn over_own(&self, at: (i32, i32)) -> bool;
    /// Installs window hooks that report to `tx` until the guard is dropped.
    /// `None` when the platform has none (the tracker then only polls).
    fn hooks(&self, tx: Sender<HookEvent>) -> Option<Guard>;
    /// Foreign window geometry is known (false: ground-only snow).
    fn has_geometry(&self) -> bool;
    /// A [`Platform::taskbar_shelves`] reader that runs on a background thread.
    fn shelves_reader(&self) -> Box<dyn FnOnce() -> Vec<PixelRect> + Send>;
    /// Claims the single-instance lock (core-app 2.2). A second launch asks the
    /// first one to open its window unless `quiet`.
    fn single_instance(&self, quiet: bool) -> Instance;
}

/// Result of [`Platform::single_instance`].
pub enum Instance {
    /// This process is the first one; keep the guard for the process lifetime.
    First(Box<dyn InstanceGuard>),
    /// Another instance runs; it was asked to open its window unless quiet.
    Second,
}

impl Instance {
    /// The guard of a first instance.
    pub fn first(self) -> Option<Box<dyn InstanceGuard>> {
        match self {
            Instance::First(g) => Some(g),
            Instance::Second => None,
        }
    }
}

/// The first instance's lock.
pub trait InstanceGuard {
    /// Spawns the "second launch" listener; `on_open` runs on that thread
    /// (marshal to the UI thread yourself).
    fn listen(&self, on_open: Box<dyn Fn() + Send>);
}

/// Session lock, suspend/resume, display-off, display-change and theme
/// notifications (desktop-native 3.7), on the UI thread.
pub trait SystemEvents {
    /// `on` gets the event and, when the combined "asleep" state changed, the
    /// new value. Events stop when the guard is dropped.
    fn watch(&self, on: Box<dyn FnMut(SystemEvent, Option<bool>)>) -> Option<Guard>;
}

/// The UI-thread tick source (architecture section 4).
pub trait Driver {
    /// Calls `tick` about every `every_ms` on this thread until the guard is
    /// dropped. `None`: no OS timer here; the app's event loop must call the
    /// tick itself.
    fn start(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<Guard>;
}

/// Monotonic + wall clock from the OS.
pub struct SystemClock {
    start: std::time::Instant,
    offset: Box<dyn Fn() -> i32>,
}

impl SystemClock {
    /// `offset` gives the current UTC offset in seconds
    /// ([`Platform::utc_offset_s`]).
    pub fn new(offset: impl Fn() -> i32 + 'static) -> Self {
        Self { start: std::time::Instant::now(), offset: Box::new(offset) }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }
    fn wall(&self) -> WallTime {
        let unix =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        WallTime::new(unix, (self.offset)())
    }
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
    /// The desktop can change the wallpaper at all (false hides the
    /// day-and-night switcher).
    fn wallpaper_supported(&self) -> bool;
    /// Shows `dir` in the file manager.
    fn open_folder(&self, dir: &Path);
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
    fn limits() {
        let none = Limits {
            no_overlay: false,
            no_window_geometry: false,
            no_native_menu: false,
            no_global_cursor: false,
            ..Limits::NULL
        };
        assert_eq!(Limits::default(), none);
        let null = Limits::NULL;
        assert!(null.no_overlay && !null.foreign_windows_partial);
        assert!(format!("{:?}", Limits::NULL).contains("no_overlay"));
    }

    #[test]
    fn clock_runs() {
        let c = SystemClock::new(|| 3600);
        assert!(c.now() >= 0.0);
        assert!(c.wall().unix > 1_600_000_000);
        assert_eq!(c.wall().utc_offset_s, 3600);
    }

    #[test]
    fn clock_is_monotonic() {
        let c = SystemClock::new(|| 0);
        let a = c.now();
        let b = c.now();
        assert!(b >= a);
    }

    #[test]
    fn stage_wallpaper_copies() {
        let dir = std::env::temp_dir().join(format!("fp-platform-test-{}", std::process::id()));
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
        // A data folder that is a file cannot hold the copies.
        assert!(stage_wallpaper(&src, &src, "x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
