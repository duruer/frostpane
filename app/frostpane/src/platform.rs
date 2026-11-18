//! Picks the platform backend at startup (porting design 2.3): Windows and
//! macOS have one each; on Linux the session decides between Wayland and
//! X11 (`fp_platform::select::linux`). A backend whose display connection
//! fails falls back to the null backends: the app runs with its windows and
//! tray, without overlay or object windows, and `Limits` says so.

use fp_core::desktop::MonitorInfo;
use fp_overlay::OverlayHost;
use fp_platform::overlay::OverlayBackend;
use fp_platform::sprites::SpriteBackend;
use fp_platform::{Limits, Parts, Services};
use std::rc::Rc;

/// Builds the overlay host for a monitor list.
pub type MakeOverlay = Box<dyn Fn(&[MonitorInfo]) -> Result<OverlayHost, String>>;

/// The selected backends.
pub struct Backend {
    pub services: Rc<dyn Services>,
    pub sprites: Box<dyn SpriteBackend>,
    pub overlay: MakeOverlay,
    /// What this session cannot do (UI notices, menu card, hide fallback).
    pub limits: Limits,
    /// The egui windows must run on X11 (Linux).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub ui_x11: bool,
}

fn from_parts<O: OverlayBackend>(p: Parts<O>, ui_x11: bool) -> Backend {
    let o = p.overlay;
    Backend {
        services: p.services,
        sprites: p.sprites,
        overlay: Box::new(move |m| OverlayHost::new(o.clone(), m)),
        limits: p.limits,
        ui_x11,
    }
}

#[cfg(windows)]
pub fn select() -> Result<Backend, String> {
    Ok(from_parts(fp_win::backend(), false))
}

#[cfg(target_os = "macos")]
pub fn select() -> Result<Backend, String> {
    Ok(from_parts(fp_mac::backend(), false))
}

#[cfg(target_os = "linux")]
pub fn select() -> Result<Backend, String> {
    use fp_platform::select::{Choice, Probes, linux};
    let env = |k: &str| std::env::var(k).ok();
    let mut layer_shell = fp_wayland::layer_shell_available;
    let mut kwin = fp_linux::kwin_on_bus;
    let choice = linux(&env, Probes { layer_shell: &mut layer_shell, kwin: &mut kwin });
    log::info!("desktop backend: {choice:?}");
    let ui_x11 = choice.ui_on_x11(&env);
    match choice {
        Choice::Wayland { source } => Ok(from_parts(fp_wayland::backend(source), ui_x11)),
        Choice::X11 { xwayland } => Ok(from_parts(fp_x11::backend(xwayland), ui_x11)),
        Choice::Nothing => Err("no display server found (neither WAYLAND_DISPLAY nor DISPLAY is set)".into()),
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn select() -> Result<Backend, String> {
    Err("this platform is not supported".into())
}
