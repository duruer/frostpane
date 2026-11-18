//! Free-placed desktop decorations (string lights, candles, baubles...).
//!
//! Spec: decorations 1-5 (catalogue, picker model, add, initial spot,
//! window model, hover/hit/cover rules as data, drag/resize/stretch/remove,
//! ambient animation, z-order intent, removal, icons, placement helpers,
//! Strand use, art), 8 (persistence `decorations`), 10 (interfaces).
//! Frostpane changes: always enabled; the cap of 30 becomes
//! `Settings::max_placed`; 6 (ItemInfo for item gifts) is dropped.
//!
//! The world is pure: [`Decorations::step`] consumes a [`PointerInput`] and
//! `fp-layered` shows [`Decorations::sprites`] (paint with
//! [`Decorations::paint`], hit-test with [`Decorations::solid`]).
//! Coordinates are global points, Y up; art is Y up with the origin at the
//! bottom-left of the art box.

mod art;
mod motion;
mod render;

use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng, mix_seed};
use fp_core::scene::{HitMode, Owner, PointerInput, Sprite, SpriteKey, ZBand};
pub use fp_paint::Strand;
use serde_json::{Map, Value};
use std::cell::RefCell;

pub use render::{AlphaMask, Look};

/// Padding around the art on every side of the window box (decorations 2).
pub const PAD: f64 = 12.0;
/// Zoom limits for non-lights.
pub const ZOOM_MIN: f64 = 0.5;
pub const ZOOM_MAX: f64 = 3.0;
const POP_IN_SECS: f64 = 0.55;
const POP_OUT_SECS: f64 = 0.2;
/// Hover fade of the button, grips and grow (decorations 3.5, 3.6).
const HOVER_SECS: f64 = 0.15;
/// Sway frame rate (decorations 3.6a).
const SWAY_FPS: f64 = 24.0;
const SCREEN_INSET: f64 = 30.0;

/// Catalogue in picker order (decorations 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Lights,
    Candle,
    RedBauble,
    GoldBauble,
    Star,
    Wreath,
    Stocking,
    CandyCane,
    Gift,
    Bells,
    Gingerbread,
    Holly,
    SantaHat,
    SnowGlobe,
    Cocoa,
    Boat,
}

impl Kind {
    pub const ALL: [Kind; 16] = [
        Kind::Lights,
        Kind::Candle,
        Kind::RedBauble,
        Kind::GoldBauble,
        Kind::Star,
        Kind::Wreath,
        Kind::Stocking,
        Kind::CandyCane,
        Kind::Gift,
        Kind::Bells,
        Kind::Gingerbread,
        Kind::Holly,
        Kind::SantaHat,
        Kind::SnowGlobe,
        Kind::Cocoa,
        Kind::Boat,
    ];

