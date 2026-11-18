//! Snow simulation for one monitor.
//!
//! Spec: snow-sim 2 (constants), 3.1-3.16 (time base, flakes, wind use,
//! landing, drifts, settle, grains/lumps/puffs, window updates, ground,
//! keep-clear, cursor, plough, bursts, fall, Snowfall wrapper), 4 (what to
//! put in the [`DrawList`]: drift columns, flakes, loose sprites; the WGSL
//! itself lives in `fp-overlay`), 9 (edge cases);
//! 3.17 decorative in-app snow positions ([`decor`]).
//!
//! Coordinates: screen-local points, Y up, ground at y = 0.
//! Determinism: all randomness from the owned [`Lcg64`]; time only from `dt`.

mod drift;
mod flake;

pub mod decor;

#[cfg(test)]
mod tests;

use drift::{Drift, Emit};
use flake::Flake;
use fp_core::falling::Falling;
use fp_core::feed::SnowFeed;
use fp_core::geom::Vec2;
use fp_core::gpu::{DrawList, Layer, Pass, PassKind, SnowFlake, SnowSprite};
use fp_core::rng::{Lcg64, Rng};
use fp_core::surface::{Surface, SurfaceId};
use fp_core::wind::{Blow, Wind, breeze_displacement, breeze_speed};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Flakes per million square points at share 1 (snow-sim 2).
pub const FLAKES_PER_MEGAPOINT: f64 = 380.0;
/// External share -> flake share multiplier (`Snowfall.Heavier`).
pub const HEAVIER: f64 = 1.25;

const GROUND_DEPTH: f64 = 40.0;
const LEDGE_DEPTH: f64 = 14.0;
const THAW_RATE: f64 = 0.034;
const THAW_SETTLE: f64 = 17.647_058;
const THIN_GROUND: f64 = 3.0;
const DEPOSITION: f64 = 1.1;
const GRAVITY: f64 = 2500.0;
const AIR_DRAG: f64 = 0.8;
const PUFF_DRAG: f64 = 3.2;
const MAX_GRAINS: usize = 3000;
const MAX_PUFFS: usize = 1500;
const FLAKE_CHECK: f64 = 0.05;
const LOOK_AHEAD: f64 = 0.1;
const REBASE_AFTER: f64 = 600.0;
const FLAKES_ADDED_AT_ONCE: usize = 20;
const EDGE_MARGIN: f64 = 30.0;
const WAKE_RADIUS: f64 = 80.0;
const WAKE_SHARE: f64 = 0.8;
const BLADE_WIDTH: f64 = 14.0;
const SWEEP_SPEED: f64 = 20.0;
const FASTEST_SWEEP: f64 = 800.0;
const WIND_LAG: f64 = 0.8;
const SETTLE_EVERY: f64 = 0.05;
const SMALLEST_LUMP: f64 = 6.0;
const LUMP_AREA: f64 = 90.0;
const LARGEST_LUMP: f64 = 11.0;
const CRUMB_SHARE: f64 = 0.18;
const MAX_AWAY: usize = 100;

/// Construction parameters.
#[derive(Clone, Debug)]
pub struct SnowConfig {
    pub width: f64,
    pub height: f64,
    pub reduce_motion: bool,
    /// Global min corner of this monitor (feed key).
    pub origin: Vec2,
    pub seed: u64,
}

#[derive(Clone, Copy, Debug)]
struct Grain {
    pos: Vec2,
    vel: Vec2,
    area: f64,
    size: f64,
    lump: f64,
}

#[derive(Clone, Copy, Debug)]
struct Puff {
    pos: Vec2,
    vel: Vec2,
    size: f64,
    alpha: f64,
    life: f64,
    age: f64,
}

