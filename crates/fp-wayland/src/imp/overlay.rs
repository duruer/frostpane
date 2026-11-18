//! Overlay panels as layer-shell surfaces (design 3.3): one per output on
//! the overlay layer (bottom layer in behind mode), anchored to every edge,
//! exclusive zone -1, no keyboard focus and an empty input region.
//!
//! The layer surface itself only carries a transparent 1x1 buffer stretched
//! by `wp_viewporter` (a full-size one without it); wgpu presents into a
//! desynchronised subsurface. So the UI thread can hide and restack a panel
//! without touching the surface the render thread presents to.

use super::Shared;
use super::wl::Wl;
use crate::logic;
use fp_core::geom::PixelRect;
use fp_platform::overlay::{OverlayBackend, PanelTarget, PanelWindow, Stack, pick_alpha};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use sctk::shell::WaylandSurface;
use sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};
use sctk::shm::slot::Buffer;
use smithay_client_toolkit as sctk;
use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::Arc;
use wayland_client::Proxy;
use wayland_client::protocol::{wl_shm, wl_subsurface, wl_surface};
use wayland_protocols::wp::viewporter::client::wp_viewport;

/// Layer-shell namespace of every surface we create.
pub const NAMESPACE: &str = "frostpane";

/// The Wayland overlay backend. `None` inside: no usable connection, every
/// panel fails (the app runs without overlay).
#[derive(Clone)]
pub struct WlOverlay {
    pub(super) wl: Option<Shared>,
    /// Output layout the panels were made for (rebuild on change).
    pub(super) layout: std::rc::Rc<std::cell::RefCell<Vec<logic::Output>>>,
}

/// A transparent ARGB buffer of `w`x`h` from the shared pool.
pub(super) fn clear_buffer(wl: &mut Wl, w: i32, h: i32) -> Option<Buffer> {
    let pool = wl.st.pool.as_mut()?;
    let (buf, canvas) = pool.create_buffer(w.max(1), h.max(1), w.max(1) * 4, wl_shm::Format::Argb8888).ok()?;
    canvas.fill(0);
    Some(buf)
}

fn setup_layer(wl: &Wl, layer: &LayerSurface) {
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    if let Some(r) = wl.empty_region() {
        layer.wl_surface().set_input_region(Some(r.wl_region()));
    }
}

struct Panel {
    wl: Shared,
    layer: LayerSurface,
    parent_buf: Option<Buffer>,
    parent_vp: Option<wp_viewport::WpViewport>,
    sub: wl_subsurface::WlSubsurface,
    child: wl_surface::WlSurface,
    child_vp: Option<wp_viewport::WpViewport>,
    target: Option<Arc<PanelTarget>>,
    size: (i32, i32),
    shown: Cell<bool>,
    behind: Cell<bool>,
}

impl Panel {
    /// Attaches the parent's transparent buffer (maps the layer surface).
    fn map_parent(&mut self, wl: &mut Wl) {
        let (w, h) = if self.parent_vp.is_some() { (1, 1) } else { self.size };
        if self.parent_buf.is_none() {
            self.parent_buf = clear_buffer(wl, w, h);
        }
        let surf = self.layer.wl_surface();
        if let Some(b) = &self.parent_buf
            && b.attach_to(surf).is_err()
        {
            // Still held by the compositor: use a fresh one.
            self.parent_buf = clear_buffer(wl, w, h);
            if let Some(b) = &self.parent_buf {
                let _ = b.attach_to(surf);
            }
        }
        if let Some(vp) = &self.parent_vp {
            vp.set_destination(self.size.0, self.size.1);
        }
        surf.damage_buffer(0, 0, w, h);
        surf.commit();
    }
}

impl PanelWindow for Panel {
    fn target(&self) -> Arc<PanelTarget> {
        self.target.clone().expect("panel target lives until destroy")
    }

    fn commit(&self) {
        self.wl.borrow_mut().dispatch();
    }

    /// Hidden: the content subsurface shrinks to one logical pixel (it stays
    /// mapped so a paused Fifo swap chain still gets frame callbacks);
    /// without viewporter the layer surface is unmapped and mapped again.
    fn show(&self, on: bool) {
        if self.shown.replace(on) == on {
            return;
        }
        let mut wl = self.wl.borrow_mut();
        if let Some(vp) = &self.child_vp {
            if on {
                vp.set_destination(self.size.0, self.size.1);
            } else {
                vp.set_destination(1, 1);
            }
            self.child.commit();
        } else if on {
            setup_layer(&wl, &self.layer);
            self.layer.set_layer(if self.behind.get() { Layer::Bottom } else { Layer::Overlay });
            wl.st.configured.remove(&self.layer.wl_surface().id());
            self.layer.commit();
            let _ = wl.wait_configure(self.layer.wl_surface());
            // Re-map with the same buffer and size.
            let surf = self.layer.wl_surface();
            if let Some(b) = &self.parent_buf {
                let _ = b.attach_to(surf);
            }
            surf.damage_buffer(0, 0, self.size.0, self.size.1);
            surf.commit();
        } else {
            let surf = self.layer.wl_surface();
            surf.attach(None, 0, 0);
            surf.commit();
        }
        wl.dispatch();
    }