    /// Persisted key (first letter lowercased).
    pub fn raw(self) -> &'static str {
        match self {
            Kind::Lights => "lights",
            Kind::Candle => "candle",
            Kind::RedBauble => "redBauble",
            Kind::GoldBauble => "goldBauble",
            Kind::Star => "star",
            Kind::Wreath => "wreath",
            Kind::Stocking => "stocking",
            Kind::CandyCane => "candyCane",
            Kind::Gift => "gift",
            Kind::Bells => "bells",
            Kind::Gingerbread => "gingerbread",
            Kind::Holly => "holly",
            Kind::SantaHat => "santaHat",
            Kind::SnowGlobe => "snowGlobe",
            Kind::Cocoa => "cocoa",
            Kind::Boat => "boat",
        }
    }

    /// Exact, case-sensitive parse of a persisted key.
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.raw() == s)
    }

    /// Art size in points (lights depend on the strand; `None` is the
    /// implicit 260 x 18 default).
    pub fn art_size(self, strand: Option<Strand>) -> (f64, f64) {
        match self {
            Kind::Lights => strand.unwrap_or(Strand::LIGHTS).art_size(),
            Kind::Candle => (52.0, 112.0),
            Kind::RedBauble | Kind::GoldBauble => (60.0, 128.0),
            Kind::Star => (72.0, 72.0),
            Kind::Wreath => (100.0, 100.0),
            Kind::Stocking => (70.0, 98.0),
            Kind::CandyCane => (52.0, 100.0),
            Kind::Gift => (80.0, 82.0),
            Kind::Bells => (86.0, 82.0),
            Kind::Gingerbread => (72.0, 86.0),
            Kind::Holly => (86.0, 62.0),
            Kind::SantaHat => (94.0, 76.0),
            Kind::SnowGlobe => (76.0, 94.0),
            Kind::Cocoa => (82.0, 88.0),
            Kind::Boat => (96.0, 96.0),
        }
    }

    /// Sway pivot in art coordinates (Y up).
    pub fn pivot(self, strand: Option<Strand>) -> (f64, f64) {
        let (w, h) = self.art_size(strand);
        match self {
            Kind::Lights => (w / 2.0, h - 8.0),
            Kind::RedBauble | Kind::GoldBauble => (30.0, 125.0),
            Kind::Star => (36.0, 36.0),
            Kind::Wreath => (50.0, 92.0),
            Kind::Stocking => (12.0, 94.0),
            Kind::CandyCane => (23.0, 80.0),
            Kind::Bells => (43.0, 76.0),
            Kind::Boat => (48.0, 4.0),
            _ => (w / 2.0, 2.0),
        }
    }

    /// Sway amplitude in radians (0 = does not sway).
    pub fn sway_angle(self) -> f64 {
        match self {
            Kind::Lights => 0.0,
            Kind::RedBauble | Kind::GoldBauble => 0.045,
            Kind::Star | Kind::CandyCane => 0.035,
            Kind::Wreath => 0.02,
            Kind::Stocking => 0.03,
            Kind::Bells | Kind::Boat => 0.04,
            _ => 0.018,
        }
    }
}

/// One placed decoration.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: u64,
    pub kind: Kind,
    /// Centre in global points.
    pub center: Vec2,
    pub zoom: f64,
    /// Explicit strand; only lights that were stretched (or loaded with a
    /// `length`) carry one.
    pub strand: Option<Strand>,
}

impl Placed {
    /// Unzoomed window box (art + padding).
    pub fn box_size(&self) -> (f64, f64) {
        render::box_size(self.kind, self.strand)
    }

    /// Window size: `ceil(box * zoom)` per axis.
    pub fn outer(&self) -> (f64, f64) {
        render::outer_size(self.kind, self.strand, self.zoom)
    }

    /// Window frame in global points (Y up) centred on [`Placed::center`].
    pub fn frame(&self) -> Frame {
        let (w, h) = self.outer();
        Frame::new(self.center.x - w / 2.0, self.center.y - h / 2.0, w, h)
    }
}

/// Keeps `p` if inside a screen, else clamps it into the nearest screen
/// inset by 30 (decorations 3.10). No usable screen leaves it unchanged.
pub fn on_screen(p: Vec2, screens: &[Frame]) -> Vec2 {
    if screens.iter().any(|f| f.contains(p)) {
        return p;
    }
    let mut best: Option<&Frame> = None;
    let mut best_d = f64::INFINITY;
    for f in screens {
        let d = f.dist_sq(p);
        if d < best_d {
            best_d = d;
            best = Some(f);
        }
    }
    match best {
        Some(f) if f.w > 0.0 => Vec2::new(
            p.x.max(f.min_x() + SCREEN_INSET).min(f.max_x() - SCREEN_INSET),
            p.y.max(f.min_y() + SCREEN_INSET).min(f.max_y() - SCREEN_INSET),
        ),
        _ => p,
    }
}

fn overlap(a: &Frame, b: &Frame) -> f64 {
    let w = a.max_x().min(b.max_x()) - a.min_x().max(b.min_x());
    let h = a.max_y().min(b.max_y()) - a.min_y().max(b.min_y());
    if w <= 0.0 || h <= 0.0 { 0.0 } else { w * h }
}

#[derive(Debug)]
struct Item {
    p: Placed,
    rev: u64,
    phase: f64,
    sway_dur: f64,
    sway_off: f64,
    /// Seconds since a user add (pop-in running).
    born: Option<f64>,
    /// Seconds since removal (pop-out running).
    dying: Option<f64>,
    /// Hover chrome fade, 0..1 (0.15 s linear).
    hover_t: f64,
    /// Hover grow progress, 0..1 (0.15 s linear).
    grow_t: f64,
    look: Look,
    key: Vec<i64>,
    mask: RefCell<Option<AlphaMask>>,
}

