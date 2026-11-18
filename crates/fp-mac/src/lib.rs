//! macOS backend (porting design 3.1): AppKit panels with a `CAMetalLayer`
//! each for the overlay, one borderless panel per sprite, the
//! `CGWindowListCopyWindowInfo` window source with synthesised hooks, the
//! Dock shelf, cursor and buttons from `NSEvent`, workspace and distributed
//! notifications, an `NSTimer` driver in the common run-loop modes, a Unix
//! socket single instance, `SMAppService` login item and `NSWorkspace`
//! wallpaper.
//!
//! Every AppKit object is made on the main thread (`MainThreadMarker`); the
//! overlay's render threads only touch their `CAMetalLayer` through wgpu and
//! read the cursor through Quartz.
//!
//! [`logic`] holds the platform-free parts and builds on every target; the
//! rest is empty off macOS.

pub mod logic;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub mod update;

#[cfg(target_os = "macos")]
pub use mac::backend;
#[cfg(target_os = "macos")]
pub use mac::overlay::MacOverlay;
