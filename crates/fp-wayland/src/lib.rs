//! Wayland backend (porting design 3.3). Skeleton: on Linux [`backend`]
//! returns the null backends with the `fp-linux` session services, and
//! [`layer_shell_available`] reports no layer shell yet. Empty on other
//! targets.

#[cfg(target_os = "linux")]
pub use imp::{backend, layer_shell_available};

#[cfg(target_os = "linux")]
mod imp {
    use fp_platform::null::{NullOverlay, NullSprites};
    use fp_platform::select::WindowSource;
    use fp_platform::{Limits, Parts};
    use std::rc::Rc;

    /// The compositor offers `zwlr_layer_shell_v1` (v >= 3). Not probed yet:
    /// always false, so Wayland sessions use X11 over XWayland for now.
    pub fn layer_shell_available() -> bool {
        false
    }

    pub fn backend(source: WindowSource) -> Parts<NullOverlay> {
        log::warn!("Wayland backend not yet supported (window source {source:?}): running without overlay");
        Parts {
            services: Rc::new(fp_linux::services()),
            sprites: Box::new(NullSprites::default()),
            overlay: NullOverlay,
            limits: Limits { no_card_placement: true, ..Limits::NULL },
        }
    }
}
