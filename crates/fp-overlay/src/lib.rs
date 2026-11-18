//! Per-monitor weather overlay. Not part of the coverage gate.
//!
//! Spec: desktop-native 3.1 (overlay window recipe), 3.2 (composition
//! presentation; here wgpu `SurfaceTargetUnsafe::CompositionVisual` on our own
//! `IDCompositionDevice`, validated by spikes/dcomp-overlay), 3.3 (second
//! "above" panel for snow in behind-windows mode), 3.4 (z-order), 3.8 (one
//! render thread per monitor, 60 fps cap, dt clamp, gate lock around the sim),
//! 3.9 (shared GPU device, premultiplied blend), snow-sim 4 and
//! other-weather 2.x rendering (WGSL ports of the five shaders), misc 5.
//!
//! The simulations fill [`fp_core::gpu::DrawList`]s; this crate owns all
//! wgpu pipelines (`gpu`, `shaders/*.wgsl`). `plan` holds the pure logic (DIP
//! sizing, cursor mapping, the behind-windows staging machine) and is
//! unit-tested; `host` owns the render threads and the staging over a
//! platform [`OverlayBackend`] (`fp_win::overlay::WinOverlay` on Windows).
//!
//! # Using the host (UI thread only)
//!
//! ```ignore
//! let mut overlay = OverlayHost::new(WinOverlay::new(), &monitors)?; // transparent panels
//! overlay.set_effect(Some(Arc::new(|p: &PanelInfo| make_fall(kind, p))));
//! overlay.set_share(0.5);                                  // battery saver x storm
//! overlay.set_wind(Wind::GentleLeft);
//! overlay.set_behind_windows(true);                        // staged Sit(behind)
//! // every tracker poll:
//! overlay.update(&poll.screens, poll.interval, poll.to_other_space);
//! // every 1 s (dock tick), then re-raise the layered object windows:
//! overlay.keep_on_top();
//! // after WM_DPICHANGED or a display change:
//! if overlay.rescaled() { overlay.rebuild(&monitors)?; }
//! ```
//!
//! Staged transitions (behind-windows hand-over, parked panels) advance on a
//! thread timer of the UI thread when the backend has one, so the host needs
//! the UI message loop to run (winit's loop does). Without one
//! ([`OverlayHost::needs_tick`]) the owner calls `OverlayHost::tick`.

pub mod gpu;
mod host;
pub mod plan;

pub use gpu::{Gpu, Image};
pub use plan::PanelInfo;

use fp_core::desktop::MonitorInfo;
use fp_core::falling::Falling;
use fp_core::surface::ScreenSnap;
use fp_core::wind::Wind;
use fp_platform::overlay::OverlayBackend;
use std::sync::Arc;

/// Builds the simulation of one panel (called on the UI thread, again for
/// every rebuild). The app reads reduced motion here and picks the seed.
pub type EffectFactory = Arc<dyn Fn(&PanelInfo) -> Box<dyn Falling> + Send + Sync>;

/// Settings the host re-applies to every new simulation and across rebuilds.
#[derive(Clone)]
pub struct Config {
    pub factory: Option<EffectFactory>,
    /// Density multiplier (battery saver x storm strength).
    pub share: f64,
    pub wind: Wind,
    /// Rain: thunderstorm on/off.
    pub lightning: bool,
    pub keep_clear: bool,
    /// Behind-windows mode (desktop-native 3.3).
    pub behind: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { factory: None, share: 1.0, wind: Wind::None, lightning: true, keep_clear: false, behind: false }
    }
}

/// The host's API with the backend type erased.
trait HostOps {
    fn config(&self) -> Config;
    fn panels(&self) -> usize;
    fn frames(&self) -> u64;
    fn infos(&self) -> Vec<PanelInfo>;
    fn set_effect(&self, factory: Option<EffectFactory>);
    fn set_share(&self, share: f64);
    fn set_wind(&self, wind: Wind);
    fn set_lightning(&self, on: bool);
    fn set_keep_clear(&self, on: bool);
    fn set_behind_windows(&self, on: bool);
    fn update(&self, screens: &[ScreenSnap], interval: f64, to_other_space: bool);
    fn keep_on_top(&self);
    fn set_under_dock(&self, ids: Vec<isize>);
    fn tick(&self);
    fn needs_tick(&self) -> bool;
}

impl<B: OverlayBackend> HostOps for host::Host<B> {
    fn config(&self) -> Config {
        host::Host::config(self)
    }
    fn panels(&self) -> usize {
        host::Host::panels(self)
    }
    fn frames(&self) -> u64 {
        host::Host::frames(self)
    }
    fn infos(&self) -> Vec<PanelInfo> {
        host::Host::infos(self)
    }
    fn set_effect(&self, factory: Option<EffectFactory>) {
        host::Host::set_effect(self, factory);
    }
    fn set_share(&self, share: f64) {
        host::Host::set_share(self, share);
    }
    fn set_wind(&self, wind: Wind) {
        host::Host::set_wind(self, wind);
    }
    fn set_lightning(&self, on: bool) {
        host::Host::set_lightning(self, on);
    }
    fn set_keep_clear(&self, on: bool) {
        host::Host::set_keep_clear(self, on);
    }
    fn set_behind_windows(&self, on: bool) {
        host::Host::set_behind_windows(self, on);
    }
    fn update(&self, screens: &[ScreenSnap], interval: f64, to_other_space: bool) {
        host::Host::update(self, screens, interval, to_other_space);
    }
    fn keep_on_top(&self) {
        host::Host::keep_on_top(self);
    }
    fn set_under_dock(&self, ids: Vec<isize>) {
        host::Host::set_under_dock(self, ids);
    }
    fn tick(&self) {
        host::Host::tick(self);
    }
    fn needs_tick(&self) -> bool {
        host::Host::needs_tick(self)
    }
}

