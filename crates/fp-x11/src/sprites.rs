//! X11 sprite windows for `fp-layered` (porting design 3.2 "Sprites"): one
//! override-redirect depth-32 window per sprite whose background is a server
//! pixmap (moves need no repaint). Click-through = empty input shape; when
//! clickable the input shape follows the pixmap's alpha, like a Windows
//! layered window ignores fully transparent pixels. Fades premultiply on the
//! CPU because `_NET_WM_WINDOW_OPACITY` on override-redirect windows depends
//! on the compositor.

use crate::logic::{self, Rect16};
use crate::x::Xc;
use fp_core::geom::PixelRect;
use fp_platform::sprites::{Place, SpriteBackend, WinId};
use std::collections::HashMap;
use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux,
    EventMask, Gcontext, ImageFormat, Rectangle, StackMode, Window, WindowClass,
};

#[derive(Default)]
struct Sprite {
    pixmap: Option<(u32, u32, u32)>,
    rgba: Vec<u8>,
    input: Vec<Rect16>,
    alpha: u8,
    pos: Option<(i32, i32)>,
    visible: bool,
    click_through: bool,
}

/// X11 implementation of [`SpriteBackend`].
pub struct X11Sprites {
    x: &'static Xc,
    gc: Option<Gcontext>,
    wins: HashMap<Window, Sprite>,
    scratch: Vec<u8>,
}

fn rects(r: &[Rect16]) -> Vec<Rectangle> {
    r.iter().map(|r| Rectangle { x: r.x, y: r.y, width: r.w, height: r.h }).collect()
}

fn win(id: WinId) -> Window {
    id.0 as Window
}

impl X11Sprites {
    pub fn new(x: &'static Xc) -> Self {
        Self { x, gc: None, wins: HashMap::new(), scratch: Vec::new() }
    }

    fn set_input(&self, w: Window, r: &[Rect16]) {
        if self.x.shape {
            let _ = self.x.conn.shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, w, 0, 0, &rects(r));
        }
    }

    /// Uploads the sprite's RGBA (with its alpha) into its pixmap.
    fn upload(&mut self, w: Window) -> bool {
        let Some(s) = self.wins.get(&w) else { return false };
        let Some((pm, pw, ph)) = s.pixmap else { return false };
        let Some(gc) = self.gc else { return false };
        logic::to_bgra(&s.rgba, s.alpha, &mut self.scratch);
        let stride = pw as usize * 4;
        let max = self.x.conn.maximum_request_bytes().saturating_sub(64).max(stride);
        for (y, n) in logic::row_bands(ph, stride, max) {
            let data = &self.scratch[y as usize * stride..(y + n) as usize * stride];
            let r = self.x.conn.put_image(ImageFormat::Z_PIXMAP, pm, gc, pw as u16, n as u16, 0, y as i16, 0, 32, data);
            if r.is_err() {
                return false;
            }
        }
        let _ = self.x.conn.clear_area(false, w, 0, 0, 0, 0);
        true
    }

    fn own_sprite(&self, w: Window) -> bool {
        self.wins.contains_key(&w)
    }
}

impl SpriteBackend for X11Sprites {
    fn create(&mut self) -> Option<WinId> {
        let x = self.x;
        let (visual, cmap) = x.argb?;
        let c = &x.conn;
        let w = c.generate_id().ok()?;
        let aux = CreateWindowAux::new()
            .background_pixel(0)
            .border_pixel(0)
            .override_redirect(1)
            .colormap(cmap)
            .event_mask(EventMask::NO_EVENT);
        c.create_window(32, w, x.root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, visual, &aux).ok()?;
        x.tag_own(w, "Frostpane object window");
        if self.gc.is_none() {
            let gc = c.generate_id().ok()?;
            c.create_gc(gc, w, &CreateGCAux::new().graphics_exposures(0)).ok()?;
            self.gc = Some(gc);
        }
        self.wins.insert(w, Sprite { alpha: 255, click_through: true, ..Default::default() });
        self.set_input(w, &[]);
        x.own().sprites.insert(w, (false, false));
        x.own().click_through.insert(w);
        let _ = c.flush();
        Some(WinId(w as isize))
    }