/// One monitor's snow (`SnowField` + `Snowfall`).
pub struct SnowField {
    cfg: SnowConfig,
    feed: Option<Arc<SnowFeed>>,
    rng: Lcg64,
    clock: f64,
    epoch: f64,
    /// Seconds stepped since construction (scene time for the shaders).
    elapsed: f64,
    last_settle: f64,
    last_check: f64,
    share: f64,
    wind: Wind,
    blow: Blow,
    /// Wall-clock time base of the changing wind (`None` = field clock).
    wind_t: Option<f64>,
    flakes: Vec<Flake>,
    grains: Vec<Grain>,
    puffs: Vec<Puff>,
    ground: Drift,
    ledges: Vec<(SurfaceId, Drift)>,
    away: BTreeMap<SurfaceId, Drift>,
    ground_shown: bool,
    keep_clear: bool,
    split: bool,
    cursor_prev: Option<Vec2>,
}

impl SnowField {
    /// `feed` = None for previews (no snowballs, no plough).
    pub fn new(cfg: SnowConfig, feed: Option<Arc<SnowFeed>>) -> Self {
        let mut rng = Lcg64::new(cfg.seed);
        let clock = rng.range(0.0, 1000.0);
        let ground = Drift::new(Vec2::ZERO, cfg.width, GROUND_DEPTH, 0.0, false, &mut rng);
        let mut f = Self {
            cfg,
            feed,
            rng,
            clock,
            epoch: clock,
            elapsed: 0.0,
            last_settle: clock,
            last_check: clock,
            share: 1.0,
            wind: Wind::None,
            blow: Blow::new(1.0),
            wind_t: None,
            flakes: Vec::new(),
            grains: Vec::new(),
            puffs: Vec::new(),
            ground,
            ledges: Vec::new(),
            away: BTreeMap::new(),
            ground_shown: true,
            keep_clear: false,
            split: false,
            cursor_prev: None,
        };
        let spread = 0.8 * f.cfg.height;
        for _ in 0..f.target() {
            f.add_flake(spread);
        }
        f
    }

    /// Current ground depth (reported to the feed every settle).
    pub fn ground_depth(&self) -> f64 {
        let d = self.ledges.iter().find(|(id, _)| *id == SurfaceId::Dock).map_or(&self.ground, |(_, d)| d);
        let tall: Vec<f64> = d.heights.iter().copied().filter(|h| *h > 0.5).collect();
        if tall.is_empty() { 0.0 } else { tall.iter().sum::<f64>() / tall.len() as f64 }
    }

    fn now(&self) -> f64 {
        self.clock - self.epoch
    }

    fn span(&self) -> f64 {
        self.cfg.width + 2.0 * EDGE_MARGIN
    }

    fn target(&self) -> usize {
        (FLAKES_PER_MEGAPOINT * self.share * HEAVIER * self.cfg.width * self.cfg.height / 1e6) as usize
    }

    fn wind_now(&self) -> f64 {
        breeze_displacement(self.clock - WIND_LAG) + self.blow.carried
    }

    fn breeze_now(&self) -> f64 {
        breeze_speed(self.clock - WIND_LAG) + self.blow.speed
    }

    fn drift(&self, id: SurfaceId) -> Option<&Drift> {
        if id == SurfaceId::Ground {
            return Some(&self.ground);
        }
        self.ledges.iter().find(|(i, _)| *i == id).map(|(_, d)| d)
    }

    fn drift_mut(&mut self, id: SurfaceId) -> Option<&mut Drift> {
        if id == SurfaceId::Ground {
            return Some(&mut self.ground);
        }
        self.ledges.iter_mut().find(|(i, _)| *i == id).map(|(_, d)| d)
    }

    fn add_flake(&mut self, spread: f64) {
        let (w, h, now, wind, reduce) =
            (self.cfg.width, self.cfg.height, self.now(), self.wind_now(), self.cfg.reduce_motion);
        let f = Flake::spawn(&mut self.rng, w, h, spread, now, wind, reduce);
        self.flakes.push(f);
    }

    // ---- step ----

    fn rebase(&mut self) {
        while self.now() >= REBASE_AFTER {
            self.epoch += REBASE_AFTER;
            let carried = self.blow.carried;
            for f in &mut self.flakes {
                f.rebase(REBASE_AFTER, carried);
            }
            self.blow.carried = 0.0;
        }
    }

