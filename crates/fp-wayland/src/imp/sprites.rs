//! Sprite (object) windows as small layer surfaces (design 3.3): anchored
//! top-left on the output under the sprite, positioned by margins, with a
//! `wl_shm` ARGB8888 buffer and an input region from the bitmap's alpha.
//!
//! Bands: `Topmost`/`Top` = overlay layer, `After(_)` (under the dock) =
//! top layer, `NotTopmost` and [`SpriteBackend::stack_desktop`] = bottom
//! layer. A layer surface lives on one output: a sprite that moves to
//! another output gets a new surface, and one that straddles two outputs is
//! clipped at the edge.
//!
//! Click-through: there is no global pointer on Wayland, so a sprite only
//! learns about the pointer through its own input region. A click-through
//! sprite therefore keeps an input region over its opaque pixels (alpha
//! mask, rows in 2 px bands) so hover can be detected at all; a clickable
//! one takes its whole rect. Sprites that never take input (moving
//! characters, `HitMode::Never` through [`SpriteBackend::set_hit_mode`])
//! get an empty input region, so clicks fall through to the window below.

use super::Shared;
use super::overlay::NAMESPACE;
use super::wl::Wl;
use crate::logic::{self, Placement, Snapshot};
use fp_core::geom::PixelRect;
use fp_core::scene::HitMode;
use fp_platform::sprites::{Place, SpriteBackend, WinId};
use sctk::shell::WaylandSurface;
use sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};
use sctk::shm::slot::Buffer;
use smithay_client_toolkit as sctk;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tiny_skia::Pixmap;
use wayland_client::Proxy;
use wayland_client::protocol::wl_shm;
use wayland_protocols::wp::viewporter::client::wp_viewport;

/// Mask band height in buffer pixels (design 3.3 "rows at 2 px steps").
const MASK_STEP: u32 = 2;

struct Mapped {
    layer: LayerSurface,
    viewport: Option<wp_viewport::WpViewport>,
    output: String,
    place: Placement,
    band: Layer,
}

#[derive(Default)]
struct Sprite {
    /// Premultiplied RGBA of the current bitmap.
    rgba: Vec<u8>,
    size: (i32, i32),
    /// Opaque runs in buffer pixels.
    mask: Vec<(i32, i32, i32, i32)>,
    buffer: Option<Buffer>,
    /// Alpha the buffer was made with.
    buffer_alpha: u8,
    click_through: bool,
    /// `HitMode::Never`: no input region at all.
    never: bool,
    band: Option<Layer>,
    mapped: Option<Mapped>,
    /// Last placement request (pixels), for re-mapping.
    at: Option<(i32, i32, u8)>,
}

/// The Wayland sprite backend; bookkeeping only without a connection.
pub struct WlSprites {
    pub(super) wl: Option<Shared>,
    pub(super) snap: Rc<RefCell<Snapshot>>,
    next: isize,
    sprites: HashMap<isize, Sprite>,
}

impl WlSprites {
    pub(super) fn new(wl: Option<Shared>, snap: Rc<RefCell<Snapshot>>) -> Self {
        Self { wl, snap, next: 0, sprites: HashMap::new() }
    }
}

fn band_of(p: Place) -> Layer {
    match p {
        Place::Topmost | Place::Top => Layer::Overlay,
        Place::After(_) => Layer::Top,
        Place::NotTopmost => Layer::Bottom,
    }
}

/// A buffer of `sp`'s bitmap with `alpha` applied.
fn make_buffer(wl: &mut Wl, sp: &Sprite, alpha: u8) -> Option<Buffer> {
    let (w, h) = sp.size;
    let pool = wl.st.pool.as_mut()?;
    let (buf, canvas) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888).ok()?;
    logic::rgba_to_argb(&sp.rgba, canvas);
    if alpha < 255 {
        let a = u16::from(alpha);
        for b in canvas.iter_mut() {
            *b = ((u16::from(*b) * a + 127) / 255) as u8;
        }
    }
    Some(buf)
}

fn input_region(wl: &Wl, sp: &Sprite, m: &Mapped) {
    let rects = if sp.never {
        Vec::new()
    } else if sp.click_through {
        logic::scale_rects(&sp.mask, wl.st.scale)
    } else {
        vec![(0, 0, m.place.w, m.place.h)]
    };
    if let Some(r) = wl.region(&rects) {
        m.layer.wl_surface().set_input_region(Some(r.wl_region()));
    }
}