impl Item {
    fn new(p: Placed) -> Self {
        let mut rng = Lcg64::new(mix_seed(p.id));
        let phase = rng.range(0.0, 2.0);
        let sway_dur = 11.0 + rng.range(0.0, 6.0);
        let sway_off = rng.unit();
        let look = Look { zoom: p.zoom, grow: 0.0, pop: 1.0, sway: 0.0, parts: Vec::new(), chrome: 0.0 };
        Self {
            p,
            rev: 0,
            phase,
            sway_dur,
            sway_off,
            born: None,
            dying: None,
            hover_t: 0.0,
            grow_t: 0.0,
            look,
            key: Vec::new(),
            mask: RefCell::new(None),
        }
    }

    /// Recomputes the look for time `now` and bumps `rev` when it changed.
    fn update(&mut self, now: f64) {
        let angle = self.p.kind.sway_angle();
        let sway = if angle > 0.0 {
            let q = (now * SWAY_FPS).floor() / SWAY_FPS;
            let u = ((q + self.sway_off * self.sway_dur) / self.sway_dur).rem_euclid(1.0);
            motion::sway(angle, u)
        } else {
            0.0
        };
        let pop = match (self.dying, self.born) {
            (Some(e), _) => motion::pop_out(e),
            (None, Some(t)) => motion::pop_in(t),
            (None, None) => 1.0,
        };
        let parts: Vec<_> = art::parts(self.p.kind)
            .iter()
            .map(|part| motion::part_state(part.motion, part.value, now, self.phase))
            .collect();
        let strand = self.p.strand.unwrap_or(Strand::LIGHTS);
        let mut key = vec![
            (self.hover_t * 100.0).round() as i64,
            (self.grow_t * 100.0).round() as i64,
            (self.p.zoom * 1000.0).round() as i64,
            (strand.length * 100.0).round() as i64,
            (strand.dip * 100.0).round() as i64,
            (sway * 20000.0).round() as i64,
            (pop * 200.0).round() as i64,
        ];
        for s in &parts {
            key.extend([s.opacity, s.sx, s.sy, s.rot, s.ty].map(|v| (v * 50.0).round() as i64));
        }
        if key != self.key {
            self.key = key;
            self.rev += 1;
        }
        self.look =
            Look { zoom: self.p.zoom, grow: self.grow_t, pop, sway: sway as f32, parts, chrome: self.hover_t as f32 };
    }

    fn live(&self) -> bool {
        self.dying.is_none()
    }
}

#[derive(Clone, Copy, Debug)]
enum Gesture {
    Move { id: u64, start: Vec2, from: Vec2, moved: bool },
    Resize { id: u64, start: Vec2, zoom0: f64, left: f64, top: f64 },
    Stretch { id: u64, grip: usize, start: Vec2, s0: Strand, last: Strand, left: f64, right: f64, top: f64 },
}

impl Gesture {
    fn id(&self) -> u64 {
        match *self {
            Gesture::Move { id, .. } | Gesture::Resize { id, .. } | Gesture::Stretch { id, .. } => id,
        }
    }
}

/// All placed decorations, in z-order (last = topmost).
#[derive(Debug, Default)]
pub struct Decorations {
    max: u32,
    next_id: u64,
    items: Vec<Item>,
    now: f64,
    was_down: bool,
    prev_right: bool,
    menu_request: Option<(u64, Vec2)>,
    hover: Option<u64>,
    armed: Option<u64>,
    gesture: Option<Gesture>,
    screens: Vec<Frame>,
}

