//! X11 backend (porting design 3.2). Skeleton: on Linux [`backend`] returns
//! the null backends with the `fp-linux` session services, so the app runs
//! with its windows and tray but without overlay or object windows. Empty on
//! other targets.

#[cfg(target_os = "linux")]
pub use imp::backend;

#[cfg(target_os = "linux")]
mod imp {
    use fp_platform::null::{NullOverlay, NullSprites};
    use fp_platform::{Limits, Parts};
    use std::rc::Rc;

    /// `xwayland`: running inside a Wayland session without layer shell.
    pub fn backend(xwayland: bool) -> Parts<NullOverlay> {
        log::warn!("X11 backend not yet supported: running without overlay and object windows");
        Parts {
            services: Rc::new(fp_linux::services()),
            sprites: Box::new(NullSprites::default()),
            overlay: NullOverlay,
            limits: Limits { foreign_windows_partial: xwayland, ..Limits::NULL },
        }
    }
}
