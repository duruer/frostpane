//! Overlay panels for `fp-overlay`'s host (porting design 3.1 "Overlay
//! panel"): one click-through `NSPanel` per screen whose content view hosts a
//! `CAMetalLayer`, the wgpu Metal surface. The swap chain is DIP (point)
//! sized and Core Animation stretches it over the screen, like the
//! DirectComposition scale transform on Windows.

use super::notify::{Observers, Timer};
use super::{screens, window, winlist};
use crate::logic::{self, BEHIND_LEVEL, FRONT_LEVEL, PANEL_LEVEL};
use fp_core::geom::PixelRect;
use fp_platform::overlay::{self as plan, OverlayBackend, PanelTarget, PanelWindow, Stack};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSPanel;
use objc2_core_graphics::CGEvent;
use objc2_foundation::NSNotificationCenter;
use objc2_quartz_core::CAMetalLayer;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Raised when the global scale differs from the one panels were made at.
static RESCALED: AtomicBool = AtomicBool::new(false);
/// Global scale (f64 bits) the live panels were created with; 0 = none yet.
static CREATED_SCALE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Screen-change observers, registered once on the main thread.
    static WATCH: RefCell<Option<Observers>> = const { RefCell::new(None) };
}

/// Re-reads the screens; a changed global scale asks for a rebuild.
fn check_scale(mtm: MainThreadMarker) {
    let (_, sp) = screens::read(mtm);
    let made = f64::from_bits(CREATED_SCALE.load(Ordering::SeqCst));
    if made > 0.0 && (made - sp.scale).abs() > 1e-6 {
        RESCALED.store(true, Ordering::SeqCst);
    }
}

fn watch_scale(mtm: MainThreadMarker) {
    WATCH.with(|w| {
        let mut w = w.borrow_mut();
        if w.is_some() {
            return;
        }
        let mut obs = Observers::default();
        let center = NSNotificationCenter::defaultCenter();
        for name in
            ["NSApplicationDidChangeScreenParametersNotification", "NSWindowDidChangeBackingPropertiesNotification"]
        {
            obs.add(&center, name, move |_| check_scale(mtm));
        }
        *w = Some(obs);
    });
}

/// One screen's panel.
struct Panel {
    window: Retained<NSPanel>,
    _layer: Retained<CAMetalLayer>,
    target: Arc<PanelTarget>,
    number: isize,
    shown: Cell<bool>,
}

fn make_panel(
    mtm: MainThreadMarker,
    instance: &wgpu::Instance,
    pixels: PixelRect,
    scale: f64,
) -> Result<Panel, String> {
    let sp = screens::space();
    let layer = CAMetalLayer::new();
    layer.setOpaque(false);
    // Drawable at DIP size, stretched by Core Animation (desktop-native 3.2).
    layer.setContentsScale(1.0);
    let window = window::panel(mtm, sp.rect_cocoa(pixels), &layer, PANEL_LEVEL);
    let number = window.windowNumber();
    // SAFETY: the layer is a live CAMetalLayer, retained by the panel's view
    // and by `Panel` for as long as the surface exists.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
            Retained::as_ptr(&layer).cast_mut().cast(),
        ))
    };
    match surface {
        Ok(surface) => {
            let s = if scale.is_finite() && scale > 0.0 { scale } else { sp.scale };
            CREATED_SCALE.store(s.to_bits(), Ordering::SeqCst);
            Ok(Panel {
                window,
                _layer: layer,
                target: Arc::new(PanelTarget { surface }),
                number,
                shown: Cell::new(false),
            })
        }
        Err(e) => {
            window.close();
            Err(e.to_string())
        }
    }
}

impl PanelWindow for Panel {
    fn target(&self) -> Arc<PanelTarget> {
        self.target.clone()
    }

    /// Core Animation commits on its own.
    fn commit(&self) {}

