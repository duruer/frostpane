//! Decorated Christmas trees.
//!
//! Spec: trees 1, 2 (TreeLook, TreeShape, TreeSnowPatch, TreeRandom ==
//! `fp_core::rng::Lcg64`, SavedTree JSON), 3.1-3.17 (planting, growth, click
//! and drag, tilt spring, supported/falling, snow accumulation and shaking,
//! clumps, style card model, restyle, chop/yeti felling, limit, blinking,
//! persistence timing, restore, windows as sprites, procedural art), 4.
//! Frostpane changes: the 3-tree cap becomes `Settings::max_placed`
//! (default 100); the network SentTree format and send-by-train are dropped;
//! every topper and trim is available.
//!
//! Driving it: the world calls [`Grove::step`] every frame with the tracker's
//! snapshots and the pointer, collects [`Grove::sprites`], repaints sprites
//! whose `rev` changed with [`Grove::paint`], and writes
//! [`Grove::save_json`] whenever a [`TreeEvent::Dirty`] arrives. The style
//! card is open while [`Grove::card_open`] returns the tree's id; the host
//! closes the card window when it returns `None` or another id.

mod art;
mod clumps;
mod look;
mod shape;
mod shelf;
mod store;

pub use art::{little_tree, topper_icon, trim_icon};
pub use look::{MAX_LEVEL, Topper, TreeLook, Trim};
pub use shape::{Patch, Tier, TreeShape, generate_patches};
pub use store::SavedTree;

use clumps::{CLUMP_BASE, Clumps, Spawn};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng, mix_seed};
use fp_core::scene::{HitMode, Owner, PointerInput, Sprite, SpriteKey, ZBand};
use fp_core::settings::DEFAULT_MAX_PLACED;
use fp_core::surface::{LedgeKey, ScreenSnap, SurfaceId};
use serde_json::Value;
use shelf::{Ledges, Shelf, min_key};
use store::{Cargo, PLACE_DOCK, PLACE_GROUND, PLACE_WINDOW, b64_decode, b64_encode, parse_saved, to_byte};

/// Style card content size in points (trees 3.9); the window adds a 22 pt
/// margin on every side for the shadow.
pub const CARD_SIZE: (f64, f64) = (340.0, 420.0);

const GRAVITY: f64 = 2500.0;
const SNOW_EVERY: f64 = 3.0;
const FULL_IN: f64 = 1200.0;
const REMEMBER_EVERY: f64 = 6.0;
const BLINK_EVERY: f64 = 0.8;
const RESTORE_EVERY: f64 = 0.25;
const DOCK_WAIT: f64 = 6.0;
const WINDOW_WAIT: f64 = 180.0;
const SAME_SPOT: f64 = 60.0;
const DRAG_THRESHOLD: f64 = 4.0;
const EDGE: f64 = 8.0;
const GROWTH_RATE: f64 = 2.6;
const CHOPS_TO_FELL: u8 = 3;
const SNOW_REDRAW: f64 = 1.0 / 14.0;

#[derive(Clone, Debug, PartialEq)]
pub enum TreeEvent {
    /// Open the style card for this tree. `quip` = refusal line 1 or 2 when
    /// the card opens because the cap was hit (3.1), else `None`.
    Style { id: u64, quip: Option<u8> },
    /// Clumps of snow fell (bursts are routed by the world). `at` is a global
    /// point, `area` the burst size (3 for a clump, 24 for a felled tree),
    /// `velocity` the burst velocity ((vx,0) for a clump, zero for a felled tree).
    Snow { at: Vec2, area: f64, velocity: Vec2 },
    /// Third refusal in a row at the cap: show the limit puppet (snowman F9,
    /// trees 3.12). Refusals 1 and 2 come as `Style { quip: Some(n) }`.
    LimitReached,
    /// The saved state changed and is due: call [`Grove::save_json`].
    Dirty,
}

#[derive(Debug)]
struct Held {
    offset: Vec2,
    start: Vec2,
}

#[derive(Debug)]
struct Tree {
    id: u64,
    look: TreeLook,
    shape: TreeShape,
    seed: u64,
    patches: Vec<Patch>,
    cap: f64,
    foot: Vec2,
    vel: Vec2,
    support: Option<LedgeKey>,
    across: f64,
    growth: f64,
    tilt: f64,
    tilt_speed: f64,
    hidden: bool,
    held: Option<Held>,
    lifted: bool,
    doomed: bool,
    felling: bool,
    fall_way: f64,
    fell: f64,
    chops: u8,
    shed: f64,
    shed_limit: f64,
    shaken_at: f64,
    carried: bool,
    rev: u64,
    snow_dirty: bool,
    snow_drawn: f64,
}

fn rotate(p: Vec2, a: f64) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
}