type MakeHost = Box<dyn Fn(&[MonitorInfo], Config) -> Result<Box<dyn HostOps>, String>>;

/// Handle to all overlay panels. Create and drop on the UI thread.
pub struct OverlayHost {
    make: MakeHost,
    /// The backend's "a panel's scale changed" flag; read even after a
    /// failed rebuild left no panels.
    rescaled: Box<dyn Fn() -> bool>,
    inner: Option<Box<dyn HostOps>>,
}

impl OverlayHost {
    /// Creates one click-through panel per monitor and starts the render
    /// threads. Monitors that fail are skipped; an error means no GPU.
    /// Without an effect the panels stay transparent and idle.
    pub fn new<B: OverlayBackend>(backend: B, monitors: &[MonitorInfo]) -> Result<Self, String> {
        let flag = backend.clone();
        let rescaled = Box::new(move || flag.take_rescaled());
        let make: MakeHost = Box::new(move |m: &[MonitorInfo], cfg: Config| {
            host::Host::new(backend.clone(), m, cfg).map(|h| Box::new(h) as Box<dyn HostOps>)
        });
        let inner = make(monitors, Config::default())?;
        Ok(Self { make, rescaled, inner: Some(inner) })
    }

    /// Tears every panel down and builds new ones for `monitors` (display or
    /// DPI change, screen selection). Effect, share, wind, lightning,
    /// keep-clear and behind-windows settings carry over. On error the host
    /// has no panels until the next successful rebuild.
    pub fn rebuild(&mut self, monitors: &[MonitorInfo]) -> Result<(), String> {
        let cfg = self.inner.as_ref().map(|h| h.config()).unwrap_or_default();
        // Full teardown first: two overlays never coexist.
        self.inner = None;
        self.inner = Some((self.make)(monitors, cfg)?);
        Ok(())
    }

    /// Number of live panels.
    pub fn panels(&self) -> usize {
        self.inner.as_ref().map_or(0, |h| h.panels())
    }

    /// Frames presented so far on the main panels, summed.
    pub fn frames(&self) -> u64 {
        self.inner.as_ref().map_or(0, |h| h.frames())
    }

    /// The screens the panels were built for, in panel order.
    pub fn infos(&self) -> Vec<PanelInfo> {
        self.inner.as_ref().map(|h| h.infos()).unwrap_or_default()
    }

    /// Sets (or removes with `None`) the weather. Every panel gets a fresh
    /// simulation from the factory, with the stored settings applied.
    pub fn set_effect(&self, factory: Option<EffectFactory>) {
        if let Some(h) = &self.inner {
            h.set_effect(factory);
        }
    }

    /// Density multiplier pushed to every simulation (3.7: 0.5 on battery
    /// saver times the storm strength).
    pub fn set_share(&self, share: f64) {
        if let Some(h) = &self.inner {
            h.set_share(share);
        }
    }

    pub fn set_wind(&self, wind: Wind) {
        if let Some(h) = &self.inner {
            h.set_wind(wind);
        }
    }

    pub fn set_lightning(&self, on: bool) {
        if let Some(h) = &self.inner {
            h.set_lightning(on);
        }
    }

    pub fn set_keep_clear(&self, on: bool) {
        if let Some(h) = &self.inner {
            h.set_keep_clear(on);
        }
    }

    /// Behind-windows mode. Snow moves to the staged two-panel arrangement
    /// (0.1 s hand-over steps); other weather sits behind all windows at once.
    pub fn set_behind_windows(&self, on: bool) {
        if let Some(h) = &self.inner {
            h.set_behind_windows(on);
        }
    }

    /// One tracker poll: pairs each snapshot with its panel by screen frame,
    /// shows or hides the panel (`shown` pauses its render thread), sets the
    /// ground flag and passes the surfaces to the simulation. Panels without
    /// a snapshot are hidden (screen not selected).
    pub fn update(&self, screens: &[ScreenSnap], interval: f64, to_other_space: bool) {
        if let Some(h) = &self.inner {
            h.update(screens, interval, to_other_space);
        }
    }

    /// `KeepOnTop` (3.4): call about once a second. Re-raises the topmost
    /// panels (or re-seats the main panel above the desktop in behind mode).
    /// The app must re-raise its layered object windows afterwards.
    pub fn keep_on_top(&self) {
        if let Some(h) = &self.inner {
            h.keep_on_top();
        }
    }

    /// Host windows kept under the taskbar (`ZBand::UnderDock`); `KeepOnTop`
    /// never re-raises them. Call before [`OverlayHost::keep_on_top`].
    pub fn set_under_dock(&self, ids: Vec<isize>) {
        if let Some(h) = &self.inner {
            h.set_under_dock(ids);
        }
    }

    /// True once after a panel's DPI changed (`WM_DPICHANGED`): call
    /// [`OverlayHost::rebuild`].
    pub fn rescaled(&self) -> bool {
        (self.rescaled)()
    }

    /// Runs staged steps that are due. Normally driven by an internal timer.
    pub fn tick(&self) {
        if let Some(h) = &self.inner {
            h.tick();
        }
    }

    /// The backend has no UI-thread timer: call [`OverlayHost::tick`] from
    /// the app's own tick (never on Windows).
    pub fn needs_tick(&self) -> bool {
        self.inner.as_ref().is_some_and(|h| h.needs_tick())
    }
}
