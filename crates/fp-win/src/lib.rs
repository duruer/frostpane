//! Windows platform services. Not part of the coverage gate.
//!
//! Spec: core-app 2.2 (single instance + second-launch event), 2.5 battery
//! read, 2.6 (taskbar theme, tray icon spot), 2.10 (launch at login, opt-in),
//! 2.13 (data folder for running.json / crash log); desktop-native 2 + 3.6
//! (monitor enumeration, DPI, display ids via QueryDisplayConfig), 3.1-3.4
//! (overlay panels: DirectComposition windows, z-order), 3.5.1 (WinEvent
//! hooks), 3.5.3 (raw attributes -> `fp_tracker::WindowInfo`), 3.5.6 (taskbar
//! shelves), 3.7 (session/suspend/display-off notifications, battery saver,
//! reduced motion), 3.10 helpers (cursor, root window), 3.12
//! (IDesktopWallpaper), weather-sources 2.5 (time-zone id, GeoID latitude).
//!
//! The traits live in `fp-platform`; [`windows_impl::Win`] implements
//! `Platform`, `Shell`, `SystemEvents` and `Driver`, [`overlay::WinOverlay`]
//! the overlay panels and [`sprites::WinBackend`] the layered object windows.
//! On other targets the crate only holds the pure [`logic`].

pub use fp_platform::stage_wallpaper;
pub use fp_platform::{Instance, Platform, PowerStatus, Shell, SleepState, SystemClock, SystemEvent, WallpaperScreen};

#[cfg(windows)]
use fp_core::desktop::MonitorInfo;
#[cfg(windows)]
use fp_core::geom::PixelRect;
#[cfg(windows)]
use std::path::{Path, PathBuf};

pub mod logic;
#[cfg(windows)]
pub mod overlay;
#[cfg(windows)]
pub mod sprites;
#[cfg(windows)]
pub mod update;
#[cfg(windows)]
pub mod windows_impl;

/// The Windows backends: `Win` services, layered sprite windows and the
/// DirectComposition overlay panels.
#[cfg(windows)]
pub fn backend() -> fp_platform::Parts<overlay::WinOverlay> {
    fp_platform::Parts {
        services: std::rc::Rc::new(windows_impl::Win),
        sprites: Box::new(sprites::WinBackend::default()),
        overlay: overlay::WinOverlay::new(),
        limits: fp_platform::Limits::default(),
    }
}