    fn destroy(&mut self, id: WinId) {
        let w = win(id);
        if let Some(s) = self.wins.remove(&w) {
            if let Some((pm, _, _)) = s.pixmap {
                let _ = self.x.conn.free_pixmap(pm);
            }
            let _ = self.x.conn.destroy_window(w);
            let mut own = self.x.own();
            own.sprites.remove(&w);
            own.click_through.remove(&w);
            drop(own);
            let _ = self.x.conn.flush();
        }
    }

    fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool {
        let w = win(id);
        let x = self.x;
        let Some(s) = self.wins.get_mut(&w) else { return false };
        let (pw, ph) = (pm.width(), pm.height());
        if pw > 32767 || ph > 32767 {
            return false;
        }
        if s.pixmap.is_none_or(|(_, a, b)| a != pw || b != ph) {
            if let Some((old, _, _)) = s.pixmap.take() {
                let _ = x.conn.free_pixmap(old);
            }
            let Ok(p) = x.conn.generate_id() else { return false };
            if x.conn.create_pixmap(32, p, w, pw as u16, ph as u16).is_err() {
                return false;
            }
            s.pixmap = Some((p, pw, ph));
            let aux = ChangeWindowAttributesAux::new().background_pixmap(p);
            let _ = x.conn.change_window_attributes(w, &aux);
            let _ = x.conn.configure_window(w, &ConfigureWindowAux::new().width(pw).height(ph));
        }
        s.rgba.clear();
        s.rgba.extend_from_slice(pm.data());
        s.alpha = 255;
        let alpha: Vec<u8> = pm.data().as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        s.input = logic::alpha_rects(pw, ph, &alpha, 1);
        let click_through = s.click_through;
        let input = s.input.clone();
        if !click_through {
            self.set_input(w, &input);
        }
        if x.shape && !x.composited() {
            // No compositor: ARGB is drawn opaque, so cut the shape instead.
            let solid = logic::alpha_rects(pw, ph, &alpha, 128);
            let _ = x.conn.shape_rectangles(SO::SET, SK::BOUNDING, ClipOrdering::UNSORTED, w, 0, 0, &rects(&solid));
        }
        let ok = self.upload(w);
        let _ = x.conn.flush();
        ok
    }

    fn present(&mut self, id: WinId, px: i32, py: i32, alpha: u8) {
        let w = win(id);
        let Some(s) = self.wins.get_mut(&w) else { return };
        if s.pixmap.is_none() {
            return;
        }
        let refade = s.alpha != alpha;
        s.alpha = alpha;
        let moved = s.pos != Some((px, py));
        s.pos = Some((px, py));
        let show = !s.visible;
        s.visible = true;
        if refade {
            self.upload(w);
        }
        let c = &self.x.conn;
        if moved {
            let _ = c.configure_window(w, &ConfigureWindowAux::new().x(px).y(py));
        }
        if show {
            let _ = c.map_window(w);
            if let Some(e) = self.x.own().sprites.get_mut(&w) {
                e.1 = true;
            }
        }
        let _ = c.flush();
    }

    fn hide(&mut self, id: WinId) {
        let w = win(id);
        if let Some(s) = self.wins.get_mut(&w)
            && s.visible
        {
            s.visible = false;
            let _ = self.x.conn.unmap_window(w);
            if let Some(e) = self.x.own().sprites.get_mut(&w) {
                e.1 = false;
            }
            let _ = self.x.conn.flush();
        }
    }

    fn set_click_through(&mut self, id: WinId, on: bool) {
        let w = win(id);
        let Some(s) = self.wins.get_mut(&w) else { return };
        if s.click_through == on {
            return;
        }
        s.click_through = on;
        let input = if on { Vec::new() } else { s.input.clone() };
        self.set_input(w, &input);
        let mut own = self.x.own();
        if on {
            own.click_through.insert(w);
        } else {
            own.click_through.remove(&w);
        }
        drop(own);
        let _ = self.x.conn.flush();
    }