    fn apply(&mut self, emits: Vec<Emit>) {
        for e in emits {
            match e {
                Emit::Release { area, pos, vel, spread, scatter } => self.release(area, pos, vel, spread, scatter),
                Emit::Lump { area, pos, vel } => self.throw_lump(area, pos, vel),
                Emit::Fall { area, pos, dir, vel } => self.fall(area, pos, dir, vel),
            }
        }
    }

    /// Small falling pieces (snow-sim 3.8 Release).
    fn release(&mut self, area: f64, pos: Vec2, vel: Vec2, spread: Vec2, scatter: f64) {
        if area.is_nan() || area <= 0.05 {
            return;
        }
        let room = MAX_GRAINS.saturating_sub(self.grains.len());
        if room == 0 {
            self.ground.deposit(area, pos.x, true);
            return;
        }
        let n = room.min(((area / 3.0).floor() as usize).clamp(1, 6));
        let weights: Vec<f64> = (0..n).map(|_| self.rng.range(0.2, 1.0)).collect();
        let total: f64 = weights.iter().sum();
        for w in weights {
            let a = area * w / total;
            let p =
                Vec2::new(pos.x + self.rng.range(-1.0, 1.0) * spread.x, pos.y + self.rng.range(0.0, 1.0) * spread.y);
            let v = vel * (1.0 + self.rng.range(-scatter, scatter))
                + Vec2::new(self.rng.range(-12.0, 12.0), self.rng.range(-10.0, 8.0));
            self.grains.push(Grain { pos: p, vel: v, area: a, size: (a.sqrt() * 1.05).clamp(1.1, 5.0), lump: 0.0 });
        }
    }

    fn add_lump(&mut self, area: f64, pos: Vec2, vel: Vec2) {
        if self.grains.len() >= MAX_GRAINS {
            self.ground.deposit(area, pos.x, true);
            return;
        }
        let lump = self.rng.range(0.05, 1.0);
        self.grains.push(Grain { pos, vel, area, size: (area.sqrt() * 1.05).min(LARGEST_LUMP), lump });
    }

    /// Chunks of snow thrown as lumps (snow-sim 3.8 ThrowLump).
    fn throw_lump(&mut self, area: f64, pos: Vec2, vel: Vec2) {
        if area.is_nan() || area < SMALLEST_LUMP {
            self.release(area, pos, vel, Vec2::new(2.0, 2.0), 0.25);
            return;
        }
        let n = (area / LUMP_AREA).floor() as usize + 1;
        for _ in 0..n {
            let p = pos + Vec2::new(self.rng.range(-2.0, 2.0), self.rng.range(-1.0, 1.0));
            let v = vel * (1.0 + self.rng.range(-0.2, 0.2))
                + Vec2::new(self.rng.range(-25.0, 25.0), self.rng.range(-80.0, 0.0));
            self.add_lump(area / n as f64, p, v);
        }
    }

    /// Snow sliding off an edge (snow-sim 3.15).
    fn fall(&mut self, area: f64, pos: Vec2, dir: f64, vel: Vec2) {
        let pieces = self.rng.inclusive(1, 3) as usize;
        let weights: Vec<f64> = (0..pieces).map(|_| self.rng.range(0.2, 1.0)).collect();
        let total: f64 = weights.iter().sum();
        for w in weights {
            let p = pos + Vec2::new(dir * self.rng.range(0.0, 3.0), self.rng.range(-2.0, 2.0));
            let v = vel + Vec2::new(dir * self.rng.range(8.0, 45.0), self.rng.range(-25.0, 0.0));
            self.throw_lump(area * w / total, p, v);
        }
    }

    fn add_puff(&mut self, pos: Vec2, vel: Vec2, size: f64, alpha: f64) {
        let life = self.rng.range(0.45, 0.9);
        self.puffs.push(Puff { pos, vel, size, alpha, life, age: 0.0 });
    }

