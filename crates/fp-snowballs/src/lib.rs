//! Snowballs: gathered from snow, rolling on window tops, dragged, thrown,
//! stacked into snowmen, shattered back into snow, persisted.
//!
//! Spec: snowballs 2 (world space), 3.2 (Absorb, seed, Grow, Spawn; the
//! feed side is `fp_core::feed`), 3.3 (ball state, supports, stacks),
//! 3.4 (ledges from screen snapshots), 3.5 (physics), 3.6 (grab, drag,
//! throw, stack), 3.7 (window size/visibility as [`Sprite`] data; painting
//! via `fp_snowman::paint_ball`), 3.8 (stacking/snowmen), 3.9 (shatter,
//! splats -> bursts), 3.10 (`standingSnowmen` save/restore), 3.11 gating
//! (none); snowman F1, F6.
//! Lifecycle (3.1) and step ordering (4) live in `fp-world`.
//!
//! Deviations, all small: the plough blade is handed over per frame with
//! [`Balls::shove`] and applied inside the next [`Balls::step`]; a carried
//! snowman cannot be grabbed; sprites are sized from the last drawn radius so
//! the art never has to be stretched.

mod ledges;
mod saved;

pub use ledges::{GROUND_HEIGHT, Ledge, Ledges, Screen};

use fp_core::feed::{Burst, Gathered, Target};
use fp_core::geom::{Frame, Vec2};
use fp_core::scene::{HitMode, Owner, PointerInput, Sprite, SpriteKey, ZBand};
use fp_core::surface::{LedgeKey, SurfaceId};
use fp_snowman::{Decor, Look, paint_ball, panel_size};
use saved::{MAX_STACK, Place, Saved};
use serde_json::Value;
use std::f64::consts::PI;

pub const MAX_BALLS: usize = 30;
pub const MIN_RADIUS: f64 = 6.0;
pub const MAX_RADIUS: f64 = 64.0;

/// Longest step the physics takes (3.1 max dt).
pub const MAX_DT: f64 = 0.05;

const GRAVITY: f64 = 2500.0;
const FRICTION: f64 = 4.0;
const STACK_SPACING: f64 = 0.82;
const BREAK_ENERGY: f64 = 400.0;
const THROW_MAX: f64 = 1500.0;
const THROWN_AT: f64 = 500.0;
const THROWN_HIT: f64 = 300.0;
const MIN_SHOVE: f64 = 120.0;
const HOP_TRIGGER: f64 = 3500.0;
const HOP_MAX: f64 = 650.0;
const HOP_TAU: f64 = 0.06;
const REACH: f64 = 14.0;
const SEED_AREA: f64 = 90.0;
const SEED_WINDOW: f64 = 0.5;
const SEED_NEAR: f64 = 60.0;
const GROW: f64 = 1.5;
const REMEMBER_EVERY: f64 = 3.0;
const BRING_EVERY: f64 = 0.25;
const DOCK_PATIENCE: f64 = 6.0;
const GIVE_UP: f64 = 180.0;
const PRESS_MOVE: f64 = 4.0;
const REDRAW_RADIUS: f64 = 0.5;
const REDRAW_ANGLE: f64 = 0.15;
const CARRY_DECAY: f64 = 0.9;
const CARRY_SNAP: f64 = 0.2;