    fn show(&self, on: bool) {
        if on {
            self.window.orderFrontRegardless();
        } else {
            self.window.orderOut(None);
        }
        self.shown.set(on);
    }

    /// `Front`: status level, above the Dock. `Behind`: right above the
    /// desktop icons, below every app window.
    fn stack(&self, s: Stack) {
        let level = match s {
            Stack::Front => PANEL_LEVEL,
            Stack::Behind => BEHIND_LEVEL,
        } as isize;
        if self.window.level() != level {
            self.window.setLevel(level);
            if self.shown.get() {
                self.window.orderFrontRegardless();
            }
        }
    }

    fn id(&self) -> isize {
        self.number
    }

    /// Surface first (callers hold the render gate for writing), then the window.
    fn destroy(self: Box<Self>) {
        let Panel { window, _layer, target, .. } = *self;
        drop(target);
        window.orderOut(None);
        window.close();
        drop(_layer);
    }
}

/// `RaisePanels` (3.4): own topmost windows (cards, front sprites) above the
/// panels at [`FRONT_LEVEL`]. Panels and `under_dock` stay; the status item
/// and menus (level >= the panel level) are never touched.
fn raise_own(mtm: MainThreadMarker, panels: &[isize], under_dock: &[isize]) {
    let seen = logic::own_seen(&winlist::all_on_screen(), winlist::own_pid());
    for n in plan::raise_targets(&seen, panels, under_dock) {
        let Some(w) = window::by_number(mtm, n as i64) else { continue };
        let level = w.level() as i64;
        if level >= PANEL_LEVEL && level != FRONT_LEVEL {
            continue;
        }
        if level != FRONT_LEVEL {
            w.setLevel(FRONT_LEVEL as isize);
        }
        w.orderFrontRegardless();
    }
}

/// The macOS overlay backend: Metal surfaces on `CAMetalLayer`s.
#[derive(Clone, Copy, Debug)]
pub struct MacOverlay(());

impl MacOverlay {
    /// Watches screen and backing-scale changes (main thread).
    pub fn new(mtm: MainThreadMarker) -> Self {
        watch_scale(mtm);
        Self(())
    }
}

impl OverlayBackend for MacOverlay {
    const BACKENDS: wgpu::Backends = wgpu::Backends::METAL;

    /// wgpu's Metal backend offers `Opaque | PostMultiplied`; post-multiplied
    /// only clears `opaque`, and Core Animation composites premultiplied, which
    /// is what the shaders write.
    fn alpha_mode(&self, caps: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode {
        plan::pick_alpha(&caps.alpha_modes)
    }

    fn create_panel(
        &self,
        instance: &wgpu::Instance,
        pixels: PixelRect,
        _swap: (u32, u32),
        scale: f64,
    ) -> Result<Box<dyn PanelWindow>, String> {
        let mtm = MainThreadMarker::new().ok_or("overlay panels must be created on the main thread")?;
        make_panel(mtm, instance, pixels, scale).map(|p| Box::new(p) as Box<dyn PanelWindow>)
    }

    fn raise_own(&self, panels: &[isize], under_dock: &[isize]) {
        if let Some(mtm) = MainThreadMarker::new() {
            raise_own(mtm, panels, under_dock);
        }
    }

    fn take_rescaled(&self) -> bool {
        RESCALED.swap(false, Ordering::SeqCst)
    }

    fn start_ticker(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<fp_platform::Guard> {
        MainThreadMarker::new()?;
        Some(Box::new(Timer::start(every_ms, tick)))
    }

    /// Quartz cursor (thread-safe) in pixels.
    fn cursor_px() -> Option<(i32, i32)> {
        let ev = CGEvent::new(None)?;
        let p = CGEvent::location(Some(&ev));
        Some(screens::space().quartz_px(p.x, p.y))
    }

    /// Render threads run at user-interactive QoS (desktop-native 3.8).
    fn raise_priority() {
        // SAFETY: changes only the calling thread's QoS class.
        unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
        }
    }
}
