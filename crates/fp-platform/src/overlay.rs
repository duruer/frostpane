//! Overlay panels: the OS seam of `fp-overlay`'s host (desktop-native 3.1-3.4,
//! 3.8). The host owns the render threads, the staging and the GPU; a backend
//! only makes, shows, stacks and destroys the per-monitor windows.

use fp_core::geom::PixelRect;
use std::sync::Arc;

/// Swap chain surface of one panel, sized by [`OverlayBackend::swap_size`].
pub struct PanelTarget {
    pub surface: wgpu::Surface<'static>,
}

/// Where `keep_on_top` puts a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    /// Above every normal window (topmost).
    Front,
    /// Right above the desktop, below every normal window (behind mode).
    Behind,
}

/// One click-through panel window with its surface. UI thread only; the
/// render threads see only [`PanelWindow::target`].
pub trait PanelWindow {
    fn target(&self) -> Arc<PanelTarget>;
    /// Flushes pending composition changes (DirectComposition `Commit`).
    fn commit(&self);
    fn show(&self, on: bool);
    fn stack(&self, s: Stack);
    /// Native window id, as the sprite host and `raise_own` see it.
    fn id(&self) -> isize;
    /// Releases the surface first, then the window (desktop-native 3.2).
    fn destroy(self: Box<Self>);
}

/// Window side of the overlay host, one implementation per platform.
pub trait OverlayBackend: Clone + 'static {
    /// wgpu backends the panels' surfaces need.
    const BACKENDS: wgpu::Backends;
    /// Composite alpha mode of the panels' swap chains.
    fn alpha_mode(&self, caps: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode;
    /// Present mode of the panels' swap chains. Default: Fifo (vsync),
    /// which every surface supports.
    fn present_mode(&self, _caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
        wgpu::PresentMode::Fifo
    }
    /// Swap chain size of a panel covering `pixels` whose DIP size is `dip`.
    /// Default: `dip`, stretched to the monitor by the compositor (Windows
    /// `SetTransform`, Core Animation, `wp_viewporter`). A platform that
    /// cannot stretch (X11) returns the panel's pixel size.
    fn swap_size(&self, _pixels: PixelRect, dip: (u32, u32)) -> (u32, u32) {
        dip
    }
    /// A hidden panel covering `pixels`, rendering at `swap`
    /// ([`OverlayBackend::swap_size`]) stretched to the monitor. The surface is not configured yet.
    fn create_panel(
        &self,
        instance: &wgpu::Instance,
        pixels: PixelRect,
        swap: (u32, u32),
        scale: f64,
    ) -> Result<Box<dyn PanelWindow>, String>;
    /// `RaisePanels` (3.4): re-raise the app's own topmost windows above the
    /// panels, leaving `panels` and `under_dock` where they are.
    fn raise_own(&self, panels: &[isize], under_dock: &[isize]);
    /// True once after a panel's scale changed: the host must be rebuilt.
    fn take_rescaled(&self) -> bool;
    /// Calls `tick` every `every_ms` on this (UI) thread until the guard is
    /// dropped. `None`: the app calls the host's `tick` itself.
    fn start_ticker(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<crate::Guard>;
    /// Global cursor in virtual-desktop pixels. Called on render threads.
    fn cursor_px() -> Option<(i32, i32)>;
    /// Runs once at the start of every render thread.
    fn raise_priority() {}
}

/// Preferred alpha mode from what a surface supports: premultiplied (the
/// shaders' output), then post-multiplied, then inherit, else the first one.
pub fn pick_alpha(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode as M;
    [M::PreMultiplied, M::PostMultiplied, M::Inherit]
        .into_iter()
        .find(|m| modes.contains(m))
        .or_else(|| modes.first().copied())
        .unwrap_or(M::Auto)
}

/// [`OverlayBackend::swap_size`] of a platform without compositor-side
/// scaling: the panel's pixel size, clamped like the DIP size.
pub fn pixel_swap(pixels: PixelRect) -> (u32, u32) {
    let f = |v: i32| v.clamp(1, 16384) as u32;
    (f(pixels.width()), f(pixels.height()))
}

/// Present mode for a panel whose swap chain must never block its render
/// thread while the compositor withholds frame callbacks (an occluded
/// Wayland surface): Mailbox, else Immediate, else Fifo.
pub fn pick_nonblocking(modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
    use wgpu::PresentMode as P;
    [P::Mailbox, P::Immediate].into_iter().find(|m| modes.contains(m)).unwrap_or(P::Fifo)
}

/// Visual scale that stretches the DIP-sized swap chain over the monitor's
/// pixels (desktop-native 3.2 `SetTransform`).
pub fn visual_scale(pixels: PixelRect, swap: (u32, u32)) -> (f32, f32) {
    (pixels.width().max(1) as f32 / swap.0.max(1) as f32, pixels.height().max(1) as f32 / swap.1.max(1) as f32)
}

/// DPI the panel was created at, for the "rescaled" check (desktop-native 3.1).
pub fn creation_dpi(scale: f64) -> u32 {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    (s * 96.0).round() as u32
}

/// One window met while walking the topmost z-order from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub hwnd: isize,
    pub topmost: bool,
    pub visible: bool,
    pub own_thread: bool,
}