/// Attaches the buffer and sizes the surface (viewport or buffer scale).
fn attach(wl: &mut Wl, sp: &mut Sprite, alpha: u8) {
    if sp.buffer.is_none() || sp.buffer_alpha != alpha {
        sp.buffer = make_buffer(wl, sp, alpha);
        sp.buffer_alpha = alpha;
    }
    let Some(m) = &sp.mapped else { return };
    let surf = m.layer.wl_surface();
    let ok = sp.buffer.as_ref().is_some_and(|b| b.attach_to(surf).is_ok());
    if !ok {
        // The compositor still holds it: draw a fresh one.
        sp.buffer = make_buffer(wl, sp, alpha);
        if let Some(b) = &sp.buffer {
            let _ = b.attach_to(surf);
        }
    }
    match &m.viewport {
        Some(vp) => vp.set_destination(m.place.w, m.place.h),
        None => surf.set_buffer_scale(wl.st.scale.round().max(1.0) as i32),
    }
    surf.damage_buffer(0, 0, sp.size.0, sp.size.1);
}

fn unmap(wl: &mut Wl, sp: &mut Sprite) {
    if let Some(m) = sp.mapped.take() {
        let id = m.layer.wl_surface().id();
        wl.st.origins.remove(&id);
        wl.st.own_rects.remove(&id);
        wl.st.configured.remove(&id);
        wl.st.closed.remove(&id);
        if let Some(vp) = m.viewport {
            vp.destroy();
        }
        drop(m.layer);
    }
}

impl WlSprites {
    fn present_in(wl: &mut Wl, sp: &mut Sprite, x: i32, y: i32, alpha: u8) {
        sp.at = Some((x, y, alpha));
        if sp.size.0 <= 0 || sp.rgba.is_empty() {
            return;
        }
        let outs = wl.outputs();
        let list: Vec<logic::Output> = outs.iter().map(|(_, o)| o.clone()).collect();
        let s = fp_platform::coords::global_scale(&list.iter().map(|o| o.scale).collect::<Vec<_>>());
        wl.st.scale = s;
        let Some(place) = logic::place(&list, s, x, y, sp.size.0, sp.size.1) else { return };
        let band = sp.band.unwrap_or(Layer::Overlay);
        let out_name = list[place.output].name.clone();
        let remap = sp
            .mapped
            .as_ref()
            .is_none_or(|m| m.output != out_name || wl.st.closed.contains(&m.layer.wl_surface().id()));
        if remap {
            unmap(wl, sp);
            let surface = wl.st.compositor.create_surface(&wl.qh);
            let layer =
                wl.st.layer.create_layer_surface(&wl.qh, surface, band, Some(NAMESPACE), Some(&outs[place.output].0));
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.set_size(place.w as u32, place.h as u32);
            layer.set_margin(place.top, 0, 0, place.left);
            layer.commit();
            if wl.wait_configure(layer.wl_surface()).is_none() {
                log::warn!("sprite layer surface not configured");
                return;
            }
            let viewport = wl.viewport(layer.wl_surface());
            sp.mapped = Some(Mapped { layer, viewport, output: out_name, place, band });
            sp.buffer_alpha = 0;
            attach(wl, sp, alpha);
        } else if let Some(m) = &mut sp.mapped {
            if m.place.w != place.w || m.place.h != place.h {
                m.layer.set_size(place.w as u32, place.h as u32);
            }
            if (m.place.left, m.place.top) != (place.left, place.top) {
                m.layer.set_margin(place.top, 0, 0, place.left);
            }
            if m.band != band {
                m.layer.set_layer(band);
                m.band = band;
            }
            m.place = place;
            if sp.buffer_alpha != alpha || sp.buffer.is_none() {
                attach(wl, sp, alpha);
            }
        }
        let Some(m) = &sp.mapped else { return };
        input_region(wl, sp, m);
        m.layer.commit();
        let id = m.layer.wl_surface().id();
        let origin = place.origin(&list);
        wl.st.origins.insert(id.clone(), origin);
        let rect = fp_platform::coords::points_to_pixels(origin.0, origin.1, f64::from(place.w), f64::from(place.h), s);
        wl.st.own_rects.insert(id, rect);
    }
}