    fn stack(&self, s: Stack) {
        let behind = s == Stack::Behind;
        if self.behind.replace(behind) == behind {
            return;
        }
        self.layer.set_layer(if behind { Layer::Bottom } else { Layer::Overlay });
        self.layer.commit();
        self.wl.borrow_mut().dispatch();
    }

    fn id(&self) -> isize {
        self.layer.wl_surface().id().protocol_id() as isize
    }

    /// The wgpu surface first, then the subsurface, then the layer surface.
    fn destroy(mut self: Box<Self>) {
        drop(self.target.take());
        if let Some(vp) = self.child_vp.take() {
            vp.destroy();
        }
        self.sub.destroy();
        self.child.destroy();
        if let Some(vp) = self.parent_vp.take() {
            vp.destroy();
        }
        let wl = self.wl.clone();
        drop(self);
        wl.borrow_mut().dispatch();
    }
}

fn make_panel(shared: &Shared, instance: &wgpu::Instance, pixels: PixelRect) -> Result<Panel, String> {
    let mut wl = shared.borrow_mut();
    wl.dispatch();
    let outs = wl.outputs();
    let list: Vec<logic::Output> = outs.iter().map(|(_, o)| o.clone()).collect();
    let s = fp_platform::coords::global_scale(&list.iter().map(|o| o.scale).collect::<Vec<_>>());
    wl.st.scale = s;
    let i = logic::output_for_pixels(&list, s, pixels).ok_or("no wl_output matches the panel's monitor")?;
    let (output, o) = &outs[i];
    let sub_c = wl.st.subcompositor.as_ref().ok_or("compositor has no wl_subcompositor")?;
    let surface = wl.st.compositor.create_surface(&wl.qh);
    let layer = wl.st.layer.create_layer_surface(&wl.qh, surface, Layer::Overlay, Some(NAMESPACE), Some(output));
    let (sub, child) = sub_c.create_subsurface(layer.wl_surface().clone(), &wl.qh);
    setup_layer(&wl, &layer);
    layer.commit();
    let conf = wl.wait_configure(layer.wl_surface()).ok_or("layer surface was not configured")?;
    let size = (if conf.0 > 0 { conf.0 as i32 } else { o.w }, if conf.1 > 0 { conf.1 as i32 } else { o.h });
    let parent_vp = wl.viewport(layer.wl_surface());
    let child_vp = wl.viewport(&child);
    sub.set_position(0, 0);
    sub.set_desync();
    if let Some(r) = wl.empty_region() {
        child.set_input_region(Some(r.wl_region()));
    }
    if let Some(vp) = &child_vp {
        vp.set_destination(size.0, size.1);
    }
    child.commit();
    let mut panel = Panel {
        wl: shared.clone(),
        layer,
        parent_buf: None,
        parent_vp,
        sub,
        child,
        child_vp,
        target: None,
        size,
        shown: Cell::new(true),
        behind: Cell::new(false),
    };
    panel.map_parent(&mut wl);
    let display = NonNull::new(wl.display_ptr()).ok_or("no wl_display")?;
    let surf = NonNull::new(panel.child.id().as_ptr().cast()).ok_or("no wl_surface")?;
    // SAFETY: the display outlives every panel (the connection is shared and
    // dropped last), and the child surface is destroyed only after the wgpu
    // surface in `destroy`.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display))),
            raw_window_handle: RawWindowHandle::Wayland(WaylandWindowHandle::new(surf)),
        })
    }
    .map_err(|e| e.to_string())?;
    panel.target = Some(Arc::new(PanelTarget { surface }));
    wl.dispatch();
    log::info!("wayland panel on {} ({}x{} logical, scale {s})", o.name, size.0, size.1);
    Ok(panel)
}

impl OverlayBackend for WlOverlay {
    const BACKENDS: wgpu::Backends = wgpu::Backends::VULKAN.union(wgpu::Backends::GL);

    fn alpha_mode(&self, caps: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode {
        pick_alpha(&caps.alpha_modes)
    }

    fn create_panel(
        &self,
        instance: &wgpu::Instance,
        pixels: PixelRect,
        _swap: (u32, u32),
        _scale: f64,
    ) -> Result<Box<dyn PanelWindow>, String> {
        let shared = self.wl.as_ref().ok_or("no Wayland layer-shell connection")?;
        let p = make_panel(shared, instance, pixels)?;
        *self.layout.borrow_mut() = shared.borrow().outputs().into_iter().map(|(_, o)| o).collect();
        Ok(Box::new(p))
    }

    /// Our cards and sprites cannot be restacked inside a layer: no-op.
    fn raise_own(&self, _: &[isize], _: &[isize]) {}

    /// The output layout changed since the panels were made.
    fn take_rescaled(&self) -> bool {
        let Some(shared) = &self.wl else { return false };
        let mut wl = shared.borrow_mut();
        wl.dispatch();
        if !std::mem::take(&mut wl.st.outputs_dirty) {
            return false;
        }
        let now: Vec<logic::Output> = wl.outputs().into_iter().map(|(_, o)| o).collect();
        let mut old = self.layout.borrow_mut();
        if old.is_empty() || *old == now {
            return false;
        }
        *old = now;
        true
    }

    fn start_ticker(&self, _: u32, _: Box<dyn FnMut()>) -> Option<fp_platform::Guard> {
        None
    }

    fn cursor_px() -> Option<(i32, i32)> {
        super::wl::cursor()
    }
}