    /// Which drift a falling point lands on (snow-sim 3.5 Landing).
    fn landing(&self, x: f64, above: f64, below: f64) -> Option<SurfaceId> {
        let mut best: Option<(SurfaceId, f64)> = None;
        for (id, d) in &self.ledges {
            if let Some(i) = d.index(x) {
                let s = d.surface(i);
                let higher = best.is_none_or(|(_, b)| s > b);
                if d.visible[i] && above >= d.floor(i) && below <= s && higher {
                    best = Some((*id, s));
                }
            }
        }
        if let Some((id, _)) = best {
            return Some(id);
        }
        let i = self.ground.clamped_index(x);
        if self.ground_shown && below <= self.ground.surface(i) { Some(SurfaceId::Ground) } else { None }
    }

    fn renew(&mut self, i: usize) {
        self.flakes.swap_remove(i);
        if self.flakes.len() < self.target() {
            self.add_flake(40.0);
        }
    }

    fn check_flakes(&mut self) {
        let now = self.now();
        let wind = self.wind_now();
        let ahead =
            breeze_displacement(self.clock + LOOK_AHEAD - WIND_LAG) + self.blow.carried + self.blow.speed * LOOK_AHEAD;
        let span = self.span();
        let keep_clear = self.keep_clear;
        let mut i = self.flakes.len();
        while i > 0 {
            i -= 1;
            let f = self.flakes[i];
            if let Some((id, lx)) = f.landing {
                if now >= f.gone {
                    if let Some(d) = self.drift_mut(id) {
                        let col = d.clamped_index(lx);
                        if !keep_clear || (id == SurfaceId::Ground && d.heights[col] < THIN_GROUND) {
                            let area = f.size * f.size * DEPOSITION * d.fill(col);
                            d.deposit(area, lx, true);
                        }
                    }
                    self.renew(i);
                }
                continue;
            }
            let pos = f.position(now, wind, span);
            let fell_off = pos.y < -2.0 * f.size;
            if !f.settles {
                if fell_off {
                    self.renew(i);
                }
                continue;
            }
            let pos2 = f.position(now + LOOK_AHEAD, ahead, span);
            match self.landing(pos2.x, pos.y, pos2.y) {
                Some(id) => {
                    let d = self.drift(id).unwrap_or(&self.ground);
                    let surface_y = d.surface(d.clamped_index(pos2.x));
                    let frac =
                        if pos.y > pos2.y { ((pos.y - surface_y) / (pos.y - pos2.y)).clamp(0.0, 1.0) } else { 0.0 };
                    let fl = &mut self.flakes[i];
                    fl.gone = now + LOOK_AHEAD * frac;
                    fl.landing = Some((id, pos2.x));
                }
                None if fell_off => self.renew(i),
                None => {}
            }
        }
        let add = self.target().saturating_sub(self.flakes.len()).min(FLAKES_ADDED_AT_ONCE);
        let spread = 0.3 * self.cfg.height;
        for _ in 0..add {
            self.add_flake(spread);
        }
    }

    fn advance_grains(&mut self, dt: f64) {
        let drag = (-AIR_DRAG * dt).exp();
        let push = self.blow.speed * 0.6 * (1.0 - drag);
        let width = self.cfg.width;
        let mut keep = Vec::with_capacity(self.grains.len());
        let mut landed = Vec::new();
        for mut g in std::mem::take(&mut self.grains) {
            let prev = g.pos.y;
            g.vel.y -= GRAVITY * dt;
            g.vel = g.vel * drag;
            g.vel.x += push;
            g.pos = g.pos + g.vel * dt;
            if g.pos.x < -EDGE_MARGIN || g.pos.x > width + EDGE_MARGIN || g.pos.y < -20.0 {
                continue;
            }
            if g.vel.y <= 0.0
                && let Some(id) = self.landing(g.pos.x, prev, g.pos.y)
            {
                landed.push((g, id));
                continue;
            }
            keep.push(g);
        }
        self.grains = keep;
        for (g, id) in landed {
            self.land(g, id);
        }
    }

    /// Release what a drift could not hold (not on the ground).
    fn re_release(&mut self, overflow: f64, pos: Vec2, vx: f64, id: SurfaceId) {
        if id != SurfaceId::Ground {
            self.release(overflow, pos, Vec2::new(vx, -20.0), Vec2::new(2.0, 2.0), 0.25);
        }
    }