    fn place(&mut self, id: WinId, p: Place) {
        let w = win(id);
        if !self.own_sprite(w) {
            return;
        }
        let x = self.x;
        let set_top = |on: bool| {
            if let Some(e) = x.own().sprites.get_mut(&w) {
                e.0 = on;
            }
        };
        match p {
            Place::Topmost => {
                set_top(true);
                x.restack(w, None, StackMode::ABOVE);
            }
            Place::Top => x.restack(w, None, StackMode::ABOVE),
            Place::After(a) => x.restack(w, Some(win(a)), StackMode::BELOW),
            Place::NotTopmost => {
                set_top(false);
                // Top of the normal band: above the highest normal client frame.
                let ids = x.clients();
                let facts = x.facts(&ids);
                let normal = facts.iter().rev().find(|f| {
                    f.mapped && !f.state.above && !logic::primary_type(&f.types).skipped() && f.pid != Some(x.pid)
                });
                if let Some(f) = normal {
                    x.restack(w, Some(x.frame_of(f.id)), StackMode::ABOVE);
                }
            }
        }
        let _ = x.conn.flush();
    }

    fn lowest_taskbar(&self) -> Option<WinId> {
        let d = self.x.docks().into_iter().next()?;
        Some(WinId(self.x.frame_of(d) as isize))
    }

    fn taskbar_above(&self, id: WinId) -> bool {
        let stack = self.x.stack();
        let me = win(id);
        self.x.docks().iter().any(|d| logic::is_above(&stack, self.x.frame_of(*d), me))
    }

    fn covered(&self, id: WinId, _cursor: (i32, i32)) -> bool {
        let me = win(id);
        let Some((_, _, _, child)) = self.x.pointer() else { return false };
        if child == 0 || child == me || self.own_sprite(child) || self.x.own().panels.contains(&child) {
            return false;
        }
        logic::is_above(&self.x.stack(), child, me)
    }

    fn fullscreen(&self) -> Vec<PixelRect> {
        let x = self.x;
        let Some(act) = x.active_window() else { return Vec::new() };
        let facts = x.facts(&[act]);
        let Some(f) = facts.first() else { return Vec::new() };
        if f.pid == Some(x.pid) || logic::primary_type(&f.types).skipped() {
            return Vec::new();
        }
        f.geometry
            .and_then(|(gx, gy, w, h)| logic::frame_rect(gx, gy, w, h, f.net_extents, f.gtk_extents))
            .into_iter()
            .collect()
    }

    fn raise_front(&mut self, ids: &[WinId]) {
        let own = self.x.own();
        let want: Vec<Window> = ids
            .iter()
            .map(|i| win(*i))
            .filter(|w| own.sprites.get(w).is_some_and(|(top, mapped)| *top && *mapped))
            .collect();
        drop(own);
        if want.is_empty() {
            return;
        }
        for w in self.x.stack().into_iter().filter(|w| want.contains(w)) {
            self.x.restack(w, None, StackMode::ABOVE);
        }
        let _ = self.x.conn.flush();
    }

    fn stack_desktop(&mut self, ids: &[WinId]) {
        let ids: Vec<Window> = ids.iter().map(|i| win(*i)).filter(|w| self.own_sprite(*w)).collect();
        if ids.is_empty() {
            return;
        }
        let x = self.x;
        let base = x.desktop_frame();
        if logic::desktop_run_ok(&x.stack(), base, &ids) {
            return;
        }
        let mut prev = base;
        for w in ids {
            match prev {
                Some(p) => x.restack(w, Some(p), StackMode::ABOVE),
                None => x.restack(w, None, StackMode::BELOW),
            }
            prev = Some(w);
        }
        let _ = x.conn.flush();
    }

    fn pump(&mut self) {
        self.x.drain();
    }
}

impl Drop for X11Sprites {
    fn drop(&mut self) {
        let ids: Vec<Window> = self.wins.keys().copied().collect();
        for w in ids {
            self.destroy(WinId(w as isize));
        }
        if let Some(gc) = self.gc.take() {
            let _ = self.x.conn.free_gc(gc);
        }
        let _ = self.x.conn.flush();
    }
}