impl SpriteBackend for WlSprites {
    fn create(&mut self) -> Option<WinId> {
        self.next += 1;
        self.sprites.insert(self.next, Sprite { click_through: true, buffer_alpha: 255, ..Default::default() });
        Some(WinId(self.next))
    }

    fn destroy(&mut self, id: WinId) {
        if let Some(mut sp) = self.sprites.remove(&id.0)
            && let Some(wl) = &self.wl
        {
            unmap(&mut wl.borrow_mut(), &mut sp);
        }
    }

    fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool {
        let Some(sp) = self.sprites.get_mut(&id.0) else { return false };
        sp.rgba = pm.data().to_vec();
        sp.size = (pm.width() as i32, pm.height() as i32);
        sp.mask = logic::mask_rects(&sp.rgba, pm.width(), pm.height(), MASK_STEP);
        sp.buffer = None;
        let Some(wl) = &self.wl else { return true };
        let mut wl = wl.borrow_mut();
        if sp.mapped.is_some() {
            let (x, y, a) = sp.at.unwrap_or((0, 0, 255));
            Self::present_in(&mut wl, sp, x, y, a);
        }
        true
    }

    fn present(&mut self, id: WinId, x: i32, y: i32, alpha: u8) {
        let Some(sp) = self.sprites.get_mut(&id.0) else { return };
        match &self.wl {
            Some(wl) => Self::present_in(&mut wl.borrow_mut(), sp, x, y, alpha),
            None => sp.at = Some((x, y, alpha)),
        }
    }

    fn hide(&mut self, id: WinId) {
        if let Some(sp) = self.sprites.get_mut(&id.0)
            && let Some(wl) = &self.wl
        {
            unmap(&mut wl.borrow_mut(), sp);
        }
    }

    fn set_click_through(&mut self, id: WinId, on: bool) {
        let Some(sp) = self.sprites.get_mut(&id.0) else { return };
        if sp.click_through == on {
            return;
        }
        sp.click_through = on;
        if let (Some(wl), Some(m)) = (&self.wl, &sp.mapped) {
            let wl = wl.borrow();
            input_region(&wl, sp, m);
            m.layer.commit();
        }
    }

    fn set_hit_mode(&mut self, id: WinId, mode: HitMode) {
        let Some(sp) = self.sprites.get_mut(&id.0) else { return };
        let never = mode == HitMode::Never;
        if sp.never == never {
            return;
        }
        sp.never = never;
        if let (Some(wl), Some(m)) = (&self.wl, &sp.mapped) {
            let wl = wl.borrow();
            input_region(&wl, sp, m);
            m.layer.commit();
        }
    }

    fn place(&mut self, id: WinId, p: Place) {
        let Some(sp) = self.sprites.get_mut(&id.0) else { return };
        let band = band_of(p);
        sp.band = Some(band);
        if let Some(m) = &mut sp.mapped
            && m.band != band
        {
            m.band = band;
            m.layer.set_layer(band);
            m.layer.commit();
        }
    }

    fn lowest_taskbar(&self) -> Option<WinId> {
        None
    }

    fn taskbar_above(&self, _: WinId) -> bool {
        false
    }

    /// Pointer events reach a sprite only where it is on top.
    fn covered(&self, _: WinId, _: (i32, i32)) -> bool {
        false
    }

    /// Fullscreen windows reported by the window source.
    fn fullscreen(&self) -> Vec<PixelRect> {
        let s = self.wl.as_ref().map_or(1.0, |w| w.borrow().st.scale);
        self.snap.borrow().fullscreen(s)
    }

    /// Restacking inside a layer is not possible: no-op.
    fn raise_front(&mut self, _: &[WinId]) {}

    fn stack_desktop(&mut self, ids: &[WinId]) {
        for id in ids {
            self.place(*id, Place::NotTopmost);
        }
    }

    fn pump(&mut self) {
        let Some(wl) = &self.wl else { return };
        let mut wl = wl.borrow_mut();
        wl.dispatch();
        // Surfaces the compositor closed are made again on the next present.
        if !wl.st.closed.is_empty() {
            for sp in self.sprites.values_mut() {
                if sp.mapped.as_ref().is_some_and(|m| wl.st.closed.contains(&m.layer.wl_surface().id())) {
                    unmap(&mut wl, sp);
                    if let Some((x, y, a)) = sp.at {
                        Self::present_in(&mut wl, sp, x, y, a);
                    }
                }
            }
        }
    }
}