/// Events the world must route elsewhere.
#[derive(Clone, Debug, PartialEq)]
pub enum BallEvent {
    /// Snow back into the simulation on the screen with this origin.
    Burst { origin: Vec2, burst: Burst },
    /// The user clicked a snowman: open the dress-up card for this stack.
    Dress { bottom: u64 },
    /// Standing stacks changed; persist when convenient.
    Dirty,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Support {
    Air,
    Ledge(LedgeKey),
    Ball(u64),
}

/// What the sprite art was last drawn with.
#[derive(Clone, Copy, Debug)]
struct Drawn {
    r: f64,
    angle: f64,
    decor: Decor,
    look: Look,
}

#[derive(Clone, Debug)]
struct Ball {
    id: u64,
    seed: u64,
    c: Vec2,
    r: f64,
    v: Vec2,
    angle: f64,
    support: Support,
    moved: Vec2,
    riding: Option<LedgeKey>,
    ride: Vec2,
    air: Option<f64>,
    rise: f64,
    hidden: bool,
    gone: bool,
    decor: Decor,
    look: Option<Look>,
    decor_look: Look,
    carried: bool,
    thrown: bool,
    /// Offset from the stack bottom while carried.
    offset: Vec2,
    /// Decaying visual shift while carried.
    shift: Vec2,
    drawn: Drawn,
    rev: u64,
}

impl Ball {
    fn new(id: u64, c: Vec2, r: f64, support: Support) -> Ball {
        Ball {
            id,
            seed: id.wrapping_mul(2654435761).wrapping_add(97),
            c,
            r,
            v: Vec2::ZERO,
            angle: 0.0,
            support,
            moved: Vec2::ZERO,
            riding: None,
            ride: Vec2::ZERO,
            air: None,
            rise: 0.0,
            hidden: false,
            gone: false,
            decor: Decor::Plain,
            look: None,
            decor_look: Look::classic(),
            carried: false,
            thrown: false,
            offset: Vec2::ZERO,
            shift: Vec2::ZERO,
            drawn: Drawn { r, angle: 0.0, decor: Decor::Plain, look: Look::classic() },
            rev: 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Seed {
    area: f64,
    point: Vec2,
    time: f64,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    id: u64,
    grab: Vec2,
}

#[derive(Clone, Copy, Debug)]
struct Plough {
    origin: Vec2,
    front: f64,
    way: f64,
    scale: f64,
    dock: bool,
}

/// All balls.
#[derive(Debug)]
pub struct Balls {
    list: Vec<Ball>,
    next_id: u64,
    seed: Option<Seed>,
    drag: Option<Drag>,
    pressed: Option<(u64, Vec2)>,
    was_down: bool,
    prev_pos: Option<Vec2>,
    pointer_v: Vec2,
    plough: Option<Plough>,
    waiting: Vec<Saved>,
    waiting_since: f64,
    last_bring: f64,
    snap: Vec<Ledge>,
    screens: Vec<Screen>,
    snap_gen: u64,
    last_save: Option<f64>,
    force_save: bool,
    last_json: Value,
}

impl Default for Balls {
    fn default() -> Self {
        Balls {
            list: Vec::new(),
            next_id: 0,
            seed: None,
            drag: None,
            pressed: None,
            was_down: false,
            prev_pos: None,
            pointer_v: Vec2::ZERO,
            plough: None,
            waiting: Vec::new(),
            waiting_since: 0.0,
            last_bring: f64::NEG_INFINITY,
            snap: Vec::new(),
            screens: Vec::new(),
            snap_gen: 0,
            last_save: None,
            force_save: false,
            last_json: Value::Array(Vec::new()),
        }
    }
}

fn grown(r: f64, area: f64) -> f64 {
    (r * r + area * GROW / PI).sqrt().min(MAX_RADIUS)
}

fn sign(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

impl Balls {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- lookups ----

    fn idx(&self, id: u64) -> Option<usize> {
        self.list.iter().position(|b| b.id == id && !b.gone)
    }

    fn bottom_of(&self, mut i: usize) -> usize {
        for _ in 0..MAX_STACK {
            let Support::Ball(base) = self.list[i].support else { break };
            match self.idx(base) {
                Some(k) => i = k,
                None => break,
            }
        }
        i
    }

    /// The stack `i` belongs to, bottom first.
    fn stack(&self, i: usize) -> Vec<usize> {
        let mut cur = self.bottom_of(i);
        let mut out = vec![cur];
        for _ in 0..MAX_STACK {
            let id = self.list[cur].id;
            let above = self.list.iter().position(|b| !b.gone && b.support == Support::Ball(id));
            match above {
                Some(k) => {
                    out.push(k);
                    cur = k;
                }
                None => break,
            }
        }
        out
    }

    /// `true` when ball `j` is transitively supported by ball `base`.
    fn rests(&self, j: usize, base: u64) -> bool {
        let mut cur = j;
        for _ in 0..MAX_STACK {
            let Support::Ball(id) = self.list[cur].support else { return false };
            if id == base {
                return true;
            }
            match self.idx(id) {
                Some(k) => cur = k,
                None => return false,
            }
        }
        false
    }

    fn depth(&self, i: usize) -> usize {
        let mut cur = i;
        let mut depth = 0;
        while depth < MAX_STACK {
            let Support::Ball(id) = self.list[cur].support else { break };
            match self.idx(id) {
                Some(k) => cur = k,
                None => break,
            }
            depth += 1;
        }
        depth
    }

    fn sync(&mut self, ledges: &Ledges) {
        if ledges.generation() != self.snap_gen {
            self.snap_gen = ledges.generation();
            self.snap = ledges.all().to_vec();
            self.screens = ledges.screens().to_vec();
        }
    }

    // ---- gathering (3.2) ----

    /// Absorb gathered snow (3.2).
    pub fn absorb(&mut self, gathered: &[Gathered], ledges: &Ledges, now: f64, dragging: bool) {
        self.sync(ledges);
        for g in gathered {
            let sane = g.area.is_finite() && g.area > 0.0 && g.point.x.is_finite() && g.point.y.is_finite();
            if !sane {
                continue;
            }
            if let Some(i) = g.ball.and_then(|id| self.idx(id)) {
                self.list[i].r = grown(self.list[i].r, g.area);
                continue;
            }
            let merged = match self.seed {
                Some(s) if now - s.time < SEED_WINDOW && s.point.dist(g.point) < SEED_NEAR => {
                    Seed { area: s.area + g.area, point: g.point, time: now }
                }
                _ => Seed { area: g.area, point: g.point, time: now },
            };
            self.seed = Some(merged);
            if merged.area >= SEED_AREA && self.count() < MAX_BALLS && !dragging && self.drag.is_none() {
                self.seed = None;
                let near = self.list.iter().position(|b| !b.gone && b.c.dist(merged.point) < b.r + REACH);
                match near {
                    Some(i) => self.list[i].r = grown(self.list[i].r, merged.area),
                    None => self.spawn(merged.point, merged.area, ledges),
                }
            }
        }
    }

    fn spawn(&mut self, point: Vec2, area: f64, ledges: &Ledges) {
        let r = (area * GROW / PI).sqrt().clamp(MIN_RADIUS, MAX_RADIUS);
        let landing = ledges
            .all()
            .iter()
            .filter(|l| l.shown && l.spans(point.x) && l.top >= point.y - 30.0 && l.top <= point.y + 20.0)
            .filter(|l| !l.is_blocked(point.x))
            .max_by(|a, b| a.top.total_cmp(&b.top));
        let (c, support) = match landing {
            Some(l) => (Vec2::new(point.x, l.top - 1.0 + r), Support::Ledge(l.key)),
            None => (Vec2::new(point.x, point.y + r), Support::Air),
        };
        let id = self.next_id;
        self.next_id += 1;
        self.list.push(Ball::new(id, c, r, support));
    }

    /// Loose balls for the feed (reach = r + 14).
    pub fn targets(&self) -> Vec<Target> {
        let loose =
            |b: &&Ball| !b.gone && !b.carried && matches!(b.support, Support::Ledge(_)) && !self.has_rider(b.id);
        self.list.iter().filter(loose).map(|b| Target { ball: b.id, center: b.c, reach: b.r + REACH }).collect()
    }

    // ---- outside actors ----

    /// Plough shove (3.5C): push balls in front of the blade (ground ledges
    /// of the screen at `screen_origin`; `front` is screen-local). Applied by
    /// the next [`Balls::step`].
    pub fn shove(&mut self, front: f64, way: f64, screen_origin: Vec2) {
        self.shove_run(front, way, screen_origin, 1.0, false);
    }

    /// [`Balls::shove`] for a plough of size `scale`, optionally on the dock.
    pub fn shove_run(&mut self, front: f64, way: f64, screen_origin: Vec2, scale: f64, dock: bool) {
        self.plough = Some(Plough { origin: screen_origin, front: front + screen_origin.x, way, scale, dock });
    }

    /// Yeti prey: bottoms of stacks with 3+ balls, as `(bottom, parts)`
    /// where `parts` are the real `(centre, radius)` pairs, bottom to top.
    pub fn prey(&self) -> Vec<(u64, Vec<(Vec2, f64)>)> {
        let mut out = Vec::new();
        for (i, b) in self.list.iter().enumerate() {
            let free = !b.gone && !b.hidden && !b.carried && !matches!(b.support, Support::Ball(_));
            let stack = self.stack(i);
            if free && stack.len() >= 3 {
                out.push((b.id, stack.iter().map(|&k| (self.list[k].c, self.list[k].r)).collect()));
            }
        }
        out
    }

    /// Smash a stack (yeti hit).
    pub fn smash(&mut self, bottom: u64, out: &mut Vec<BallEvent>) {
        if let Some(i) = self.idx(bottom) {
            self.smash_stack(i, out);
        }
    }

    /// Current look of the snowman whose stack holds `bottom` (`None` =
    /// classic, or no such ball). Restored snowmen keep their saved look.
    pub fn look(&self, bottom: u64) -> Option<Look> {
        let i = self.bottom_of(self.idx(bottom)?);
        self.list[i].look
    }

    /// `true` while `bottom` is still the bottom of a standing snowman the
    /// dress-up card can style (snowballs 3.6 KeepDressCard): present, not
    /// carried, not resting on another ball, with at least three balls.
    pub fn dressable(&self, bottom: u64) -> bool {
        let Some(i) = self.idx(bottom) else { return false };
        let b = &self.list[i];
        !b.carried && !matches!(b.support, Support::Ball(_)) && self.stack(i).len() >= 3
    }

    /// Restyle a snowman (dress-up card result).
    pub fn restyle(&mut self, bottom: u64, look: Option<Look>) {
        if let Some(i) = self.idx(bottom) {
            let i = self.bottom_of(i);
            self.list[i].look = look.and_then(Look::stored);
            self.force_save = true;
        }
    }

    /// Shatter everything (setting off), with bursts for visible balls.
    pub fn break_all(&mut self, out: &mut Vec<BallEvent>) {
        for i in 0..self.list.len() {
            self.shatter(i, out);
        }
        self.clear();
    }

    /// Close everything without bursts (weather changed).
    pub fn clear(&mut self) {
        self.list.clear();
        self.waiting.clear();
        self.seed = None;
        self.drag = None;
        self.pressed = None;
        self.plough = None;
        self.force_save = false;
        self.last_json = Value::Array(Vec::new());
        self.last_save = None;
        self.snap.clear();
        self.screens.clear();
        self.snap_gen = 0;
        self.prev_pos = None;
        self.pointer_v = Vec2::ZERO;
        self.was_down = false;
    }

    pub fn count(&self) -> usize {
        self.list.iter().filter(|b| !b.gone).count()
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    // ---- shatter ----

    fn shatter(&mut self, i: usize, out: &mut Vec<BallEvent>) {
        let b = &mut self.list[i];
        if b.gone {
            return;
        }
        b.gone = true;
        let (id, c, r, v, hidden) = (b.id, b.c, b.r, b.v, b.hidden);
        if self.drag.is_some_and(|d| d.id == id) {
            self.drag = None;
        }
        let screen = self.screens.iter().find(|s| s.frame.contains(c));
        if let (false, Some(screen)) = (hidden, screen) {
            let origin = screen.frame.min();
            let burst = Burst { point: c - origin, area: PI * r * r / GROW, velocity: v };
            out.push(BallEvent::Burst { origin, burst });
        }
    }

    fn smash_stack(&mut self, i: usize, out: &mut Vec<BallEvent>) {
        for k in self.stack(i) {
            self.shatter(k, out);
        }
    }

    /// `Breaks` (3.5E): clears `thrown`, smashes the stack on a hard impact.
    fn breaks(&mut self, i: usize, speed: f64, out: &mut Vec<BallEvent>) -> bool {
        self.list[i].thrown = false;
        let hard = speed * speed / 5000.0 >= BREAK_ENERGY;
        if hard {
            self.smash_stack(i, out);
        }
        hard
    }

    // ---- the step ----

    /// One physics step (3.5, 3.6, 3.8, 3.9).
    pub fn step(&mut self, dt: f64, now: f64, ledges: &Ledges, pointer: &PointerInput, out: &mut Vec<BallEvent>) {
        // 3.1: a step without time (or a NaN one) does nothing.
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        let dt = dt.min(MAX_DT);
        self.sync(ledges);
        self.bring_back(now);
        self.read_pointer(dt, pointer);
        self.buttons(pointer, out);
        let riders: Vec<u64> = self
            .list
            .iter()
            .filter(|b| !b.gone)
            .filter_map(|b| if let Support::Ball(base) = b.support { Some(base) } else { None })
            .collect();
        let mut order: Vec<usize> = (0..self.list.len()).collect();
        order.sort_by_key(|&i| self.depth(i));
        for i in order {
            if self.list[i].gone || self.list[i].carried {
                continue;
            }
            let before = self.list[i].c;
            self.advance(i, dt, ledges, pointer, &riders, out);
            self.list[i].moved = self.list[i].c - before;
        }
        self.collide(&riders);
        self.dress();
        self.list.retain(|b| !b.gone);
        self.refresh();
        self.remember(now, out);
        self.plough = None;
    }

    fn read_pointer(&mut self, dt: f64, pointer: &PointerInput) {
        self.pointer_v = match (pointer.pos, self.prev_pos) {
            (Some(pos), Some(prev)) => self.pointer_v * 0.5 + (pos - prev) * (0.5 / dt),
            _ => Vec2::ZERO,
        };
        self.prev_pos = pointer.pos;
    }

    fn buttons(&mut self, pointer: &PointerInput, out: &mut Vec<BallEvent>) {
        if let (Some(pos), Some((id, at)), None) = (pointer.pos, self.pressed, self.drag)
            && pos.dist(at) >= PRESS_MOVE
        {
            self.pressed = None;
            self.lift(id, pos);
        }
        if !pointer.left_down {
            self.release(out);
        } else if !self.was_down
            && let (Some(pos), Some(hot)) = (pointer.pos, pointer.hot)
            && hot.owner_byte() == Owner::Ball as u8
        {
            self.grab(hot.id(), pos);
        }
        self.was_down = pointer.left_down;
    }

    /// Left button went down over ball `id` (3.6 `Grab`, `Pressed`).
    fn grab(&mut self, id: u64, pos: Vec2) {
        let Some(i) = self.idx(id) else { return };
        if self.drag.is_some() || self.pressed.is_some() || self.list[i].carried {
            return;
        }
        if self.stack(i).len() >= 3 {
            self.pressed = Some((id, pos));
        } else {
            self.lift(id, pos);
        }
    }

    fn lift(&mut self, id: u64, pos: Vec2) {
        let Some(i) = self.idx(id) else { return };
        let b = &mut self.list[i];
        self.drag = Some(Drag { id, grab: b.c - pos });
        b.support = Support::Air;
        b.v = Vec2::ZERO;
        b.thrown = false;
    }

    /// Button released (idempotent): a click on a snowman asks for the card,
    /// a drag ends in a throw or a stack assist.
    fn release(&mut self, out: &mut Vec<BallEvent>) {
        if let Some((id, _)) = self.pressed.take() {
            if let Some(i) = self.idx(id) {
                let bottom = self.list[self.bottom_of(i)].id;
                out.push(BallEvent::Dress { bottom });
            }
            return;
        }
        let Some(drag) = self.drag.take() else { return };
        let Some(i) = self.idx(drag.id) else { return };
        let speed = self.list[i].v.len();
        if speed > THROW_MAX {
            self.list[i].v = self.list[i].v * (THROW_MAX / speed);
        }
        self.list[i].thrown = speed > THROWN_AT;
        if speed > THROWN_AT {
            return;
        }
        let (c, r) = (self.list[i].c, self.list[i].r);
        let base = self
            .list
            .iter()
            .enumerate()
            .filter(|(k, o)| *k != i && !o.gone && o.c.y < c.y && (o.c.x - c.x).abs() < o.r + 0.4 * r)
            .filter(|(k, o)| !self.has_rider(o.id) && !self.rests(*k, drag.id))
            .max_by(|a, b| a.1.c.y.total_cmp(&b.1.c.y))
            .map(|(_, o)| (o.c, o.r));
        if let Some((bc, br)) = base {
            let b = &mut self.list[i];
            b.c = Vec2::new(bc.x, c.y.max(bc.y + (br + r) * STACK_SPACING + 1.0));
            b.v = Vec2::ZERO;
        }
    }

    fn has_rider(&self, id: u64) -> bool {
        self.list.iter().any(|b| !b.gone && b.support == Support::Ball(id))
    }

    fn fall(&mut self, i: usize) {
        let b = &mut self.list[i];
        b.support = Support::Air;
        b.v.y = b.v.y.min(0.0);
    }

    fn advance(
        &mut self,
        i: usize,
        dt: f64,
        ledges: &Ledges,
        pointer: &PointerInput,
        riders: &[u64],
        out: &mut Vec<BallEvent>,
    ) {
        let id = self.list[i].id;
        if let Some(drag) = self.drag.filter(|d| d.id == id) {
            self.advance_dragged(i, dt, drag, pointer);
            return;
        }
        match self.list[i].support {
            Support::Ball(base) => self.on_ball(i, base),
            Support::Ledge(key) => self.on_ledge(i, key, dt, ledges, pointer, riders, out),
            Support::Air => self.drop_ball(i, dt, ledges, riders, out),
        }
    }

    /// 3.5A.
    fn advance_dragged(&mut self, i: usize, dt: f64, drag: Drag, pointer: &PointerInput) {
        let b = &mut self.list[i];
        let before = b.c;
        if let Some(pos) = pointer.pos {
            b.c = pos + drag.grab;
        }
        b.v = b.v * 0.5 + (b.c - before) * (0.5 / dt);
        b.angle -= (b.c.x - before.x) / b.r;
        b.hidden = false;
    }

    /// 3.5B.
    fn on_ball(&mut self, i: usize, base: u64) {
        let Some(bi) = self.idx(base) else { return self.fall(i) };
        let (bc, br, bv, bh, bm) = {
            let o = &self.list[bi];
            (o.c, o.r, o.v, o.hidden, o.moved)
        };
        let b = &mut self.list[i];
        b.c.x += bm.x;
        b.c.y = bc.y + (br + b.r) * STACK_SPACING;
        b.v = bv;
        b.hidden = bh;
        if (b.c.x - bc.x).abs() > br * 0.9 {
            self.fall(i);
        }
    }

    /// 3.5C.
    #[allow(clippy::too_many_arguments)]
    fn on_ledge(
        &mut self,
        i: usize,
        key: LedgeKey,
        dt: f64,
        ledges: &Ledges,
        pointer: &PointerInput,
        riders: &[u64],
        out: &mut Vec<BallEvent>,
    ) {
        let ledge = match ledges.find(key) {
            Some(l) if !l.jumped => l,
            _ => return self.fall(i),
        };
        if !self.hop(i, ledge, dt, ledges, out) {
            return;
        }
        let nudge = self.nudge(i, pointer, riders);
        let plough = self.plough;
        let b = &mut self.list[i];
        let lvx = ledge.velocity.x;
        b.v.x = match nudge {
            Some(vx) => vx,
            None => lvx + (b.v.x - lvx) * (-FRICTION * dt).exp(),
        };
        b.v.y = if b.air.is_some() { b.rise } else { ledge.velocity.y };
        b.c.x += b.v.x * dt;
        let wanted =
            |p: &Plough| p.origin == key.origin && key.id == if p.dock { SurfaceId::Dock } else { SurfaceId::Ground };
        if let Some(p) = plough.filter(wanted) {
            let d = (b.c.x - p.front) * p.way;
            if d < b.r && d > -112.0 * p.scale {
                b.c.x = p.front + p.way * b.r;
                b.v.x = p.way * MIN_SHOVE;
            }
        }
        b.c.y = b.air.unwrap_or(ledge.top - 1.0) + b.r;
        b.angle -= (b.v.x - lvx) * dt / b.r;
        b.hidden = !ledge.shown || ledge.is_blocked(b.c.x);
        if b.c.x < ledge.min_x || b.c.x > ledge.max_x {
            b.support = Support::Air;
        }
    }

    /// Cursor nudge (3.5C): a slow pointer pushes a free ball sideways.
    fn nudge(&self, i: usize, pointer: &PointerInput, riders: &[u64]) -> Option<f64> {
        let b = &self.list[i];
        let pos = pointer.pos?;
        let slow = self.pointer_v.x.abs() > 30.0 && self.pointer_v.len() < 350.0;
        let near = (pos.x - b.c.x).abs() < b.r + 4.0 && (pos.y - b.c.y).abs() < b.r + 8.0;
        (!riders.contains(&b.id) && !pointer.left_down && slow && near).then_some(self.pointer_v.x)
    }

    /// 3.5D: the window trampoline. `false` means the ball changed support
    /// or broke, so the rest of its step is skipped.
    fn hop(&mut self, i: usize, ledge: &Ledge, dt: f64, ledges: &Ledges, out: &mut Vec<BallEvent>) -> bool {
        let top = ledge.top - 1.0;
        let b = &mut self.list[i];
        if b.riding != Some(ledge.key) {
            b.riding = Some(ledge.key);
            b.ride = ledge.velocity;
            b.air = None;
            return true;
        }
        let prev = b.ride;
        b.ride = b.ride + (ledge.velocity - b.ride) * (1.0 - (-dt / HOP_TAU).exp());
        if b.air.is_none() && -GRAVITY - (b.ride.y - prev.y) / dt > HOP_TRIGGER {
            b.air = Some(top);
            b.rise = prev.y.min(HOP_MAX);
        }
        let Some(prev_air) = b.air else { return true };
        b.rise -= GRAVITY * dt;
        let (rise, x, r) = (b.rise, b.c.x, b.r);
        let air = prev_air + rise * dt;
        let target = ledges
            .all()
            .iter()
            .filter(|o| o.key != ledge.key && o.shown && o.top - 1.0 > top && o.spans(x) && !o.is_blocked(x))
            .filter(|o| prev_air >= o.top - 1.0 - o.velocity.y.max(0.0) * dt - 0.5 && air <= o.top - 1.0)
            .max_by(|a, b| a.top.total_cmp(&b.top));
        if let Some(o) = target {
            if !self.breaks(i, -rise, out) {
                let b = &mut self.list[i];
                b.support = Support::Ledge(o.key);
                b.riding = None;
                b.air = None;
                b.c.y = o.top - 1.0 + r;
                b.v.y = o.velocity.y;
                b.hidden = false;
            }
            return false;
        }
        if air <= top {
            if self.breaks(i, -rise, out) {
                return false;
            }
            self.list[i].air = None;
        } else {
            self.list[i].air = Some(air);
        }
        true
    }

    /// 3.5E: free fall.
    fn drop_ball(&mut self, i: usize, dt: f64, ledges: &Ledges, riders: &[u64], out: &mut Vec<BallEvent>) {
        let id = self.list[i].id;
        let r = self.list[i].r;
        let prev = self.list[i].c;
        {
            let b = &mut self.list[i];
            b.v.y -= GRAVITY * dt;
            b.c = b.c + b.v * dt;
            b.angle -= b.v.x * dt / b.r * 0.3;
            b.hidden = false;
        }
        let (c, v) = (self.list[i].c, self.list[i].v);
        if self.list[i].thrown && v.len() > THROWN_HIT {
            let hit = (0..self.list.len()).find(|&k| {
                let o = &self.list[k];
                k != i
                    && !o.gone
                    && !o.hidden
                    && self.drag.is_none_or(|d| d.id != o.id)
                    && !self.rests(k, id)
                    && c.dist(o.c) < (r + o.r) * 0.9
            });
            if let Some(k) = hit {
                self.smash_stack(k, out);
                self.smash_stack(i, out);
                return;
            }
        }
        for k in 0..self.list.len() {
            // Side pushes move this ball, so later checks read it afresh.
            let (c, v) = (self.list[i].c, self.list[i].v);
            let o = &self.list[k];
            if k == i || o.gone || riders.contains(&o.id) || self.rests(k, id) || o.support == Support::Air {
                continue;
            }
            let (oc, or, oid) = (o.c, o.r, o.id);
            let top = oc.y + (or + r) * STACK_SPACING;
            if (c.x - oc.x).abs() < or * 0.75 && prev.y >= top - 0.5 && c.y <= top {
                if !self.breaks(i, -v.y, out) {
                    let b = &mut self.list[i];
                    b.c = Vec2::new(oc.x, top);
                    b.v = Vec2::ZERO;
                    b.support = Support::Ball(oid);
                }
                return;
            }
            let reach = (r + or) * 0.9;
            if c.dist(oc) < reach && c.y > oc.y {
                let dy = c.y - oc.y;
                let side = sign(c.x - oc.x);
                let b = &mut self.list[i];
                b.c.x = oc.x + side * (reach * reach - dy * dy).sqrt();
                b.v.x = side * v.x.abs().max(MIN_SHOVE);
            }
        }
        let (c, v) = (self.list[i].c, self.list[i].v);
        let (bottom, bottom_prev) = (c.y - r, prev.y - r);
        let landing = ledges
            .all()
            .iter()
            .filter(|l| l.shown && l.spans(c.x) && !l.is_blocked(c.x))
            .filter(|l| bottom_prev >= l.top - 1.0 - l.velocity.y.max(0.0) * dt - 0.5 && bottom <= l.top - 1.0)
            .max_by(|a, b| a.top.total_cmp(&b.top))
            .map(|l| (l.key, l.top - 1.0));
        let floor = || {
            ledges
                .screens()
                .iter()
                .find(|s| {
                    c.x >= s.frame.min_x() && c.x <= s.frame.max_x() && bottom < s.frame.min_y() + GROUND_HEIGHT - 1.0
                })
                .map(|s| (LedgeKey::new(s.frame.min(), SurfaceId::Ground), s.frame.min_y() + GROUND_HEIGHT - 1.0))
        };
        if let Some((key, t)) = landing.or_else(floor) {
            if !self.breaks(i, -v.y, out) {
                let b = &mut self.list[i];
                b.c.y = t + r;
                b.v = Vec2::new(v.x * 0.6, 0.0);
                b.support = Support::Ledge(key);
            }
            return;
        }
        let inside = ledges.screens().iter().any(|s| s.frame.inset(-r, -r).contains(c));
        if !ledges.screens().is_empty() && !inside {
            self.list[i].gone = true;
        }
    }

    /// 3.5F.
    fn collide(&mut self, riders: &[u64]) {
        let n = self.list.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.list[i], &self.list[j]);
                let same = match (a.support, b.support) {
                    (Support::Ledge(x), Support::Ledge(y)) => x == y,
                    _ => false,
                };
                if !same || a.gone || b.gone || a.carried || b.carried {
                    continue;
                }
                let overlap = (a.r + b.r) * 0.95 - (b.c.x - a.c.x).abs();
                if overlap <= 0.0 {
                    continue;
                }
                let n = sign(b.c.x - a.c.x);
                let (heavy_a, heavy_b) = (riders.contains(&a.id), riders.contains(&b.id));
                let (m1, m2) = (a.r * a.r, b.r * b.r);
                if heavy_a && heavy_b {
                    self.shift_stack(i, -n * overlap * 0.1);
                    self.shift_stack(j, n * overlap * 0.1);
                    continue;
                }
                let share = if heavy_a {
                    0.0
                } else if heavy_b {
                    1.0
                } else {
                    m2 / (m1 + m2)
                };
                self.list[i].c.x -= n * overlap * share;
                self.list[j].c.x += n * overlap * (1.0 - share);
                let closing = (self.list[i].v.x - self.list[j].v.x) * n;
                if closing > 0.0 {
                    if heavy_a {
                        self.list[j].v.x = self.list[i].v.x + n * closing * 0.4;
                    } else if heavy_b {
                        self.list[i].v.x = self.list[j].v.x - n * closing * 0.4;
                    } else {
                        let impulse = closing * 1.4 / (1.0 / m1 + 1.0 / m2);
                        self.list[i].v.x -= n * impulse / m1;
                        self.list[j].v.x += n * impulse / m2;
                    }
                }
            }
        }
    }

    fn shift_stack(&mut self, i: usize, dx: f64) {
        for k in self.stack(i) {
            self.list[k].c.x += dx;
        }
    }

    /// 3.5G: the top two balls of a stack of 3+ wear the snowman.
    fn dress(&mut self) {
        for b in &mut self.list {
            b.decor = Decor::Plain;
            b.decor_look = Look::classic();
        }
        for i in 0..self.list.len() {
            if self.list[i].gone || matches!(self.list[i].support, Support::Ball(_)) {
                continue;
            }
            let stack = self.stack(i);
            let n = stack.len();
            if n < 3 {
                continue;
            }
            let look = self.list[i].look.unwrap_or_else(Look::classic);
            for (k, decor) in [(stack[n - 1], Decor::Head), (stack[n - 2], Decor::Body)] {
                let b = &mut self.list[k];
                b.decor = decor;
                b.decor_look = look;
                b.angle = 0.0;
            }
        }
    }

    /// Decides which sprites need new art.
    fn refresh(&mut self) {
        for b in &mut self.list {
            let look_changed = b.decor != Decor::Plain && b.decor_look != b.drawn.look;
            let spun = b.decor == Decor::Plain && (b.angle - b.drawn.angle).abs() >= REDRAW_ANGLE;
            if (b.r - b.drawn.r).abs() >= REDRAW_RADIUS || b.decor != b.drawn.decor || look_changed || spun {
                b.drawn = Drawn { r: b.r, angle: b.angle, decor: b.decor, look: b.decor_look };
                b.rev += 1;
            }
        }
    }

    // ---- sprites (3.7) ----

    /// Window descriptions for this frame. The dragged stack comes last so
    /// it is topmost.
    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        let held = self.drag.and_then(|d| self.idx(d.id)).map(|i| self.stack(i)).unwrap_or_default();
        let held = &held;
        let live =
            |first: bool| self.list.iter().enumerate().filter(move |(i, b)| !b.gone && held.contains(i) == first);
        for (_, b) in live(false).chain(live(true)) {
            let side = panel_size(b.drawn.decor, b.drawn.r);
            out.push(Sprite {
                key: SpriteKey::new(Owner::Ball, b.id),
                frame: Frame::new(b.c.x - side / 2.0, b.c.y - side / 2.0, side, side),
                // 3.7: every ball window is ordered front (topmost).
                z: ZBand::Front,
                hit: if b.hidden { HitMode::Never } else { HitMode::Alpha },
                rev: b.rev,
                opacity: if b.hidden { 0.0 } else { 1.0 },
            });
        }
    }

    /// Paint one ball sprite at `scale` (delegates to `fp_snowman::paint_ball`).
    pub fn paint(&self, id: u64, scale: f32) -> Option<fp_paint::tiny_skia::Pixmap> {
        let b = &self.list[self.idx(id)?];
        let px = (panel_size(b.drawn.decor, b.drawn.r) * f64::from(scale)).ceil() as u32;
        let mut pixmap = fp_paint::tiny_skia::Pixmap::new(px, px)?;
        paint_ball(&mut pixmap, scale, b.drawn.r, b.drawn.angle, b.seed, b.drawn.decor, &b.drawn.look);
        Some(pixmap)
    }

    // ---- persistence (3.10) ----

    fn stack_record(&self, i: usize) -> Saved {
        let stack = self.stack(i);
        let balls: Vec<&Ball> = stack.iter().map(|&k| &self.list[k]).collect();
        Saved::bare(
            balls.iter().map(|b| b.r).collect(),
            balls.iter().map(|b| b.seed).collect(),
            balls.iter().map(|b| b.angle).collect(),
            balls[0].look,
        )
    }

    /// `standingSnowmen` JSON (3.10, snowman F6), `None` when a save must be skipped.
    pub fn save_json(&self) -> Option<Value> {
        let flying = self.list.iter().any(|b| !b.gone && b.support == Support::Air);
        if self.drag.is_some() || self.snap.is_empty() || flying {
            return None;
        }
        let mut records: Vec<Value> = self.waiting.iter().map(Saved::to_value).collect();
        for (i, b) in self.list.iter().enumerate() {
            let Support::Ledge(key) = b.support else { continue };
            let Some(ledge) = self.snap.iter().find(|l| l.key == key) else { continue };
            if b.gone {
                continue;
            }
            let mut rec = self.stack_record(i);
            rec.screen = key.origin;
            rec.across = b.c.x;
            match key.id {
                SurfaceId::Ground => {}
                SurfaceId::Dock => rec.place = Place::Dock,
                SurfaceId::Window(handle) => {
                    // 3.10: an unresolved owner aborts the whole save.
                    if ledge.owner.is_empty() {
                        return None;
                    }
                    rec.place = Place::Window;
                    rec.id = handle;
                    rec.owner = ledge.owner.clone();
                    rec.middle = ledge.middle;
                    rec.top = ledge.top;
                    rec.across = b.c.x - ledge.middle;
                }
            }
            records.push(rec.to_value());
        }
        Some(Value::Array(records))
    }

    /// Autosave cadence (3.10 `Remember`): emits `Dirty` when the record changed.
    fn remember(&mut self, now: f64, out: &mut Vec<BallEvent>) {
        let last = *self.last_save.get_or_insert(now);
        if !self.force_save && now - last < REMEMBER_EVERY {
            return;
        }
        self.force_save = false;
        self.last_save = Some(now);
        if let Some(json) = self.save_json().filter(|j| *j != self.last_json) {
            self.last_json = json;
            out.push(BallEvent::Dirty);
        }
    }

    /// Queue saved stacks for restore.
    pub fn restore(&mut self, saved: &Value, now: f64) {
        let list = saved.as_array().map(|a| a.iter().filter_map(Saved::from_value).collect());
        self.waiting = list.unwrap_or_default();
        self.waiting_since = now;
        self.last_bring = f64::NEG_INFINITY;
        self.last_json = Value::Array(self.waiting.iter().map(Saved::to_value).collect());
    }

    /// 3.10 `BringBack`: tries every waiting stack every 0.25 s.
    fn bring_back(&mut self, now: f64) {
        if self.waiting.is_empty() || self.snap.is_empty() || now - self.last_bring < BRING_EVERY {
            return;
        }
        self.last_bring = now;
        let waited = now - self.waiting_since;
        let mut keep = Vec::new();
        for saved in std::mem::take(&mut self.waiting) {
            match self.place_for(&saved, waited) {
                Some(k) => {
                    let ledge = self.snap[k].clone();
                    let x = if saved.place == Place::Window { saved.across + ledge.middle } else { saved.across };
                    self.stand_up(&saved, x, &ledge);
                }
                None if waited > GIVE_UP => {}
                None => keep.push(saved),
            }
        }
        self.waiting = keep;
    }

    fn ground_index(&self, origin: Option<Vec2>) -> Option<usize> {
        let fits = |l: &Ledge| l.key.id == SurfaceId::Ground && origin.is_none_or(|o| l.key.origin == o);
        self.snap.iter().position(fits)
    }

    fn place_for(&self, s: &Saved, waited: f64) -> Option<usize> {
        match s.place {
            Place::Ground => self.ground_index(Some(s.screen)).or(self.ground_index(None)),
            Place::Dock => {
                let dock = self.snap.iter().position(|l| l.key.id == SurfaceId::Dock && l.key.origin == s.screen);
                dock.or(if waited > DOCK_PATIENCE { self.ground_index(None) } else { None })
            }
            Place::Window => {
                let exact = self.snap.iter().position(|l| l.key.id == SurfaceId::Window(s.id) && l.owner == s.owner);
                exact.or_else(|| self.nearest_window(s))
            }
        }
    }

    fn nearest_window(&self, s: &Saved) -> Option<usize> {
        self.snap
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.key.id, SurfaceId::Window(_)) && l.owner == s.owner)
            .map(|(k, l)| (k, (l.middle - s.middle).abs(), (l.top - s.top).abs()))
            .filter(|&(_, dm, dt)| dm < 60.0 && dt < 60.0)
            .min_by(|a, b| (a.1 + a.2).total_cmp(&(b.1 + b.2)))
            .map(|(k, _, _)| k)
    }