impl Decorations {
    /// `max` is `Settings::max_placed`.
    pub fn new(max: u32) -> Self {
        Self { max, next_id: 1, ..Self::default() }
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    /// Change the cap (`Settings::max_placed`). Items above a lowered cap
    /// stay; only new adds are refused.
    pub fn set_max(&mut self, max: u32) {
        self.max = max;
    }

    /// Live decorations (a removed one still fading out is not counted).
    pub fn len(&self) -> usize {
        self.items.iter().filter(|i| i.live()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_full(&self) -> bool {
        self.len() >= self.max as usize
    }

    /// Live decorations in z-order, bottom first.
    pub fn items(&self) -> impl Iterator<Item = &Placed> {
        self.items.iter().filter(|i| i.live()).map(|i| &i.p)
    }

    pub fn get(&self, id: u64) -> Option<&Placed> {
        self.live_item(id).map(|i| &i.p)
    }

    /// Decoration under the cursor, if any.
    pub fn hovered(&self) -> Option<u64> {
        self.hover
    }

    /// True while a drag, resize, stretch or button press is running.
    pub fn busy(&self) -> bool {
        self.gesture.is_some() || self.armed.is_some()
    }

    /// Monitor frames in global points (decorations 3.10 `OnScreen`).
    /// Every decoration is re-clamped; returns true when any moved.
    pub fn set_screens(&mut self, screens: Vec<Frame>) -> bool {
        self.screens = screens;
        let mut moved = false;
        for it in self.items.iter_mut().filter(|i| i.live()) {
            let c = on_screen(it.p.center, &self.screens);
            moved |= c != it.p.center;
            it.p.center = c;
        }
        moved
    }

    fn live_item(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.p.id == id && i.live())
    }

    fn index(&self, id: u64) -> Option<usize> {
        self.items.iter().position(|i| i.p.id == id && i.live())
    }

    /// Adds a decoration centred on `spot` (user add: pops in). `None` at the cap.
    pub fn add(&mut self, kind: Kind, spot: Vec2) -> Option<u64> {
        if self.is_full() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut it = Item::new(Placed { id, kind, center: spot, zoom: 1.0, strand: None });
        it.born = Some(0.0);
        it.update(self.now);
        self.items.push(it);
        Some(id)
    }

    /// Initial spot (decorations 3.3): up to 40 random candidates in the work
    /// area (`work`, global points), cost `4 * overlap with obstacles + 1 *
    /// overlap with existing decorations`, first lowest wins, stops at 0.
    pub fn spot(&self, kind: Kind, work: Frame, obstacles: &[Frame], rng: &mut impl Rng) -> Vec2 {
        let (bw, bh) = render::box_size(kind, None);
        let xmin = work.min_x() + bw / 2.0 + 16.0;
        let xmax = xmin.max(work.max_x() - bw / 2.0 - 16.0);
        let top = bh / 2.0 + 4.0;
        let (lo, hi) = if kind == Kind::Lights {
            (top, top + 120.0)
        } else {
            (top + 12.0, (top + 12.0).max(work.h - bh / 2.0 - 16.0))
        };
        let existing: Vec<Frame> = self.items().map(Placed::frame).collect();
        let mut best: Option<Vec2> = None;
        let mut best_cost = f64::INFINITY;
        for _ in 0..40 {
            let c = Vec2::new(rng.range(xmin, xmax), work.max_y() - rng.range(lo, hi));
            let r = Frame::new(c.x - bw / 2.0, c.y - bh / 2.0, bw, bh);
            let cost = 4.0 * obstacles.iter().map(|o| overlap(&r, o)).sum::<f64>()
                + existing.iter().map(|o| overlap(&r, o)).sum::<f64>();
            if cost < best_cost {
                best = Some(c);
                best_cost = cost;
                if cost <= 0.0 {
                    break;
                }
            }
        }
        best.unwrap_or(work.center())
    }

    /// Takes one down (button or menu): fades out, then disappears.
    pub fn remove(&mut self, id: u64) {
        if let Some(i) = self.index(id) {
            self.items[i].dying = Some(0.0);
            self.items[i].born = None;
        }
        self.clear_state(id);
    }

    pub fn remove_all(&mut self) {
        for it in self.items.iter_mut().filter(|i| i.live()) {
            it.dying = Some(0.0);
            it.born = None;
        }
        self.hover = None;
        self.armed = None;
        self.gesture = None;
    }

    fn clear_state(&mut self, id: u64) {
        if self.hover == Some(id) {
            self.hover = None;
        }
        if self.armed == Some(id) {
            self.armed = None;
        }
        if self.gesture.is_some_and(|g| g.id() == id) {
            self.gesture = None;
        }
    }

    /// Drag/resize/stretch interaction and sway (3.6, 3.6a). Returns true when the list must be saved.
    /// A non-finite `now` keeps the previous clock; a non-finite or negative
    /// `dt` counts as 0.
    pub fn step(&mut self, dt: f64, now: f64, pointer: &PointerInput) -> bool {
        let dt = if dt > 0.0 && dt.is_finite() { dt } else { 0.0 };
        if now.is_finite() {
            self.now = now;
        }
        for it in &mut self.items {
            if let Some(t) = it.born {
                it.born = (t + dt < POP_IN_SECS).then_some(t + dt);
            }
            if let Some(e) = it.dying.as_mut() {
                *e += dt;
            }
        }
        self.items.retain(|i| i.dying.is_none_or(|e| e < POP_OUT_SECS));
        let down = pointer.left_down;
        let right_up = self.prev_right && !pointer.right_down;
        self.prev_right = pointer.right_down;
        let (pressed, released) = (down && !self.was_down, !down && self.was_down);
        self.was_down = down;
        if !self.busy() {
            self.track(pointer);
        }
        if right_up && !self.busy() && !down {
            let hovered = self.hover.filter(|&h| self.live_item(h).is_some());
            if let (Some(id), Some(pos)) = (hovered, pointer.pos) {
                self.menu_request = Some((id, pos));
            }
        }
        let mut save = false;
        if pressed {
            self.press(pointer);
        } else if down {
            self.drag(pointer);
        } else if released {
            save = self.release(pointer);
        }
        let (hover, gesture, now) = (self.hover, self.gesture, self.now);
        let fade = dt / HOVER_SECS;
        let toward = |v: f64, on: bool| if on { (v + fade).min(1.0) } else { (v - fade).max(0.0) };
        for it in &mut self.items {
            let h = hover == Some(it.p.id);
            let resizing = matches!(gesture, Some(Gesture::Resize { id, .. }) if id == it.p.id);
            it.hover_t = toward(it.hover_t, h);
            // a resize re-places the rig at plain zoom, cancelling the grow
            it.grow_t = if resizing { 0.0 } else { toward(it.grow_t, h && it.p.kind != Kind::Lights) };
            it.update(now);
        }
        save
    }

    /// The decoration whose context menu was requested by a right-button
    /// release (3.6), with the cursor position. Taken once.
    pub fn take_menu_request(&mut self) -> Option<(u64, Vec2)> {
        self.menu_request.take()
    }

    /// Keeps the hovered item while the cursor stays in its frame, else
    /// takes the host's alpha-tested pick (3.5 Track).
    fn track(&mut self, pointer: &PointerInput) {
        let keep =
            self.hover.and_then(|h| self.live_item(h)).zip(pointer.pos).is_some_and(|(it, p)| it.p.frame().contains(p));
        if keep {
            return;
        }
        self.hover = pointer
            .hot
            .filter(|k| k.owner_byte() == Owner::Decoration as u8)
            .map(SpriteKey::id)
            .filter(|&id| self.live_item(id).is_some());
    }

    fn press(&mut self, pointer: &PointerInput) {
        let (Some(pos), Some(id)) = (pointer.pos, self.hover) else { return };
        let Some(i) = self.index(id) else { return };
        let p = self.items[i].p.clone();
        let f = p.frame();
        if !f.contains(pos) {
            return;
        }
        let (lx, ly) = (pos.x - f.x, pos.y - f.y);
        let (bx, by) = render::button_center(p.kind, p.strand, p.zoom);
        if (lx - bx).abs() <= 13.0 && (ly - by).abs() <= 13.0 {
            self.armed = Some(id);
            return;
        }
        let it = self.items.remove(i);
        self.items.push(it);
        let hit = render::grips(p.kind, p.strand, p.zoom)
            .into_iter()
            .find(|&(_, gx, gy)| (lx - gx).abs() < 14.0 && (ly - gy).abs() < 16.0);
        self.gesture = Some(match hit {
            Some((3, ..)) => Gesture::Resize { id, start: pos, zoom0: p.zoom, left: f.x, top: f.max_y() },
            Some((grip, ..)) => {
                let s0 = p.strand.unwrap_or(Strand::LIGHTS);
                Gesture::Stretch { id, grip, start: pos, s0, last: s0, left: f.x, right: f.max_x(), top: f.max_y() }
            }
            None => Gesture::Move { id, start: pos, from: p.center, moved: false },
        });
    }

    fn drag(&mut self, pointer: &PointerInput) {
        let (Some(pos), Some(g)) = (pointer.pos, self.gesture) else { return };
        let Some(i) = self.index(g.id()) else { return };
        let it = &mut self.items[i];
        match g {
            Gesture::Move { id, start, from, moved } => {
                let d = pos - start;
                if moved || d.len() > 2.0 {
                    it.p.center = from + d;
                    self.gesture = Some(Gesture::Move { id, start, from, moved: true });
                }
            }
            Gesture::Resize { start, zoom0, left, top, .. } => {
                let (bw, bh) = it.p.box_size();
                let (dx, dy) = (pos.x - start.x, pos.y - start.y);
                let z = (zoom0 * (1.0 + (dx - dy) / (bw + bh))).clamp(ZOOM_MIN, ZOOM_MAX);
                if (z - it.p.zoom).abs() > 0.004 {
                    it.p.zoom = z;
                    let (w, h) = it.p.outer();
                    it.p.center = Vec2::new(left + w / 2.0, top - h / 2.0);
                }
            }
            Gesture::Stretch { id, grip, start, s0, last, left, right, top } => {
                let (dx, dy) = (pos.x - start.x, pos.y - start.y);
                let mut next = s0;
                match grip {
                    0 => next.length -= dx,
                    1 => next.length += dx,
                    _ => next.dip -= dy,
                }
                let next = Strand {
                    length: next.length.clamp(110.0, 4000.0).round_ties_even(),
                    dip: next.dip.clamp(0.0, 260.0).round_ties_even(),
                };
                let change = (next.length - last.length).abs().max((next.dip - last.dip).abs());
                if change >= 2.0 || (change > 0.0 && (next.dip == 0.0 || next.length == 110.0)) {
                    it.p.strand = Some(next);
                    *it.mask.get_mut() = None;
                    let (w, h) = it.p.outer();
                    let x = if grip == 0 { right - w } else { left };
                    it.p.center = Vec2::new(x + w / 2.0, top - h / 2.0);
                    self.gesture = Some(Gesture::Stretch { id, grip, start, s0, last: next, left, right, top });
                }
            }
        }
    }

    fn release(&mut self, pointer: &PointerInput) -> bool {
        if let Some(id) = self.armed.take() {
            let inside = self.live_item(id).zip(pointer.pos).is_some_and(|(it, pos)| {
                let f = it.p.frame();
                let (bx, by) = render::button_center(it.p.kind, it.p.strand, it.p.zoom);
                (pos.x - f.x - bx).abs() <= 13.0 && (pos.y - f.y - by).abs() <= 13.0
            });
            if inside {
                self.remove(id);
            }
            return inside;
        }
        let Some(g) = self.gesture.take() else { return false };
        let save = match g {
            Gesture::Move { id, moved: true, .. } => {
                let screens = self.screens.clone();
                for it in self.items.iter_mut().filter(|i| i.p.id == id) {
                    it.p.center = on_screen(it.p.center, &screens);
                }
                true
            }
            Gesture::Move { .. } => false,
            Gesture::Resize { id, zoom0, .. } => self.get(id).is_some_and(|p| p.zoom != zoom0),
            Gesture::Stretch { id, s0, .. } => self.get(id).is_some_and(|p| p.strand != Some(s0)),
        };
        self.track(pointer);
        save
    }

    /// Sprites for this frame in z-order (bottom first), including items
    /// still fading out.
    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        for it in &self.items {
            let id = it.p.id;
            let active = self.hover == Some(id) || self.gesture.is_some_and(|g| g.id() == id);
            let (hit, opacity) = match it.dying {
                Some(e) => (HitMode::Never, (1.0 - e / POP_OUT_SECS).clamp(0.0, 1.0) as f32),
                None if active => (HitMode::Always, 1.0),
                None => (HitMode::Alpha, 1.0),
            };
            out.push(Sprite {
                key: SpriteKey::new(Owner::Decoration, id),
                frame: it.p.frame(),
                z: ZBand::Desktop,
                hit,
                rev: it.rev,
                opacity,
            });
        }
    }

    /// Window pixmap (`ceil(frame * scale)` pixels) of the current frame.
    pub fn paint(&self, id: u64, scale: f32) -> Option<fp_paint::tiny_skia::Pixmap> {
        let it = self.items.iter().find(|i| i.p.id == id)?;
        Some(render::paint(it.p.kind, it.p.strand, &it.look, scale))
    }

    /// Alpha mask hit test in sprite-local points: `(x, y)` is measured from
    /// the frame's bottom-left corner, Y up (3.5 `Solid`).
    pub fn solid(&self, id: u64, x: f64, y: f64) -> bool {
        let Some(it) = self.live_item(id) else { return false };
        let (bw, bh) = it.p.box_size();
        let (ow, oh) = it.p.outer();
        let z = it.p.zoom;
        let bx = (x - (ow - bw * z) / 2.0) / z;
        let by = (y - (oh - bh * z) / 2.0) / z;
        let mut cache = it.mask.borrow_mut();
        let mask = cache.get_or_insert_with(|| render::mask(it.p.kind, it.p.strand));
        mask.solid(bx, f64::from(mask.h) - by)
    }

    /// `decorations` array (decorations 8), z-order bottom first.
    pub fn save_json(&self) -> Value {
        let round = |v: f64, k: f64| (v * k).round() / k;
        let num = |v: f64| if v.fract() == 0.0 && v.abs() < 1e15 { Value::from(v as i64) } else { Value::from(v) };
        Value::Array(
            self.items()
                .map(|p| {
                    let mut m = Map::new();
                    m.insert("kind".into(), p.kind.raw().into());
                    m.insert("x".into(), num(round(p.center.x, 100.0)));
                    m.insert("y".into(), num(round(p.center.y, 100.0)));
                    if let (Kind::Lights, Some(s)) = (p.kind, p.strand) {
                        m.insert("length".into(), num(s.length));
                        m.insert("dip".into(), num(s.dip));
                    }
                    if p.zoom != 1.0 {
                        m.insert("size".into(), num(round(p.zoom, 10000.0)));
                    }
                    Value::Object(m)
                })
                .collect(),
        )
    }

    /// Replaces the list from a saved array: first `max` entries before
    /// filtering, unknown kinds skipped, no pop-in.
    pub fn load_json(&mut self, v: &Value) {
        self.items.clear();
        self.hover = None;
        self.armed = None;
        self.gesture = None;
        let Some(arr) = v.as_array() else { return };
        for e in arr.iter().take(self.max as usize) {
            let Some(kind) = e.get("kind").and_then(Value::as_str).and_then(Kind::parse) else { continue };
            let num = |k: &str| e.get(k).and_then(Value::as_f64);
            let (Some(x), Some(y)) = (num("x"), num("y")) else { continue };
            let lights = kind == Kind::Lights;
            let strand = num("length")
                .filter(|_| lights)
                .map(|length| Strand { length, dip: num("dip").unwrap_or(18.0) }.fitted());
            let zoom = if lights { 1.0 } else { num("size").unwrap_or(1.0).clamp(ZOOM_MIN, ZOOM_MAX) };
            let id = self.next_id;
            self.next_id += 1;
            let center = on_screen(Vec2::new(x, y), &self.screens);
            let mut it = Item::new(Placed { id, kind, center, zoom, strand });
            it.update(self.now);
            self.items.push(it);
        }
    }
}

/// Picker tile icon: the full picture at `scale`, no shadow, no padding.
pub fn icon(kind: Kind, scale: f32) -> fp_paint::tiny_skia::Pixmap {
    render::icon(kind, scale)
}

/// Icon fitted uniformly and centred into a `side` x `side` square (tray icon: red bauble).
pub fn icon_square(kind: Kind, side: u32) -> fp_paint::tiny_skia::Pixmap {
    render::icon_square(kind, side)
}

/// Remove button centre in window points (Y up, from the frame's bottom-left).
pub fn button_center(p: &Placed) -> Vec2 {
    let (x, y) = render::button_center(p.kind, p.strand, p.zoom);
    Vec2::new(x, y)
}

/// Active grips as `(index, window point)`: lights 0/1/2 (left, right,
/// middle), everything else 3 (corner).
pub fn grips(p: &Placed) -> Vec<(usize, Vec2)> {
    render::grips(p.kind, p.strand, p.zoom).into_iter().map(|(i, x, y)| (i, Vec2::new(x, y))).collect()
}

#[cfg(test)]
mod tests;