    fn land(&mut self, g: Grain, id: SurfaceId) {
        let Some(d) = self.drift_mut(id) else { return };
        if g.lump <= 0.0 {
            let over = d.deposit(g.area, g.pos.x, true);
            self.re_release(over, Vec2::new(g.pos.x, g.pos.y - 3.0), g.vel.x * 0.5, id);
            return;
        }
        let size = g.size;
        let x = g.pos.x;
        let speed = (-g.vel.y).max(0.0);
        let k = (speed / 900.0).clamp(0.35, 1.3);
        let crumb = CRUMB_SHARE * g.area;
        let part = (g.area - crumb) / 3.0;
        let mut over = 0.0;
        for dx in [-0.3 * size, 0.0, 0.3 * size] {
            over += d.deposit(part, x + dx, true);
        }
        let surface_y = d.index(x).map_or(g.pos.y, |i| d.surface(i));
        self.re_release(over, Vec2::new(x, g.pos.y - 3.0), g.vel.x * 0.3, id);
        let up = speed * self.rng.range(0.08, 0.16);
        self.release(
            crumb,
            Vec2::new(x, surface_y + 1.0),
            Vec2::new(g.vel.x * 0.3, up),
            Vec2::new(0.4 * size, 1.0),
            0.6,
        );
        let count = MAX_PUFFS.saturating_sub(self.puffs.len()).min((2.0 + 0.6 * size * k) as usize);
        for _ in 0..count {
            let pos = Vec2::new(x + self.rng.range(-0.5, 0.5) * size, surface_y + self.rng.range(0.0, 0.4 * size));
            let angle = self.rng.range(0.15, 2.9916);
            let sp = self.rng.range(25.0, 110.0) * k;
            let vel = Vec2::new(angle.cos() * sp * 1.4 + 0.2 * g.vel.x, angle.sin() * sp * 0.6);
            let psize = self.rng.range(0.3, 0.6) * size + 1.0;
            let alpha = self.rng.range(0.1, 0.22);
            self.add_puff(pos, vel, psize, alpha);
        }
    }

    fn advance_puffs(&mut self, dt: f64) {
        let drag = (-PUFF_DRAG * dt).exp();
        let breeze = 0.4 * self.breeze_now();
        self.puffs.retain_mut(|p| {
            p.vel = p.vel * drag;
            p.pos = p.pos + (p.vel + Vec2::new(breeze, 0.0)) * dt;
            p.age += dt;
            p.age < p.life
        });
    }

    fn sweep(&mut self, dt: f64, cursor: Option<Vec2>) {
        let prev = std::mem::replace(&mut self.cursor_prev, cursor);
        let (Some(c), Some(p)) = (cursor, prev) else { return };
        if dt <= 0.0 {
            return;
        }
        let v = (c - p) * (1.0 / dt);
        if v.len() <= SWEEP_SPEED {
            return;
        }
        let (now, wind, span) = (self.now(), self.wind_now(), self.span());
        for f in &mut self.flakes {
            let d = f.position(now, wind, span).dist(c);
            if d < WAKE_RADIUS {
                let k = (1.0 - d / WAKE_RADIUS) * WAKE_SHARE * dt;
                f.x += v.x * k;
                f.top += v.y * k;
                f.landing = None;
                f.gone = f64::from(f32::MAX);
            }
        }
        if self.ground_shown {
            self.carve(SurfaceId::Ground, c, v);
        }
        for k in 0..self.ledges.len() {
            let id = self.ledges[k].0;
            self.carve(id, c, v);
        }
    }