    /// Builds a stack from a record; ignores the ball cap (3.10 `StandUp`).
    fn push_stack(&mut self, s: &Saved, x: f64, bottom_y: f64, support: Support, carried: bool) -> u64 {
        let mut y = bottom_y;
        let mut prev: Option<(u64, f64)> = None;
        let mut first = None;
        for (k, &r) in s.radii.iter().enumerate() {
            let id = self.next_id;
            self.next_id += 1;
            if let Some((_, pr)) = prev {
                y += (pr + r) * STACK_SPACING;
            }
            let mut b = Ball::new(id, Vec2::new(x, y), r, prev.map_or(support, |(pid, _)| Support::Ball(pid)));
            if let Some(seed) = s.seeds.as_ref().and_then(|v| v.get(k)) {
                b.seed = *seed;
            }
            b.angle = s.angles.as_ref().and_then(|v| v.get(k)).copied().unwrap_or(0.0);
            b.drawn.angle = b.angle;
            b.carried = carried;
            if prev.is_none() {
                b.look = s.look.and_then(Look::stored);
            }
            self.list.push(b);
            prev = Some((id, r));
            first.get_or_insert(id);
        }
        let bottom = first.unwrap_or(0);
        let base = self.list.iter().find(|b| b.id == bottom).map_or(Vec2::ZERO, |b| b.c);
        for b in self.list.iter_mut().filter(|b| b.id >= bottom) {
            b.offset = b.c - base;
        }
        bottom
    }

