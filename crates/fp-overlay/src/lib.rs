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
//! unit-tested; `win` is the Win32/DirectComposition host.
//!
//! # Using the host (UI thread only)
//!
//! ```ignore
//! let mut overlay = OverlayHost::new(&monitors)?;          // transparent panels
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
//! thread timer of the UI thread, so the host needs the UI message loop to
//! run (winit's loop does). `OverlayHost::tick` runs them by hand.

pub mod gpu;
pub mod plan;
#[cfg(not(windows))]
#[path = "stub.rs"]
mod win;
#[cfg(windows)]
mod win;

pub use gpu::{Gpu, Image};
pub use plan::PanelInfo;

use fp_core::desktop::MonitorInfo;
use fp_core::falling::Falling;
use fp_core::surface::ScreenSnap;
use fp_core::wind::Wind;
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

/// Handle to all overlay panels. Create and drop on the UI thread.
pub struct OverlayHost {
    inner: Option<win::Host>,
}

impl OverlayHost {
    /// Creates one click-through panel per monitor and starts the render
    /// threads. Monitors that fail are skipped; an error means no GPU.
    /// Without an effect the panels stay transparent and idle.
    pub fn new(monitors: &[MonitorInfo]) -> Result<Self, String> {
        Ok(Self { inner: Some(win::Host::new(monitors, Config::default())?) })
    }

    /// Tears every panel down and builds new ones for `monitors` (display or
    /// DPI change, screen selection). Effect, share, wind, lightning,
    /// keep-clear and behind-windows settings carry over. On error the host
    /// has no panels until the next successful rebuild.
    pub fn rebuild(&mut self, monitors: &[MonitorInfo]) -> Result<(), String> {
        let cfg = self.inner.as_ref().map(win::Host::config).unwrap_or_default();
        // Full teardown first: two overlays never coexist.
        self.inner = None;
        self.inner = Some(win::Host::new(monitors, cfg)?);
        Ok(())
    }

    /// Number of live panels.
    pub fn panels(&self) -> usize {
        self.inner.as_ref().map_or(0, win::Host::panels)
    }

    /// Frames presented so far on the main panels, summed.
    pub fn frames(&self) -> u64 {
        self.inner.as_ref().map_or(0, win::Host::frames)
    }

    /// The screens the panels were built for, in panel order.
    pub fn infos(&self) -> Vec<PanelInfo> {
        self.inner.as_ref().map(win::Host::infos).unwrap_or_default()
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
        win::take_rescaled()
    }

    /// Runs staged steps that are due. Normally driven by an internal timer.
    pub fn tick(&self) {
        if let Some(h) = &self.inner {
            h.tick();
        }
    }
}
