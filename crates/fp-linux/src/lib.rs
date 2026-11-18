//! Linux session services shared by `fp-x11` and `fp-wayland` (porting
//! design 3.5): single instance, system events over D-Bus, autostart,
//! wallpaper, `xdg-open`, language, time zone, data folder.
//!
//! [`Services`] implements `Platform`, `Shell`, `SystemEvents` and `Driver`
//! with the session parts real and the display parts (monitors, windows,
//! cursor, hooks) empty; a display backend wraps it and delegates the
//! session parts. The events arrive on a background thread; the UI thread
//! must call [`Services::pump`] regularly (every tick) to deliver them to
//! the `watch` callback. The logic in [`logic`] is platform-free and tested
//! on every OS; the rest compiles only on Linux.

pub mod logic;

#[cfg(target_os = "linux")]
mod bus;
#[cfg(target_os = "linux")]
mod imp;
#[cfg(target_os = "linux")]
mod instance;
#[cfg(target_os = "linux")]
mod watcher;
#[cfg(target_os = "linux")]
pub use imp::{Services, kwin_on_bus, services, utc_offset_s};
#[cfg(target_os = "linux")]
pub use instance::{Acquired, SingleInstance};
