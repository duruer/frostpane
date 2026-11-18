//! X11 backend (porting design 3.2). On Linux [`backend`] returns the X11
//! services (display, EWMH window source, pointer, hooks; session services
//! from `fp-linux`), ARGB override-redirect sprite windows and the ARGB
//! overlay panels. The overlay needs a compositing manager; without one the
//! session reports `Limits::no_overlay` and objects still work (shaped
//! windows). Without a display at all the null backends stand in.
//!
//! [`logic`] is platform-free and tested on every target; the rest compiles
//! only on Linux.

pub mod logic;

#[cfg(target_os = "linux")]
mod overlay;
#[cfg(target_os = "linux")]
mod platform;
#[cfg(target_os = "linux")]
mod sprites;
#[cfg(target_os = "linux")]
mod watch;
#[cfg(target_os = "linux")]
mod x;

#[cfg(target_os = "linux")]
pub use imp::backend;
#[cfg(target_os = "linux")]
pub use overlay::{NO_COMPOSITOR, X11Overlay};

#[cfg(target_os = "linux")]
mod imp {
    use crate::overlay::X11Overlay;
    use crate::platform::X11Platform;
    use crate::sprites::X11Sprites;
    use fp_platform::null::NullSprites;
    use fp_platform::sprites::SpriteBackend;
    use fp_platform::{Limits, Parts};
    use std::rc::Rc;

    /// `xwayland`: running inside a Wayland session without layer shell, so
    /// only X11 clients are seen.
    pub fn backend(xwayland: bool) -> Parts<X11Overlay> {
        let services = fp_linux::services();
        let Some(x) = crate::x::get() else {
            log::warn!("x11: no display; running without overlay and object windows");
            return Parts {
                services: Rc::new(services),
                sprites: Box::new(NullSprites::default()),
                overlay: X11Overlay,
                limits: Limits { foreign_windows_partial: xwayland, ..Limits::NULL },
            };
        };
        crate::watch::ensure();
        let composited = x.composited();
        if !composited {
            log::warn!("x11: {}", crate::overlay::NO_COMPOSITOR);
        }
        let sprites: Box<dyn SpriteBackend> = if x.argb.is_some() {
            Box::new(X11Sprites::new(x))
        } else {
            log::warn!("x11: no 32-bit visual; object windows disabled");
            Box::new(NullSprites::default())
        };
        Parts {
            services: Rc::new(X11Platform { x, inner: Box::new(services) }),
            sprites,
            overlay: X11Overlay,
            limits: Limits {
                no_overlay: !composited || x.argb.is_none(),
                no_compositor: !composited,
                foreign_windows_partial: xwayland,
                no_native_menu: true,
                ..Limits::default()
            },
        }
    }
}