    fn stand_up(&mut self, s: &Saved, x: f64, ledge: &Ledge) {
        let x = x.max(ledge.min_x).min(ledge.max_x);
        self.push_stack(s, x, ledge.top - 1.0 + s.radii[0], Support::Ledge(ledge.key), false);
    }

    // ---- local toy train cargo (characters 5.3 / 8) ----
    // Cargo is an opaque JSON value (one saved stack, snowballs 3.10 record
    // without window data) carried by `fp-characters` in `trainQueue`.

    /// Standing snowmen that can be picked for a ride: `(bottom, foot)`.
    pub fn standing_snowmen(&self) -> Vec<(u64, Vec2)> {
        let mut out = Vec::new();
        for (i, b) in self.list.iter().enumerate() {
            let standing = !b.gone && !b.carried && matches!(b.support, Support::Ledge(_));
            if standing && self.stack(i).len() >= 2 {
                out.push((b.id, Vec2::new(b.c.x, b.c.y - b.r)));
            }
        }
        out
    }

    /// Carry this stack so its bottom sits at `point`; `None` releases it.
    pub fn carry(&mut self, bottom: u64, point: Option<Vec2>) {
        let Some(i) = self.idx(bottom) else { return };
        let stack = self.stack(i);
        let Some(point) = point else {
            for k in stack {
                self.list[k].carried = false;
                self.list[k].shift = Vec2::ZERO;
            }
            return;
        };
        let base = self.list[stack[0]].c;
        let anchor = Vec2::new(point.x, point.y + self.list[stack[0]].r);
        let first = stack[0];
        for k in stack {
            let b = &mut self.list[k];
            if !b.carried {
                b.carried = true;
                b.offset = b.c - base;
                b.shift = b.c - (anchor + b.offset);
                if k == first {
                    b.support = Support::Air;
                }
            }
            let target = anchor + b.offset;
            b.shift = b.shift * CARRY_DECAY;
            if b.shift.len() < CARRY_SNAP {
                b.shift = Vec2::ZERO;
            }
            b.c = target + b.shift;
            b.v = Vec2::ZERO;
            b.moved = Vec2::ZERO;
            b.hidden = false;
        }
    }