    fn carve(&mut self, id: SurfaceId, c: Vec2, v: Vec2) {
        let Some(d) = self.drift_mut(id) else { return };
        let removed = d.cut(c.x, c.y);
        if removed <= 0.3 {
            return;
        }
        let origin = self.cfg.origin;
        let absorbed = self.feed.as_ref().map_or(0.0, |f| f.gather(removed, c, v, origin)).clamp(0.0, removed);
        let rest = removed - absorbed;
        let dir = if v.x.abs() > 0.5 * v.y.abs() { v.x.signum() } else { 0.0 };
        let Some(d) = self.drift_mut(id) else { return };
        if dir == 0.0 {
            d.deposit(rest * 0.3, c.x - 14.0, false);
            d.deposit(rest * 0.3, c.x + 14.0, false);
        } else {
            d.deposit(rest * 0.6, c.x + dir * 14.0, false);
        }
        let vel = Vec2::new(0.35 * v.x, 60.0 + 0.15 * v.len().min(FASTEST_SWEEP));
        self.release(rest * 0.4, Vec2::new(c.x, c.y + 3.0), vel, Vec2::new(6.0, 3.0), 0.6);
    }

    fn plough(&mut self) {
        let Some(blade) = self.feed.as_ref().and_then(|f| f.blade(self.cfg.origin)) else { return };
        if !self.ground_shown {
            return;
        }
        let id = if blade.on_dock { SurfaceId::Dock } else { SurfaceId::Ground };
        let Some(d) = self.drift_mut(id) else { return };
        let back = blade.front - blade.way * BLADE_WIDTH;
        let removed = d.plough_cut(blade.front.min(back), blade.front.max(back), blade.leave);
        if removed <= 0.3 {
            return;
        }
        let x2 = blade.front + blade.way * 6.0;
        let over = d.deposit(0.6 * removed, x2, true);
        let surface_y = d.surface(d.clamped_index(blade.front));
        self.release(over, Vec2::new(x2, surface_y), Vec2::new(blade.way * blade.speed, 0.0), Vec2::new(4.0, 4.0), 0.4);
        let up = self.rng.range(220.0, 420.0);
        let vel = Vec2::new(blade.way * (1.3 * blade.speed + 60.0), up);
        self.release(0.4 * removed, Vec2::new(blade.front, surface_y + 2.0), vel, Vec2::new(4.0, 6.0), 0.5);
    }

    fn bursts(&mut self) {
        let Some(feed) = self.feed.clone() else { return };
        for b in feed.bursts(self.cfg.origin) {
            self.shatter(b.point, b.area, b.velocity);
        }
    }

    /// Snowball impact (snow-sim 3.14).
    fn shatter(&mut self, point: Vec2, area: f64, burst_vel: Vec2) {
        if area.is_nan() || area <= 0.0 {
            return;
        }
        let radius = 0.4 * area.sqrt();
        let n = ((area / LUMP_AREA) as usize).clamp(5, 30);
        for _ in 0..n {
            let (dir, off) = self.random_dir_offset(radius);
            let vel = burst_vel * 0.25 + dir * self.rng.range(120.0, 420.0) + Vec2::new(0.0, 160.0);
            self.add_lump(area / n as f64, point + off, vel);
        }
        let puffs = MAX_PUFFS.saturating_sub(self.puffs.len()).min(24);
        for _ in 0..puffs {
            let (dir, off) = self.random_dir_offset(radius);
            let vel = dir * self.rng.range(40.0, 200.0);
            let size = self.rng.range(0.4, 0.9) * radius + 3.0;
            let alpha = self.rng.range(0.12, 0.28);
            self.add_puff(point + off, vel, size, alpha);
        }
    }

    fn random_dir_offset(&mut self, radius: f64) -> (Vec2, Vec2) {
        let a = self.rng.range(0.0, std::f64::consts::TAU);
        let dir = Vec2::new(a.cos(), a.sin());
        (dir, dir * (self.rng.unit().sqrt() * radius))
    }

    fn settle(&mut self, dt: f64) {
        let (gf, lf, settle) = if self.keep_clear { (3.0, 0.0, 8.0) } else { (8.0, 4.0, THAW_SETTLE) };
        let mut ems = Vec::new();
        self.ground.settle(dt, gf, settle, THAW_RATE, &mut self.rng, &mut ems);
        for (_, d) in &mut self.ledges {
            d.settle(dt, lf, settle, THAW_RATE, &mut self.rng, &mut ems);
        }
        self.apply(ems);
        if let Some(feed) = &self.feed {
            feed.report_depth(self.cfg.origin, self.ground_depth());
        }
    }