/// `RaisePanels` (3.4): windows to re-apply TOPMOST to, in apply order.
/// `panels` are the overlay's own panels, `under_dock` the host's windows
/// that stay below the taskbar.
pub fn raise_targets(seen: &[Seen], panels: &[isize], under_dock: &[isize]) -> Vec<isize> {
    let list: Vec<fp_tracker::TopWindow> = seen
        .iter()
        .map(|w| fp_tracker::TopWindow {
            hwnd: w.hwnd as i64,
            topmost: w.topmost,
            panel: panels.contains(&w.hwnd),
            under_dock: under_dock.contains(&w.hwnd),
            visible: w.visible,
            own_thread: w.own_thread,
        })
        .collect();
    fp_tracker::raise_list(&list).into_iter().map(|h| h as isize).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_scale_stretches() {
        let s = visual_scale(PixelRect::new(0, 0, 3840, 2160), (2560, 1440));
        assert!((s.0 - 1.5).abs() < 1e-6 && (s.1 - 1.5).abs() < 1e-6);
        assert_eq!(visual_scale(PixelRect::new(0, 0, 0, 0), (0, 0)), (1.0, 1.0));
    }

    #[test]
    fn pixel_swap_clamps() {
        assert_eq!(pixel_swap(PixelRect::new(0, 0, 3840, 2160)), (3840, 2160));
        assert_eq!(pixel_swap(PixelRect::new(0, 0, 0, 0)), (1, 1));
        assert_eq!(pixel_swap(PixelRect::new(0, 0, 40000, 10)), (16384, 10));
    }

    #[test]
    fn nonblocking_preference() {
        use wgpu::PresentMode as P;
        assert_eq!(pick_nonblocking(&[P::Fifo, P::Mailbox, P::Immediate]), P::Mailbox);
        assert_eq!(pick_nonblocking(&[P::Fifo, P::Immediate]), P::Immediate);
        assert_eq!(pick_nonblocking(&[P::Fifo]), P::Fifo);
        assert_eq!(pick_nonblocking(&[]), P::Fifo);
    }

    #[test]
    fn alpha_preference() {
        use wgpu::CompositeAlphaMode as M;
        assert_eq!(pick_alpha(&[M::Opaque, M::PreMultiplied]), M::PreMultiplied);
        assert_eq!(pick_alpha(&[M::Opaque, M::PostMultiplied, M::Inherit]), M::PostMultiplied);
        assert_eq!(pick_alpha(&[M::Inherit, M::Opaque]), M::Inherit);
        assert_eq!(pick_alpha(&[M::Opaque]), M::Opaque);
        assert_eq!(pick_alpha(&[]), M::Auto);
    }

    #[test]
    fn raise_targets_filter_and_order() {
        let s = |hwnd, topmost, visible, own_thread| Seen { hwnd, topmost, visible, own_thread };
        let seen = [
            s(1, true, true, true),
            s(2, true, true, true),
            s(3, true, true, true),
            s(4, true, false, true),
            s(5, true, true, false),
            s(6, true, true, true),
            s(7, false, true, true),
            s(8, true, true, true),
        ];
        // 2 = panel, 3 = under dock, 4 hidden, 5 foreign, 7 ends the walk.
        assert_eq!(raise_targets(&seen, &[2], &[3]), vec![6, 1]);
        assert!(raise_targets(&[], &[], &[]).is_empty());
    }

    #[test]
    fn dpi_values() {
        assert_eq!(creation_dpi(1.0), 96);
        assert_eq!(creation_dpi(1.5), 144);
        assert_eq!(creation_dpi(f64::NAN), 96);
        assert_eq!(creation_dpi(-2.0), 96);
    }
}