    fn remove(&mut self, ids: &[u64]) {
        self.list.retain(|b| !ids.contains(&b.id));
        if self.drag.is_some_and(|d| ids.contains(&d.id)) {
            self.drag = None;
        }
        if self.pressed.is_some_and(|(id, _)| ids.contains(&id)) {
            self.pressed = None;
        }
    }

    /// Remove a stack and return its cargo value.
    pub fn take(&mut self, bottom: u64) -> Option<Value> {
        let i = self.idx(bottom)?;
        let value = self.stack_record(i).to_value();
        let ids: Vec<u64> = self.stack(i).iter().map(|&k| self.list[k].id).collect();
        self.remove(&ids);
        Some(value)
    }

    /// Create a carried stack from cargo at `point` (cap of 30 balls still applies).
    pub fn board(&mut self, cargo: &Value, point: Vec2) -> Option<u64> {
        let saved = Saved::from_value(cargo)?;
        if self.count() + saved.radii.len() > MAX_BALLS {
            return None;
        }
        Some(self.push_stack(&saved, point.x, point.y + saved.radii[0], Support::Air, true))
    }

    /// Remove a stack silently.
    pub fn vanish(&mut self, bottom: u64) {
        if let Some(i) = self.idx(bottom) {
            let ids: Vec<u64> = self.stack(i).iter().map(|&k| self.list[k].id).collect();
            self.remove(&ids);
        }
    }

    /// Stand cargo at `x` on the ground ledge of `screen` (global frame);
    /// `false` when that screen has no ground ledge (characters 5.3).
    pub fn stand(&mut self, cargo: &Value, x: f64, screen: Frame, ledges: &Ledges) -> bool {
        let Some(saved) = Saved::from_value(cargo) else { return false };
        let Some(ledge) = ledges.ground_of(screen.min()) else { return false };
        self.stand_up(&saved, x, ledge);
        self.force_save = true;
        true
    }
}

#[cfg(test)]
mod tests;
