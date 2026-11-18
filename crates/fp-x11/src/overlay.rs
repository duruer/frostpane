//! X11 overlay panels (porting design 3.2 "Overlay panel"): one
//! override-redirect, depth-32 ARGB window per monitor with an empty input
//! shape (click-through), a Vulkan (or GL) surface from its XCB handle, and
//! the z-order rules of desktop-native 3.4 mapped to X stacking.

use crate::logic;
use crate::watch;
use crate::x::{self, Xc};
use fp_core::geom::PixelRect;
use fp_platform::overlay::{OverlayBackend, PanelTarget, PanelWindow, Stack, pick_alpha};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, XcbDisplayHandle, XcbWindowHandle};
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ClipOrdering, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, StackMode, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

/// Error of `create_panel` without a compositing manager.
pub const NO_COMPOSITOR: &str = "no compositing manager: the X11 overlay needs one for transparency";

struct Panel {
    x: &'static Xc,
    win: Window,
    target: Option<Arc<PanelTarget>>,
}

fn panel(x: &'static Xc, instance: &wgpu::Instance, pixels: PixelRect, swap: (u32, u32)) -> Result<Panel, String> {
    if !x.composited() {
        return Err(NO_COMPOSITOR.into());
    }
    let (visual, cmap) = x.argb.ok_or("no 32-bit TrueColor visual")?;
    let c = &x.conn;
    let win = c.generate_id().map_err(|e| e.to_string())?;
    let aux = CreateWindowAux::new()
        .background_pixel(0)
        .border_pixel(0)
        .override_redirect(1)
        .colormap(cmap)
        .event_mask(EventMask::NO_EVENT);
    let (w, h) = (pixels.width().clamp(1, 32767) as u16, pixels.height().clamp(1, 32767) as u16);
    c.create_window(
        32,
        win,
        x.root,
        pixels.left as i16,
        pixels.top as i16,
        w,
        h,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &aux,
    )
    .map_err(|e| e.to_string())?;
    x.tag_own(win, "Frostpane overlay");
    // 2 = never unredirect, even over full-screen windows.
    let _ = c.change_property32(PropMode::REPLACE, win, x.atoms._NET_WM_BYPASS_COMPOSITOR, AtomEnum::CARDINAL, &[2]);
    if x.shape {
        let _ = c.shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &[]);
    }
    let _ = c.flush();
    if swap != (u32::from(w), u32::from(h)) {
        // X has no compositor-side scaling: the swap chain should match the window.
        log::warn!("x11 overlay: swap {swap:?} differs from the window {w}x{h}; output is not stretched");
    }
    x.own().panels.insert(win);
    let display = XcbDisplayHandle::new(NonNull::new(c.get_raw_xcb_connection()), x.screen as i32);
    let mut wh = XcbWindowHandle::new(NonZeroU32::new(win).ok_or("window id 0")?);
    wh.visual_id = NonZeroU32::new(visual);
    // SAFETY: the connection is a process-lifetime static and the window
    // outlives the surface (`destroy` drops the surface first).
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Xcb(display)),
            raw_window_handle: RawWindowHandle::Xcb(wh),
        })
    };
    match surface {
        Ok(surface) => Ok(Panel { x, win, target: Some(Arc::new(PanelTarget { surface })) }),
        Err(e) => {
            x.own().panels.remove(&win);
            let _ = c.destroy_window(win);
            let _ = c.flush();
            Err(e.to_string())
        }
    }
}

/// Our windows plus our own managed frames: what may lie above a panel.
fn ours(x: &Xc) -> impl Fn(u32) -> bool {
    let own = x.own();
    let mut set: Vec<u32> = own.panels.iter().copied().chain(own.sprites.keys().copied()).collect();
    drop(own);
    set.extend(x.own_frames(false));
    move |w| set.contains(&w)
}