fn clamp_to(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

impl Tree {
    fn new(id: u64, look: TreeLook, seed: u64, foot: Vec2) -> Tree {
        let look = look.clamped();
        let shape = TreeShape::from_level(look.level);
        let patches = generate_patches(&shape, seed);
        Tree {
            id,
            look,
            shape,
            seed,
            patches,
            cap: 0.0,
            foot,
            vel: Vec2::ZERO,
            support: None,
            across: 0.0,
            growth: 1.0,
            tilt: 0.0,
            tilt_speed: 0.0,
            hidden: false,
            held: None,
            lifted: false,
            doomed: false,
            felling: false,
            fall_way: 0.0,
            fell: 0.0,
            chops: 0,
            shed: 0.0,
            shed_limit: 0.0,
            shaken_at: f64::NEG_INFINITY,
            carried: false,
            rev: 1,
            snow_dirty: false,
            snow_drawn: f64::NEG_INFINITY,
        }
    }

    fn key(&self) -> SpriteKey {
        SpriteKey::new(Owner::Tree, self.id)
    }

    fn snow_total(&self) -> f64 {
        self.patches.iter().map(|p| p.depth).sum()
    }

    fn add_snow(&mut self, strength: f64) {
        let inc = strength * SNOW_EVERY / FULL_IN;
        for p in self.patches.iter_mut().filter(|p| p.depth < p.fullest) {
            p.depth = p.fullest.min(p.depth + inc * p.weight);
        }
        self.cap = (self.cap + inc).min(1.0);
        self.snow_dirty = true;
    }

    /// 3.7 ShakeOff.
    fn shake_off(&mut self, rng: &mut Lcg64, share: f64, spawns: &mut Vec<Spawn>) {
        let total = self.snow_total();
        if total <= 0.5 {
            return;
        }
        if self.shed == 0.0 {
            self.shed_limit = total * rng.range(0.4, 0.62);
        }
        let mut amount = (total * share).min(self.shed_limit - self.shed);
        if amount <= 0.0 {
            return;
        }
        let reach = self.shape.unit() * 2.4;
        let mut attempts = 0;
        while amount > 0.0 && attempts < 30 {
            attempts += 1;
            let pick = &self.patches[rng.below(self.patches.len() as u64) as usize];
            if !(pick.depth > 0.08 && rng.range(0.0, 0.82) > pick.cling) {
                continue;
            }
            let (px, py) = (pick.x, pick.y);
            let (mut taken, mut cx, mut cy, mut q) = (0.0, 0.0, 0.0, 0.0);
            for p in self.patches.iter_mut() {
                if (p.x - px).hypot(p.y - py) <= reach && p.depth > 0.05 && rng.unit() > p.cling * 0.9 + 0.1 {
                    let take = p.depth * rng.range(0.6, 1.0);
                    p.depth -= take;
                    taken += take;
                    cx += p.x * take;
                    cy += p.y * take;
                    q += p.widest * p.widest * (0.3 + take);
                }
            }
            if taken > 0.0 {
                amount -= taken;
                self.shed += taken;
                let at = self.foot + rotate(Vec2::new(cx / taken, cy / taken), self.tilt);
                let radius = (self.shape.unit() * 1.2).min(2.5_f64.max(q.sqrt() * 0.72));
                spawns.push(Spawn { point: at, vel: self.vel, radius });
            }
        }
        if self.cap > 0.2 && rng.unit() < share * 6.0 {
            self.cap *= 0.5;
            let at = Vec2::new(self.foot.x, self.foot.y + self.shape.height + self.shape.topper_size());
            spawns.push(Spawn { point: at, vel: self.vel, radius: 4.0 });
        }
        self.snow_dirty = true;
    }

    /// 3.5 supported branch of Move.
    fn follow_shelf(&mut self, key: LedgeKey, shelves: &[Shelf], dt: f64) {
        match shelves.iter().find(|s| s.key == key) {
            Some(s) if !s.jumped => {
                self.foot = Vec2::new(s.middle + self.across, s.top - 1.0);
                if dt > 0.0 {
                    self.vel = s.vel;
                }
                self.hidden = !s.shown || s.blocked_at(self.foot.x);
                if !s.spans(self.foot.x) {
                    self.support = None;
                }
            }
            _ => self.support = None,
        }
    }

    /// 3.5 Drop. Returns true when the tree landed this step.
    fn fall(&mut self, shelves: &[Shelf], dt: f64, rng: &mut Lcg64, spawns: &mut Vec<Spawn>) -> bool {
        self.vel.y -= GRAVITY * dt;
        let old = self.foot;
        self.foot = old + self.vel * dt;
        self.hidden = false;
        let fx = self.foot.x;
        let gap = |s: &Shelf| (s.min_x - fx).max(0.0).max(fx - s.max_x);
        if let Some(g) = min_key(shelves.iter().filter(|s| s.ground), gap) {
            self.foot.x = clamp_to(self.foot.x, g.min_x + EDGE, g.max_x - EDGE);
        }
        let foot = self.foot;
        let doomed = self.doomed;
        let landing = min_key(
            shelves.iter().filter(|s| {
                !s.ground
                    && s.shown
                    && s.spans(foot.x)
                    && (!doomed || s.is_dock())
                    && old.y >= s.top - 1.0 - 0.5
                    && foot.y <= s.top - 1.0
                    && !s.blocked_at(foot.x)
            }),
            |s| -s.top,
        )
        .or_else(|| shelves.iter().find(|s| s.ground && s.spans(foot.x) && foot.y <= s.top - 1.0));
        let Some(s) = landing else {
            return false;
        };
        let impact = -self.vel.y;
        self.foot.y = s.top - 1.0;
        self.vel = Vec2::ZERO;
        self.support = Some(s.key);
        self.across = self.foot.x - s.middle;
        self.tilt_speed += rng.range(-1.0, 1.0) * (impact / 900.0).min(2.0);
        if impact > 700.0 {
            self.shake_off(rng, (impact / 8000.0).min(0.3), spawns);
        }
        true
    }

    /// 3.2 visual scale about the foot.
    fn visual_scale(&self) -> f64 {
        if self.felling || self.growth >= 1.0 {
            self.growth
        } else {
            let g = self.growth - 1.0;
            (1.0 + 2.6 * g * g * g + 1.6 * g * g).max(0.01)
        }
    }

    /// Growth, felling and the tilt spring (3.2, 3.4, 3.11).
    fn swing(&mut self, dt: f64) {
        if self.felling {
            self.fell += dt;
            self.tilt = self.fall_way * ((self.fell / 0.35).powi(2) * 0.55).min(0.55);
            if self.fell > 0.3 {
                self.growth = (self.growth - 5.0 * dt).max(0.0);
            }
            self.rev += 1;
            return;
        }
        if self.growth < 1.0 {
            self.growth = (self.growth + GROWTH_RATE * dt).min(1.0);
            self.rev += 1;
        }
        let target = if self.lifted { (-self.vel.x / 9000.0).clamp(-0.17, 0.17) } else { 0.0 };
        if self.lifted || self.tilt.abs() > 0.0008 || self.tilt_speed.abs() > 0.004 {
            self.tilt_speed += (-170.0 * (self.tilt - target) - 11.0 * self.tilt_speed) * dt;
            self.tilt += self.tilt_speed * dt;
            self.rev += 1;
        } else {
            self.tilt = 0.0;
            self.tilt_speed = 0.0;
        }
    }

    fn cargo(&self) -> Cargo {
        let depths: Vec<u8> = self.patches.iter().map(|p| to_byte(p.depth)).collect();
        Cargo { look: self.look.clone(), seed: self.seed, depths: b64_encode(&depths), cap: to_byte(self.cap) }
    }

    fn sprite(&self) -> Sprite {
        let (w, h) = self.shape.canvas();
        let takes = !self.doomed && !self.felling && !self.hidden && !self.carried && self.growth >= 1.0;
        Sprite {
            key: self.key(),
            frame: Frame::new(self.foot.x - self.shape.pad(), self.foot.y - 8.0, w, h),
            z: ZBand::Front,
            hit: if takes || self.held.is_some() { HitMode::Alpha } else { HitMode::Never },
            rev: self.rev,
            opacity: if self.hidden { 0.0 } else { 1.0 },
        }
    }
}

/// Depths from saved bytes (3.15): equal length maps 1:1, otherwise the mean
/// scaled by each patch's weight and capped at its fullest.
fn apply_saved_depths(patches: &mut [Patch], bytes: &[u8], exact_only: bool) {
    if bytes.len() == patches.len() {
        for (p, b) in patches.iter_mut().zip(bytes) {
            p.depth = f64::from(*b) / 255.0;
        }
    } else if !bytes.is_empty() && !exact_only {
        let mean = bytes.iter().map(|b| f64::from(*b)).sum::<f64>() / bytes.len() as f64 / 255.0;
        for p in patches.iter_mut() {
            p.depth = (mean * p.weight).min(p.fullest);
        }
    }
}

/// All trees.
#[derive(Debug)]
pub struct Grove {
    max: u32,
    trees: Vec<Tree>,
    waiting: Vec<SavedTree>,
    next_id: u64,
    refusals: u8,
    shelves: Vec<Shelf>,
    ledges: Ledges,
    pointer: Option<Vec2>,
    prev_down: bool,
    rng: Lcg64,
    clumps: Clumps,
    pending: Vec<TreeEvent>,
    snow_at: Option<f64>,
    blink_at: Option<f64>,
    blink: u8,
    restore_at: f64,
    waiting_since: f64,
    remembered_at: Option<f64>,
    force_save: bool,
    last_json: Option<String>,
    card: Option<u64>,
    carry_point: Option<Vec2>,
}

impl Default for Grove {
    fn default() -> Self {
        Grove::new(DEFAULT_MAX_PLACED)
    }
}

impl Grove {
    /// `max` = `Settings::max_placed` (the cap on planted trees).
    pub fn new(max: u32) -> Self {
        Grove {
            max,
            trees: Vec::new(),
            waiting: Vec::new(),
            next_id: 1,
            refusals: 0,
            shelves: Vec::new(),
            ledges: Ledges::default(),
            pointer: None,
            prev_down: false,
            rng: Lcg64::new(0x7265_6573),
            clumps: Clumps::default(),
            pending: Vec::new(),
            snow_at: None,
            blink_at: None,
            blink: 0,
            restore_at: 0.0,
            waiting_since: 0.0,
            remembered_at: None,
            force_save: false,
            last_json: None,
            card: None,
            carry_point: None,
        }
    }

    /// Standing (non-felling) trees.
    pub fn count(&self) -> usize {
        self.trees.iter().filter(|t| !t.felling).count()
    }

    pub fn look(&self, id: u64) -> Option<&TreeLook> {
        self.tree(id).map(|t| &t.look)
    }

    /// Tree whose style card should be open, if any.
    pub fn card_open(&self) -> Option<u64> {
        self.card
    }

    /// The user closed the card window.
    pub fn close_card(&mut self) {
        self.card = None;
    }

    /// Window origin of the style card for tree `id` on the work area
    /// `work` of its monitor (3.9): beside the tree on the right if it fits,
    /// else on the left, 22 pt shadow margin included.
    pub fn card_origin(&self, id: u64, work: Frame) -> Option<Vec2> {
        let t = self.tree(id)?;
        let right = t.foot.x + t.shape.half + 18.0;
        let x = if right + CARD_SIZE.0 <= work.max_x() - 8.0 {
            right
        } else {
            (work.min_x() + 8.0).max(t.foot.x - t.shape.half - 18.0 - CARD_SIZE.0)
        };
        let y = (t.foot.y + t.shape.height * 0.25).max(work.min_y() + 70.0).min(work.max_y() - CARD_SIZE.1 - 40.0);
        Some(Vec2::new(x - 22.0, y - 22.0))
    }

    /// Anchor that picks the monitor of tree `id`'s style card (3.9): the
    /// centre X and `MinY + 10` of the ground strip holding the tree's foot,
    /// else of the ground strip with the nearest centre X. `None` when the
    /// tree is unknown or there is no ground strip (callers use the primary).
    pub fn card_anchor(&self, id: u64) -> Option<Vec2> {
        let foot = self.tree(id)?.foot;
        let ground = || self.shelves.iter().filter(|s| s.ground);
        let strip = ground().find(|s| s.spans(foot.x)).or_else(|| min_key(ground(), |s| (s.middle - foot.x).abs()))?;
        Some(Vec2::new(strip.middle, strip.screen.min_y() + 10.0))
    }

    /// Bring a tree to the front of the sprite order.
    pub fn raise(&mut self, id: u64) {
        if let Some(i) = self.trees.iter().position(|t| t.id == id) {
            let t = self.trees.remove(i);
            self.trees.push(t);
        }
    }

    fn tree(&self, id: u64) -> Option<&Tree> {
        self.trees.iter().find(|t| t.id == id)
    }

    fn tree_mut(&mut self, id: u64) -> Option<&mut Tree> {
        self.trees.iter_mut().find(|t| t.id == id)
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn close_card_of(&mut self, id: u64) {
        if self.card == Some(id) {
            self.card = None;
        }
    }

    // ---- planting (3.1) ----

    /// Plant a tree at a free spot (3.1); `false` when no shelf is known yet
    /// or the cap is reached (refusals 1 and 2 open the style card of the
    /// last tree with a quip, refusal 3 emits `LimitReached`).
    pub fn plant(&mut self, _now: f64, seed: u64, out: &mut Vec<TreeEvent>) -> bool {
        if self.shelves.is_empty() {
            return false;
        }
        if self.count() + self.waiting.len() >= self.max as usize {
            self.refusals += 1;
            if self.refusals >= 3 {
                self.refusals = 0;
                out.push(TreeEvent::LimitReached);
            } else if let Some(id) = self.trees.iter().rev().find(|t| !t.felling && !t.doomed).map(|t| t.id) {
                self.card = Some(id);
                out.push(TreeEvent::Style { id, quip: Some(self.refusals) });
            }
            return false;
        }
        self.refusals = 0;
        self.rng = Lcg64::new(mix_seed(seed));
        let (x, shelf) = self.pick_spot();
        let (top, middle, key) = (self.shelves[shelf].top, self.shelves[shelf].middle, self.shelves[shelf].key);
        let id = self.take_id();
        let mut tree = Tree::new(id, TreeLook::default(), seed, Vec2::new(x, top - 1.0));
        tree.growth = 0.01;
        tree.support = Some(key);
        tree.across = x - middle;
        self.trees.push(tree);
        self.card = Some(id);
        self.force_save = true;
        out.push(TreeEvent::Style { id, quip: None });
        true
    }

    /// PlantingSpot: 16 samples in the free intervals, keep the one farthest
    /// from every existing tree. Returns `(x, shelf index)`.
    fn pick_spot(&mut self) -> (f64, usize) {
        let ground = |s: &Shelf| s.ground;
        let all = &self.shelves;
        let at = |p: Vec2| all.iter().position(|s| ground(s) && s.shown && s.spans(p.x));
        let gi = self
            .pointer
            .and_then(at)
            .or_else(|| all.iter().position(|s| ground(s) && s.shown))
            .or_else(|| all.iter().position(ground))
            .unwrap_or(0);
        let g = &all[gi];
        let m = TreeShape::from_level(TreeLook::default().level).half + 30.0;
        let dock = all.iter().position(|s| s.is_dock() && s.key.origin == g.key.origin);
        let mut spans: Vec<(f64, f64, usize)> = Vec::new();
        if let Some(di) = dock {
            let d = &all[di];
            if d.min_x - m - g.min_x > m {
                spans.push((g.min_x + m, d.min_x - m, gi));
            }
            if g.max_x - m - d.max_x > m {
                spans.push((d.max_x + m, g.max_x - m, gi));
            }
            if spans.is_empty() && d.max_x - d.min_x > 2.0 * m {
                spans.push((d.min_x + m, d.max_x - m, di));
            }
        }
        if spans.is_empty() {
            spans.push((g.min_x + m, g.max_x - m, gi));
        }
        let mut best = (0.0, gi, f64::NEG_INFINITY);
        for _ in 0..16 {
            let (lo, hi, shelf) = spans[self.rng.below(spans.len() as u64) as usize];
            let x = self.rng.range(lo, hi);
            let far = self.trees.iter().map(|t| (t.foot.x - x).abs()).fold(f64::MAX, f64::min);
            if far > best.2 {
                best = (x, shelf, far);
            }
        }
        (best.0, best.1)
    }

    // ---- per frame ----

    pub fn step(
        &mut self,
        dt: f64,
        now: f64,
        ledges: &[ScreenSnap],
        pointer: &PointerInput,
        snowing: bool,
        out: &mut Vec<TreeEvent>,
    ) {
        out.append(&mut self.pending);
        let shelves = self.ledges.build(now, ledges);
        if pointer.pos.is_some() {
            self.pointer = pointer.pos;
        }
        let remembered = *self.remembered_at.get_or_insert(now);
        let snow_fire = now >= *self.snow_at.get_or_insert(now + SNOW_EVERY);
        if snow_fire {
            self.snow_at = Some(now + SNOW_EVERY);
        }
        let toggled = self.blink_at.is_none_or(|t| now >= t);
        if toggled {
            self.blink ^= 1;
            self.blink_at = Some(now + BLINK_EVERY);
        }
        self.pointer_step(pointer, dt, out);
        let carry = self.carry_point;
        let mut spawns = Vec::new();
        for t in self.trees.iter_mut() {
            let mut landed = false;
            if t.carried {
                if let Some(p) = carry {
                    t.foot = p;
                }
                t.support = None;
                t.vel = Vec2::ZERO;
                t.hidden = false;
            } else if let Some(key) = t.support {
                t.follow_shelf(key, &shelves, dt);
            } else if !t.lifted {
                landed = t.fall(&shelves, dt, &mut self.rng, &mut spawns);
            }
            self.force_save |= landed;
            if t.lifted || t.support.is_some() {
                let speed = t.vel.len();
                if speed >= 850.0 {
                    t.shaken_at = now;
                    let k = ((speed - 850.0) / 1550.0).min(1.0);
                    t.shake_off(&mut self.rng, dt * (0.5 + 1.8 * k), &mut spawns);
                } else if now - t.shaken_at > 0.7 {
                    t.shed = 0.0;
                }
            }
            t.swing(dt);
            if snow_fire && snowing && !t.hidden && t.held.is_none() {
                t.add_snow(1.0);
            }
            if toggled && !t.hidden && t.look.has(Trim::Lights) {
                t.rev += 1;
            }
            if t.snow_dirty && now - t.snow_drawn >= SNOW_REDRAW {
                t.snow_dirty = false;
                t.snow_drawn = now;
                t.rev += 1;
            }
        }
        self.trees.retain(|t| !(t.felling && t.growth <= 0.0));
        if self.card.is_some_and(|id| self.tree(id).is_none()) {
            self.card = None;
        }
        for s in spawns {
            self.clumps.spawn(&shelves, &mut self.rng, s);
        }
        self.clumps.step(&shelves, &mut self.rng, dt, out);
        if !self.waiting.is_empty() && !shelves.is_empty() && now >= self.restore_at {
            self.restore_at = now + RESTORE_EVERY;
            self.try_restore(now, &shelves);
        }
        let due = self.force_save || now - remembered >= REMEMBER_EVERY;
        if due
            && !shelves.is_empty()
            && self.trees.iter().all(|t| t.held.is_none())
            && let Some(v) = self.build_json(&shelves)
        {
            self.remembered_at = Some(now);
            self.force_save = false;
            let text = v.to_string();
            if self.last_json.as_deref() != Some(text.as_str()) {
                self.last_json = Some(text);
                out.push(TreeEvent::Dirty);
            }
        }
        self.shelves = shelves;
    }

    /// 3.3 press, lift, drag, release.
    fn pointer_step(&mut self, p: &PointerInput, dt: f64, out: &mut Vec<TreeEvent>) {
        let rising = p.left_down && !self.prev_down;
        self.prev_down = p.left_down;
        let idle = self.trees.iter().all(|t| t.held.is_none());
        if let (true, true, Some(pos), Some(key)) = (rising, idle, p.pos, p.hot)
            && let Some(t) = self.trees.iter_mut().find(|t| t.key() == key && !t.doomed && !t.felling && !t.carried)
        {
            t.held = Some(Held { offset: t.foot - pos, start: pos });
            t.lifted = false;
        }
        let Some(i) = self.trees.iter().position(|t| t.held.is_some()) else {
            return;
        };
        let id = self.trees[i].id;
        if !p.left_down {
            let t = &mut self.trees[i];
            t.held = None;
            if t.lifted {
                t.lifted = false;
                t.vel = Vec2::new(t.vel.x * 0.2, t.vel.y.min(0.0) * 0.3);
            } else if self.card == Some(id) {
                self.card = None;
            } else {
                self.card = Some(id);
                out.push(TreeEvent::Style { id, quip: None });
            }
            return;
        }
        let Some(pos) = p.pos else {
            return;
        };
        let t = &mut self.trees[i];
        let held = t.held.as_ref().map(|h| (h.offset, h.start));
        let (offset, start) = held.unwrap_or_default();
        let lifting = !t.lifted && pos.dist(start) > DRAG_THRESHOLD;
        if lifting {
            t.lifted = true;
            t.support = None;
        }
        if t.lifted {
            let new = pos + offset;
            if dt > 0.0 {
                t.vel = t.vel * 0.6 + (new - t.foot) * (1.0 / dt) * 0.4;
            }
            t.foot = new;
            t.hidden = false;
        }
        if lifting {
            self.close_card_of(id);
            self.raise(id);
        }
    }

    // ---- restyle, felling ----

    /// Replace a tree's look (3.10). A different level regenerates the snow
    /// patches from the seed keeping the average depth.
    pub fn restyle(&mut self, id: u64, look: TreeLook) {
        let look = look.clamped();
        let Some(t) = self.tree_mut(id) else {
            return;
        };
        if look.level != t.look.level {
            let avg = if t.patches.is_empty() { 0.0 } else { t.snow_total() / t.patches.len() as f64 };
            t.shape = TreeShape::from_level(look.level);
            t.patches = generate_patches(&t.shape, t.seed);
            for p in t.patches.iter_mut() {
                p.depth = p.fullest.min(avg * p.weight);
            }
            t.tilt_speed += 1.4;
        }
        t.look = look;
        t.rev += 1;
        self.force_save = true;
    }

    /// Change the cap on planted trees (`Settings::max_placed`). Trees above
    /// a lowered cap stay; only new plantings are refused.
    pub fn set_max(&mut self, max: u32) {
        self.max = max;
    }

    /// Mark a tree for the yeti (3.11). A tree on a window falls to the dock
    /// or ground.
    pub fn doom(&mut self, id: u64) {
        let Some(t) = self.tree_mut(id).filter(|t| !t.felling) else {
            return;
        };
        t.doomed = true;
        if t.support.is_some_and(|k| matches!(k.id, SurfaceId::Window(_))) {
            t.support = None;
        }
        self.close_card_of(id);
    }

    /// DoomNewest (3.11), called when a yeti is summoned without a snowman
    /// target: `true` if a tree is (now) doomed and the yeti should hunt it.
    /// Keeps an already doomed tree; else dooms the newest standing visible
    /// tree, else the newest standing one; `false` with no standing tree.
    pub fn doom_newest(&mut self) -> bool {
        if self.trees.iter().any(|t| t.doomed && !t.felling) {
            return true;
        }
        let standing = || self.trees.iter().rev().filter(|t| !t.felling);
        let Some(id) = standing().find(|t| !t.hidden).or_else(|| standing().next()).map(|t| t.id) else {
            return false;
        };
        self.doom(id);
        true
    }

    /// Yeti target: first doomed, standing, supported tree: `(id, point,
    /// reach radius)`.
    pub fn yeti_target(&self) -> Option<(u64, Vec2, f64)> {
        self.trees
            .iter()
            .find(|t| t.doomed && !t.felling && t.support.is_some() && t.growth >= 1.0)
            .map(|t| (t.id, Vec2::new(t.foot.x, t.foot.y + t.shape.height * 0.3), t.shape.trunk_half() + 24.0))
    }

    pub fn doomed(&self) -> Option<(u64, Vec2, f64)> {
        self.yeti_target()
    }

    /// One yeti chop on tree `id`, the tree falling to the right.
    pub fn chop(&mut self, id: u64) {
        self.chop_from(id, f64::MAX);
    }

    /// One yeti chop from `x` on tree `id` (doomed, standing, supported): the
    /// tree falls away from the yeti; the third chop fells it.
    pub fn chop_from(&mut self, id: u64, x: f64) {
        let Some(t) = self.trees.iter_mut().find(|t| t.id == id && t.doomed && !t.felling && t.support.is_some())
        else {
            return;
        };
        t.chops += 1;
        t.fall_way = if x < t.foot.x { -1.0 } else { 1.0 };
        t.tilt_speed += t.fall_way * 1.8;
        t.shed = 0.0;
        let mut spawns = Vec::new();
        t.shake_off(&mut self.rng, 0.25, &mut spawns);
        if t.chops >= CHOPS_TO_FELL {
            t.felling = true;
            if !t.hidden {
                self.pending.push(TreeEvent::Snow {
                    at: Vec2::new(t.foot.x, t.foot.y + t.shape.height * 0.3),
                    area: 24.0,
                    velocity: Vec2::ZERO,
                });
            }
            self.force_save = true;
        }
        for s in spawns {
            self.clumps.spawn(&self.shelves, &mut self.rng, s);
        }
    }

    // ---- output ----

    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        out.extend(self.trees.iter().map(Tree::sprite));
        self.clumps.sprites(out);
    }

    /// Draws sprite `id` (a tree id, or a clump overlay id as given in
    /// [`Sprite::key`]) at monitor `scale`.
    pub fn paint(&self, id: u64, scale: f32) -> Option<fp_paint::tiny_skia::Pixmap> {
        if id >= CLUMP_BASE {
            return self.clumps.paint((id - CLUMP_BASE) as usize, scale);
        }
        let t = self.tree(id)?;
        let tp = art::TreePaint {
            shape: &t.shape,
            look: &t.look,
            patches: &t.patches,
            cap: t.cap,
            lit: ((u64::from(self.blink) + (t.seed & 1)) % 2) as u8,
            grow: t.visual_scale(),
            tilt: t.tilt,
        };
        art::paint_tree(&tp, scale)
    }

    // ---- persistence (3.14, 3.15) ----

    fn saved_entry(&self, t: &Tree, shelves: &[Shelf]) -> Option<SavedTree> {
        let key = t.support?;
        let sh = shelves.iter().find(|s| s.key == key)?;
        let (place, id, owner, middle, top, across) = match key.id {
            SurfaceId::Ground => (PLACE_GROUND, 0, String::new(), 0.0, 0.0, t.foot.x),
            SurfaceId::Dock => (PLACE_DOCK, 0, String::new(), 0.0, 0.0, t.foot.x),
            SurfaceId::Window(h) if !sh.owner.is_empty() => {
                (PLACE_WINDOW, h, sh.owner.clone(), sh.middle, sh.top, t.foot.x - sh.middle)
            }
            SurfaceId::Window(_) => return None,
        };
        let c = t.cargo();
        Some(SavedTree {
            place,
            id,
            owner,
            middle,
            top,
            screen_x: key.origin.x,
            screen_y: key.origin.y,
            across,
            look: Some(t.look.clone()),
            seed: t.seed,
            depths: Some(c.depths),
            cap: c.cap,
        })
    }

    fn build_json(&self, shelves: &[Shelf]) -> Option<Value> {
        let mut list = self.waiting.clone();
        for t in self.trees.iter().filter(|t| !t.felling) {
            list.push(self.saved_entry(t, shelves)?);
        }
        serde_json::to_value(list).ok()
    }

    /// Snow clumps currently falling.
    pub fn clump_count(&self) -> usize {
        self.clumps.count()
    }

    /// Stop (3.15): returns the final save (same abort rules as
    /// [`save_json`](Self::save_json)) and forgets every tree, clump and shelf.
    pub fn stop(&mut self) -> Option<Value> {
        let saved = self.save_json();
        self.trees.clear();
        self.waiting.clear();
        self.clumps.clear();
        self.shelves.clear();
        self.card = None;
        self.carry_point = None;
        self.prev_down = false;
        saved
    }

    /// The `standingTrees` value, or `None` when saving must wait (a tree is
    /// in the air or carried, or its shelf or owner is unknown).
    pub fn save_json(&self) -> Option<Value> {
        self.build_json(&self.shelves)
    }

    /// Load saved trees (3.15); they stand up as soon as their shelf is known.
    pub fn restore(&mut self, saved: &Value, now: f64) {
        self.waiting = parse_saved(saved);
        self.waiting_since = now;
        self.remembered_at = Some(now);
        self.snow_at = Some(now + SNOW_EVERY);
        self.blink_at = Some(now);
    }

    fn restore_target(&self, s: &SavedTree, now: f64, shelves: &[Shelf]) -> Option<(usize, f64)> {
        let age = now - self.waiting_since;
        let origin = Vec2::new(s.screen_x, s.screen_y);
        let ground = || {
            shelves
                .iter()
                .position(|h| h.ground && h.key.origin == origin)
                .or_else(|| shelves.iter().position(|h| h.ground))
        };
        match s.place {
            PLACE_GROUND => ground().map(|i| (i, s.across)),
            PLACE_DOCK => shelves
                .iter()
                .position(|h| h.is_dock() && h.key.origin == origin)
                .map(|i| (i, s.across))
                .or_else(|| if age >= DOCK_WAIT { ground().map(|i| (i, s.across)) } else { None }),
            _ => {
                let same = |h: &Shelf| h.owner == s.owner && !h.ground && !h.is_dock();
                let exact = shelves.iter().position(|h| h.key.id == SurfaceId::Window(s.id) && same(h));
                let near = || {
                    shelves
                        .iter()
                        .enumerate()
                        .filter(|(_, h)| {
                            same(h) && (h.middle - s.middle).abs() < SAME_SPOT && (h.top - s.top).abs() < SAME_SPOT
                        })
                        .min_by(|a, b| {
                            let d = |h: &Shelf| (h.middle - s.middle).abs() + (h.top - s.top).abs();
                            d(a.1).total_cmp(&d(b.1))
                        })
                        .map(|(i, _)| i)
                };
                exact
                    .or_else(near)
                    .map(|i| (i, s.across + shelves[i].middle))
                    .or_else(|| if age >= WINDOW_WAIT { ground().map(|i| (i, s.across + s.middle)) } else { None })
            }
        }
    }

    fn try_restore(&mut self, now: f64, shelves: &[Shelf]) {
        let mut still = Vec::new();
        for s in std::mem::take(&mut self.waiting) {
            let Some((i, x)) = self.restore_target(&s, now, shelves) else {
                still.push(s);
                continue;
            };
            let sh = &shelves[i];
            let foot = Vec2::new(clamp_to(x, sh.min_x + EDGE, sh.max_x - EDGE), sh.top - 1.0);
            let id = self.take_id();
            let mut t = Tree::new(id, s.look.clone().unwrap_or_default(), s.seed, foot);
            t.support = Some(sh.key);
            t.across = foot.x - sh.middle;
            apply_saved_depths(&mut t.patches, &b64_decode(s.depths.as_deref().unwrap_or_default()), false);
            t.cap = f64::from(s.cap) / 255.0;
            self.trees.push(t);
        }
        self.waiting = still;
    }

    // ---- local toy train cargo (trees 5 interfaces, characters 5.3) ----
    // Cargo is an opaque JSON value (tree look, seed, snow depths) so that
    // `fp-characters` can queue it in `trainQueue` without depending on this
    // crate. `fp-world` moves values between the two.

    /// Trees that can be picked for a ride: `(id, foot)`; non-felling,
    /// non-doomed, not carried, Growth >= 1.
    pub fn standing(&self) -> Vec<(u64, Vec2)> {
        self.trees
            .iter()
            .filter(|t| !t.felling && !t.doomed && !t.carried && t.growth >= 1.0)
            .map(|t| (t.id, t.foot))
            .collect()
    }

    /// Carry exactly this tree to `point` (every other tree un-carried);
    /// `carry(None, None)` releases all (released trees fall).
    pub fn carry(&mut self, id: Option<u64>, point: Option<Vec2>) {
        let mut newly = None;
        for t in self.trees.iter_mut() {
            let on = Some(t.id) == id;
            if on && !t.carried {
                t.carried = true;
                newly = Some(t.id);
            } else if !on {
                t.carried = false;
            }
        }
        // The point is replaced every call: `None` means "stay where you are",
        // never "go to the previous tree's point".
        self.carry_point = point;
        if let Some(id) = newly {
            self.close_card_of(id);
            self.raise(id);
        }
    }

    /// Remove a tree and return its cargo value.
    pub fn take(&mut self, id: u64) -> Option<Value> {
        let t = self.trees.iter_mut().find(|t| t.id == id && !t.felling)?;
        t.felling = true;
        t.fall_way = 0.0;
        t.fell = 0.3;
        let cargo = serde_json::to_value(t.cargo()).ok();
        self.close_card_of(id);
        self.force_save = true;
        cargo
    }

    /// Remove a tree without felling animation.
    pub fn vanish(&mut self, id: u64) {
        if let Some(t) = self.tree_mut(id) {
            t.felling = true;
            t.fall_way = 0.0;
            t.fell = 1.0;
            t.growth = 0.0;
        }
        self.close_card_of(id);
        self.force_save = true;
    }

    fn tree_from_cargo(&mut self, c: &Cargo, foot: Vec2) -> Tree {
        let id = self.take_id();
        let mut t = Tree::new(id, c.look.clone(), c.seed, foot);
        apply_saved_depths(&mut t.patches, &b64_decode(&c.depths), true);
        t.cap = f64::from(c.cap) / 255.0;
        t
    }

    /// Create a carried Growth-1 tree from cargo at `point` (ignores the cap).
    pub fn board(&mut self, cargo: &Value, point: Vec2) -> Option<u64> {
        let c: Cargo = serde_json::from_value(cargo.clone()).ok()?;
        let mut t = self.tree_from_cargo(&c, point);
        t.carried = true;
        let id = t.id;
        self.carry_point = Some(point);
        self.trees.push(t);
        Some(id)
    }

    /// Plant cargo near `x` on the ground ledge of `screen` (global frame);
    /// spreads away from other trees, ignores the cap (trees 5 Plant).
    pub fn plant_cargo(&mut self, cargo: &Value, x: f64, screen: Frame) -> bool {
        let Ok(c) = serde_json::from_value::<Cargo>(cargo.clone()) else {
            return false;
        };
        let Some(sh) = self.shelves.iter().find(|s| s.ground && s.key.origin == screen.min()).cloned() else {
            return false;
        };
        let half = TreeShape::from_level(c.look.level).half;
        let bounds = (sh.min_x + half + EDGE, sh.max_x - half - EDGE);
        let mut spot = clamp_to(x, bounds.0, bounds.1);
        for _ in 0..6 {
            let Some(o) = self.trees.iter().find(|o| !o.felling && (o.foot.x - spot).abs() < half + o.shape.half * 0.6)
            else {
                break;
            };
            let gap = half + o.shape.half * 0.6 + 6.0;
            spot = clamp_to(if o.foot.x <= spot { o.foot.x + gap } else { o.foot.x - gap }, bounds.0, bounds.1);
        }
        let mut t = self.tree_from_cargo(&c, Vec2::new(spot, sh.top - 1.0));
        t.growth = 0.01;
        t.support = Some(sh.key);
        t.across = spot - sh.middle;
        self.trees.push(t);
        self.force_save = true;
        true
    }
}

#[cfg(test)]
mod grove_tests;
