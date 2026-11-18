//! Linux-only parts of the Wayland backend.

mod overlay;
mod platform;
mod sources;
mod sprites;
mod wl;

pub use overlay::WlOverlay;
pub use sources::kwin_on_bus;

use fp_platform::select::WindowSource;
use fp_platform::{Limits, Parts};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// The connection shared by the overlay, the sprites and the platform.
pub(crate) type Shared = Rc<RefCell<wl::Wl>>;

/// The compositor offers `zwlr_layer_shell_v1` (v >= 3): selection probe
/// (design 2.3 rule 2). False without a reachable compositor or libwayland.
pub fn layer_shell_available() -> bool {
    wl::probe_layer_shell()
}

/// KDE sessions whose selection could not see KWin on the bus still get
/// the KWin source when it is there.
fn resolve(source: WindowSource) -> WindowSource {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")));
    if source == WindowSource::None && kde && kwin_on_bus() { WindowSource::KWin } else { source }
}

/// The Wayland backends for the selection's window source.
pub fn backend(source: WindowSource) -> Parts<WlOverlay> {
    let wl = match wl::Wl::connect() {
        Ok(w) => Some(Rc::new(RefCell::new(w))),
        Err(e) => {
            log::warn!("Wayland backend: {e}; running without overlay");
            None
        }
    };
    let wake: sources::Wake = Arc::new(Mutex::new(None));
    let source = resolve(source);
    let src = sources::open(source, wake.clone());
    if src.is_none() {
        log::info!("Wayland window source {source:?} unavailable: ground and panel only");
    } else {
        log::info!("Wayland window source: {source:?}");
    }
    let snap = Rc::new(RefCell::new(crate::logic::Snapshot::default()));
    let ui_x11 = std::env::var("FROSTPANE_UI").is_ok_and(|v| v.trim().eq_ignore_ascii_case("x11"));
    let limits = Limits {
        no_overlay: wl.is_none(),
        no_window_geometry: src.is_none(),
        foreign_windows_partial: false,
        no_native_menu: true,
        no_global_cursor: true,
        no_card_placement: !ui_x11,
    };
    let services = platform::WlServices {
        inner: fp_linux::services(),
        wl: wl.clone(),
        source: RefCell::new(src),
        snap: snap.clone(),
        wake,
        synth: RefCell::new(fp_tracker::synth::HookSynth::new()),
        start: std::time::Instant::now(),
    };
    Parts {
        services: Rc::new(services),
        sprites: Box::new(sprites::WlSprites::new(wl.clone(), snap)),
        overlay: WlOverlay { wl, layout: Rc::new(RefCell::new(Vec::new())) },
        limits,
    }
}