impl PanelWindow for Panel {
    fn target(&self) -> Arc<PanelTarget> {
        self.target.clone().expect("panel target present until destroy")
    }

    fn commit(&self) {
        let _ = self.x.conn.flush();
    }

    fn show(&self, on: bool) {
        let c = &self.x.conn;
        let _ = if on { c.map_window(self.win) } else { c.unmap_window(self.win) };
        let _ = c.flush();
    }

    /// `Front`: top of the root stack unless only our own windows lie above.
    /// `Behind`: right above the desktop window, else the bottom (wallpaper
    /// on the root).
    fn stack(&self, s: Stack) {
        let x = self.x;
        let stack = x.stack();
        match s {
            Stack::Front => {
                if logic::needs_front(&stack, self.win, ours(x)) {
                    x.restack(self.win, None, StackMode::ABOVE);
                }
            }
            Stack::Behind => {
                let base = x.desktop_frame();
                if !logic::desktop_run_ok(&stack, base, &[self.win]) {
                    match base {
                        Some(d) => x.restack(self.win, Some(d), StackMode::ABOVE),
                        None => x.restack(self.win, None, StackMode::BELOW),
                    }
                }
            }
        }
        let _ = x.conn.flush();
    }

    fn id(&self) -> isize {
        self.win as isize
    }

    fn destroy(mut self: Box<Self>) {
        drop(self.target.take());
        let x = self.x;
        x.own().panels.remove(&self.win);
        let _ = x.conn.destroy_window(self.win);
        let _ = x.conn.flush();
    }
}

/// The X11 overlay backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct X11Overlay;

impl OverlayBackend for X11Overlay {
    const BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN.union(wgpu::Backends::GL);

    /// Mesa offers pre- or post-multiplied on ARGB X windows; pick from caps.
    fn alpha_mode(&self, caps: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode {
        let m = pick_alpha(&caps.alpha_modes);
        log::info!("x11 overlay: alpha modes {:?} -> {m:?}", caps.alpha_modes);
        m
    }

    fn create_panel(
        &self,
        instance: &wgpu::Instance,
        pixels: PixelRect,
        swap: (u32, u32),
        _scale: f64,
    ) -> Result<Box<dyn PanelWindow>, String> {
        let x = x::get().ok_or("no X display")?;
        panel(x, instance, pixels, swap).map(|p| Box::new(p) as Box<dyn PanelWindow>)
    }

    /// `RaisePanels`: our topmost sprites and always-on-top cards end above
    /// every panel. Cards are WM-managed, so panels go below the lowest card
    /// frame instead of raising the card.
    fn raise_own(&self, panels: &[isize], under_dock: &[isize]) {
        let Some(x) = x::get() else { return };
        let stack = x.stack();
        let panels: Vec<u32> = panels.iter().map(|p| *p as u32).collect();
        let under: Vec<u32> = under_dock.iter().map(|p| *p as u32).collect();
        let sprites: Vec<u32> =
            x.own().sprites.iter().filter(|(_, (top, mapped))| *top && *mapped).map(|(w, _)| *w).collect();
        let cards = x.own_frames(true);
        let plan = logic::raise_plan(&stack, &panels, &under, &sprites, &cards);
        if let Some(card) = plan.below {
            for p in &plan.lower {
                x.restack(*p, Some(card), StackMode::BELOW);
            }
        }
        for s in &plan.raise {
            x.restack(*s, None, StackMode::ABOVE);
        }
        let _ = x.conn.flush();
    }

    fn take_rescaled(&self) -> bool {
        watch::RESCALED.swap(false, Ordering::SeqCst)
    }

    /// No UI-thread timer on X11: the app ticks the host itself.
    fn start_ticker(&self, _: u32, _: Box<dyn FnMut()>) -> Option<fp_platform::Guard> {
        None
    }

    fn cursor_px() -> Option<(i32, i32)> {
        x::get()?.pointer().map(|(x, y, _, _)| (x, y))
    }
}
