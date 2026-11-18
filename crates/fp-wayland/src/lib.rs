//! Wayland backend (porting design 3.3).
//!
//! On Linux [`backend`] connects to the compositor and returns:
//! - overlay panels: one `zwlr_layer_shell_v1` surface per output on the
//!   overlay layer (bottom layer in behind mode), anchored to every edge,
//!   exclusive zone -1, no keyboard, empty input region; wgpu renders into
//!   a desynchronised subsurface of it;
//! - sprite windows: small layer surfaces positioned by margins, `wl_shm`
//!   buffers, input regions from the bitmap's alpha;
//! - outputs (xdg-output logical layout times the largest output scale,
//!   design 2.4), the pointer over our own surfaces, and foreign window
//!   geometry from the sway IPC, the Hyprland IPC or a KWin script, chosen
//!   by the selection's [`fp_platform::select::WindowSource`].
//!
//! [`layer_shell_available`] is the selection probe. The parsers live in
//! [`logic`], which builds on every target; the rest is Linux only.

pub mod logic;

#[cfg(target_os = "linux")]
mod imp;
#[cfg(target_os = "linux")]
pub use imp::{backend, kwin_on_bus, layer_shell_available};