    fn draw_drift(d: &Drift, out: &mut DrawList) {
        if d.is_empty() {
            return;
        }
        let b = d.base();
        let uniforms = vec![
            b.x as f32,
            b.y as f32,
            2.0,
            d.count() as f32,
            (-d.bed_depth - 1.0) as f32,
            (d.top + 2.0) as f32,
            d.occluders.len() as f32,
            0.0,
        ];
        let columns: Vec<u8> = d.shape().iter().flat_map(|f| f.to_le_bytes()).collect();
        let mut occ: Vec<u8> =
            d.occluders.iter().flat_map(|r| [r.x0, r.y0, r.x1, r.y1]).flat_map(|f| (f as f32).to_le_bytes()).collect();
        if occ.is_empty() {
            occ = vec![0; 16];
        }
        out.push(Pass { kind: PassKind::SnowDrift, uniforms, buffers: vec![columns, occ], vertices: 4, instances: 1 });
    }
}

impl Falling for SnowField {
    fn set_share(&mut self, share: f64) {
        self.share = if share.is_finite() { share.max(0.0) } else { 0.0 };
    }

    fn share(&self) -> f64 {
        self.share
    }

    fn set_wind_time(&mut self, t: f64) {
        if t.is_finite() {
            self.wind_t = Some(t);
        }
    }

    fn set_wind(&mut self, wind: Wind) {
        self.wind = wind;
    }

    fn step(&mut self, dt: f64, cursor: Option<Vec2>) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.clock += dt;
        self.elapsed += dt;
        self.rebase();
        self.blow.step(dt, self.clock, self.wind, self.wind_t.unwrap_or(self.clock));
        self.sweep(dt, cursor);
        self.plough();
        self.bursts();
        if dt > 0.0 {
            let mut ems = Vec::new();
            for (_, d) in &mut self.ledges {
                d.carry(dt, &mut self.rng, &mut ems);
            }
            self.apply(ems);
        }
        if self.clock - self.last_check >= FLAKE_CHECK {
            self.last_check = self.clock;
            self.check_flakes();
        }
        self.advance_grains(dt);
        self.advance_puffs(dt);
        let since = self.clock - self.last_settle;
        if since >= SETTLE_EVERY {
            self.last_settle = self.clock;
            self.settle(since.min(0.25));
        }
    }

    fn update(&mut self, surfaces: &[Surface], interval: f64, to_other_space: bool) {
        let interval = interval.clamp(0.0, 0.25);
        let mut ems = Vec::new();
        let mut k = 0;
        while k < self.ledges.len() {
            let id = self.ledges[k].0;
            if surfaces.iter().any(|s| s.id == id) {
                k += 1;
                continue;
            }
            let (_, mut d) = self.ledges.remove(k);
            if to_other_space && id != SurfaceId::Dock {
                d.size = None;
                self.away.insert(id, d);
            } else if !d.is_empty() {
                let v = d.velocity;
                d.break_up(1.0, v, &mut self.rng, &mut ems);
            }
        }
        for s in surfaces.iter().filter(|s| s.id != SurfaceId::Ground) {
            let origin = Vec2::new(s.frame.min_x(), s.frame.max_y());
            let (k, fresh) = match self.ledges.iter().position(|(i, _)| *i == s.id) {
                Some(k) => (k, false),
                None => {
                    let (d, fresh) = match self.away.remove(&s.id) {
                        Some(d) => (d, false),
                        None => {
                            (Drift::new(origin, s.frame.w, LEDGE_DEPTH, s.corner_radius, true, &mut self.rng), true)
                        }
                    };
                    self.ledges.push((s.id, d));
                    (self.ledges.len() - 1, fresh)
                }
            };
            let clock = self.clock;
            let screen_w = self.cfg.width;
            let d = &mut self.ledges[k].1;
            let resized = d.size.is_some_and(|(w, h)| w != s.frame.w || h != s.frame.h);
            d.size = Some((s.frame.w, s.frame.h));
            d.set_corner(s.corner_radius);
            d.occluders = s.blockers.iter().take(16).copied().collect();
            if !fresh {
                follow(d, origin, s.frame.w, resized, interval, clock, &mut self.rng, &mut ems);
            }
            d.recompute_visible(&s.blockers, screen_w);
        }
        while self.away.len() > MAX_AWAY {
            self.away.pop_first();
        }
        self.apply(ems);
    }

    fn draw(&self, layer: Layer, out: &mut DrawList) {
        out.scene = [self.cfg.width as f32, self.cfg.height as f32, self.elapsed as f32, 1.0];
        let main = layer == Layer::Main;
        if main && self.ground_shown {
            Self::draw_drift(&self.ground, out);
        }
        if self.split != main {
            for (_, d) in &self.ledges {
                Self::draw_drift(d, out);
            }
        }
        if !main {
            return;
        }
        let (now, wind, span) = (self.now(), self.wind_now(), self.span());
        let flakes: Vec<SnowFlake> = self.flakes.iter().map(|f| f.to_gpu()).collect();
        let uniforms = vec![now as f32, wind as f32, self.breeze_now() as f32, span as f32, EDGE_MARGIN as f32];
        out.instanced(PassKind::SnowFlakes, uniforms, &flakes);
        let mut sprites: Vec<SnowSprite> = self
            .grains
            .iter()
            .map(|g| SnowSprite {
                pos: [g.pos.x as f32, g.pos.y as f32],
                vel: [g.vel.x as f32, g.vel.y as f32],
                size: g.size as f32,
                alpha: 0.95,
                soft: if g.lump > 0.0 { 0.25 } else { 0.5 },
                lump: g.lump as f32,
            })
            .collect();
        sprites.extend(self.puffs.iter().map(|p| SnowSprite {
            pos: [p.pos.x as f32, p.pos.y as f32],
            vel: [0.0, 0.0],
            size: p.size as f32,
            alpha: (p.alpha * (1.0 - p.age / p.life)) as f32,
            soft: 1.0,
            lump: 0.0,
        }));
        out.instanced(PassKind::SnowLoose, Vec::new(), &sprites);
    }

    fn show_ground(&mut self, on: bool) {
        self.ground_shown = on;
    }

    fn keep_clear(&mut self, on: bool) {
        self.keep_clear = on;
    }

    fn draws_above(&self) -> bool {
        true
    }

    fn set_split(&mut self, on: bool) {
        self.split = on;
    }

    fn has_above(&self) -> bool {
        self.ledges.iter().any(|(_, d)| !d.is_empty())
    }
}

/// Make a known drift follow its window (snow-sim 3.9 Follow).
#[allow(clippy::too_many_arguments)]
fn follow(
    d: &mut Drift,
    origin: Vec2,
    width: f64,
    resized: bool,
    interval: f64,
    clock: f64,
    rng: &mut Lcg64,
    out: &mut Vec<Emit>,
) {
    let old = d.origin;
    let keep_left = (origin.x - old.x).abs() < 1.0;
    let cut = d.resize(width, keep_left);
    if cut > 0.0 {
        let (x, dir) = if keep_left { (origin.x + width, 1.0) } else { (origin.x, -1.0) };
        out.push(Emit::Fall { area: cut, pos: Vec2::new(x, origin.y), dir, vel: Vec2::ZERO });
    }
    if origin == old {
        return;
    }
    let delta = origin - old;
    let step = interval.max(1.0 / 120.0);
    let was_still = clock - d.moved_at > 0.3 && interval > 0.05;
    d.moved_at = clock;
    d.origin = origin;
    if resized {
        d.track = origin;
        d.velocity = Vec2::ZERO;
    } else if delta.len() / step > 6000.0 {
        d.break_up(1.0, Vec2::ZERO, rng, out);
        d.offset = Vec2::ZERO;
        d.offset_vel = Vec2::ZERO;
        d.velocity = Vec2::ZERO;
        d.track = origin;
    } else if was_still {
        d.velocity = delta * (1.0 / step);
        d.track = origin - d.velocity * 0.06;
    }
}
