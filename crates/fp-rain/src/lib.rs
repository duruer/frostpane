//! Rain and thunderstorm for one monitor.
//!
//! Spec: other-weather 1 (conventions), 1.1 (Blow, strength 3), 2.1.1-2.1.9
//! (constants, ambient wind, step order, spawning, motion, landing, splash/
//! drips/pour/fling, lightning, render data, edge cases); weather-sources 2.1
//! (strike timing). Thunder audio does not exist.
//!
//! Output passes: [`fp_core::gpu::PassKind::RainFlash`], `RainRipples`,
//! `RainStrokes` in that order.
//!
//! Coordinates are screen-local points, Y up. Deviations from the literal
//! spec, all harmless: the landing test runs before the horizontal wrap (no
//! screen-spanning segment), and among ledges that pass the inset/blocker
//! checks the highest top wins (a blocked higher ledge lets the drop fall on).

use std::collections::BTreeMap;

use fp_core::falling::Falling;
use fp_core::geom::{Frame, Vec2};
use fp_core::gpu::{DrawList, Layer, Pass, PassKind, RainRipple, RainStroke};
use fp_core::rng::{Lcg64, Rng};
use fp_core::surface::{Surface, SurfaceId, blocked};
use fp_core::wind::{Blow, Wind};

const DROPS_PER_MEGAPOINT: f64 = 300.0;
const GRAVITY: f64 = 2200.0;
const FASTEST_DRIP: f64 = 1300.0;
const MAX_DROPLETS: usize = 2500;
const MAX_RIPPLES: usize = 600;
const WRAP_MARGIN: f64 = 60.0;
const WATER_CAP: f64 = 40.0;
const FLING_SPEED: f64 = 450.0;
const JUMP_SPEED: f64 = 6000.0;
const POINTER_OFFSET: Vec2 = Vec2::new(5.0, -8.0);
const POINTER_RADIUS: f64 = 9.0;
const WAKE_RADIUS: f64 = 80.0;
const WAKE_FACTOR: f64 = 0.5;
const GROUND_Y: f64 = 3.0;
const SPAWN_BURST: usize = 30;
const FLASH_MIN: f64 = 0.002;
const BOLT_POINTS: f64 = 65.0;
const BRANCH_POINTS: f64 = 17.0;

#[derive(Clone, Debug)]
struct Drop {
    pos: Vec2,
    vel: Vec2,
    width: f64,
    alpha: f64,
    drift: f64,
    lands: bool,
    dripping: bool,
}

#[derive(Clone, Debug)]
struct Droplet {
    pos: Vec2,
    vel: Vec2,
    width: f64,
    alpha: f64,
    floor: f64,
    glow: f32,
    age: f64,
}

#[derive(Clone, Debug)]
struct Ripple {
    center: Vec2,
    reach: f64,
    alpha: f64,
    life: f64,
    age: f64,
}

#[derive(Clone, Debug)]
struct Seg {
    a: Vec2,
    b: Vec2,
    width: f64,
    alpha: f64,
}

#[derive(Clone, Debug)]
struct Strike {
    pulses: Vec<f64>,
    age: f64,
    life: f64,
    strength: f64,
    segs: Vec<Seg>,
}

impl Strike {
    fn brightness(&self) -> f64 {
        let sum: f64 = self
            .pulses
            .iter()
            .enumerate()
            .filter(|(_, p)| self.age >= **p)
            .map(|(i, p)| (-(self.age - p) * 11.0).exp() * if i == 0 { 1.0 } else { 0.75 })
            .sum();
        sum.min(1.0)
    }
}

#[derive(Clone, Debug)]
struct Ledge {
    frame: Frame,
    water: f64,
    drip_at: f64,
}

struct Hit {
    pos: Vec2,
    ledge: Option<SurfaceId>,
    pointer: bool,
}

/// Ambient left-leaning wind (2.1.2).
fn ambient_wind(t: f64) -> f64 {
    -70.0
        + 45.0 * (0.05 * t).sin()
        + 25.0 * (0.17 * t + 1.3).sin()
        + 12.0 * (0.9 * t + 0.4).sin() * (0.11 * t + 2.2).sin().max(0.0)
}

/// Midpoint displacement; endpoints fixed, `2^levels + 1` points.
fn jagged(rng: &mut Lcg64, a: Vec2, b: Vec2, levels: u32, roughness: f64) -> Vec<Vec2> {
    let mut pts = vec![a, b];
    for _ in 0..levels {
        let mut next = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            let d = w[1] - w[0];
            // (-dy, dx) is the unit perpendicular times the segment length.
            let off = Vec2::new(-d.y, d.x) * (rng.range(-1.0, 1.0) * roughness);
            next.push(w[0]);
            next.push((w[0] + w[1]) * 0.5 + off);
        }
        next.push(b);
        pts = next;
    }
    pts
}

/// x where the segment from -> to crosses height `y` (from.y >= y >= to.y).
fn cross_x(from: Vec2, to: Vec2, y: f64) -> f64 {
    let dy = from.y - to.y;
    if dy > 0.0 { from.x + (to.x - from.x) * ((from.y - y) / dy) } else { to.x }
}

/// Rain and thunderstorm simulation for one monitor.
pub struct RainField {
    width: f64,
    height: f64,
    slow: f64,
    reduce: bool,
    rng: Lcg64,
    share: f64,
    wind: Wind,
    blow: Blow,
    /// Wall-clock time base of the changing wind (`None` = field clock).
    wind_t: Option<f64>,
    clock: f64,
    last_cursor: Option<Vec2>,
    drops: Vec<Drop>,
    droplets: Vec<Droplet>,
    ripples: Vec<Ripple>,
    strikes: Vec<Strike>,
    ledges: BTreeMap<SurfaceId, Ledge>,
    surfaces: Vec<Surface>,
    lightning: bool,
    next_strike: f64,
}

impl RainField {
    pub fn new(width: f64, height: f64, reduce_motion: bool, seed: u64) -> Self {
        let mut rng = Lcg64::new(seed);
        let clock = rng.range(0.0, 1000.0);
        let next_strike = clock + rng.range(4.0, 10.0);
        let mut f = Self {
            width,
            height,
            slow: if reduce_motion { 0.5 } else { 1.0 },
            reduce: reduce_motion,
            rng,
            share: 1.0,
            wind: Wind::None,
            blow: Blow::new(3.0),
            wind_t: None,
            clock,
            last_cursor: None,
            drops: Vec::new(),
            droplets: Vec::new(),
            ripples: Vec::new(),
            strikes: Vec::new(),
            ledges: BTreeMap::new(),
            surfaces: Vec::new(),
            lightning: true,
            next_strike,
        };
        for _ in 0..f.target() {
            let y = f.rng.unit() * 1.2 * height;
            let d = f.spawn_drop(y);
            f.drops.push(d);
        }
        f
    }

    /// Current screen flash strength (max over strikes).
    pub fn flash(&self) -> f64 {
        self.strikes.iter().map(|s| s.brightness() * s.strength).fold(0.0, f64::max)
    }

    fn target(&self) -> usize {
        (DROPS_PER_MEGAPOINT * self.share * self.width * self.height / 1e6) as usize
    }

    fn wind_now(&self) -> f64 {
        ambient_wind(self.clock) * self.slow + self.blow.speed
    }

    fn spawn_drop(&mut self, y: f64) -> Drop {
        let far = self.rng.unit() < 0.03;
        let n = if far { 1.0 } else { self.rng.unit().powf(1.4) };
        let speed = if far { self.rng.range(1500.0, 1800.0) } else { 650.0 + 650.0 * n + self.rng.range(0.0, 120.0) }
            * self.slow;
        let drift = if far { 1.2 } else { 0.4 + 0.6 * n };
        let x = self.rng.range(-WRAP_MARGIN, self.width + WRAP_MARGIN);
        let width = if far { self.rng.range(2.2, 3.0) } else { 0.7 + 0.9 * n };
        let alpha = if far { 0.16 } else { 0.16 + 0.36 * n };
        Drop {
            pos: Vec2::new(x, y),
            vel: Vec2::new(self.wind_now() * drift, -speed),
            width,
            alpha,
            drift,
            lands: !far,
            dripping: false,
        }
    }

    fn land(&self, from: Vec2, to: Vec2, collider: Option<Vec2>) -> Option<Hit> {
        if let Some(c) = collider {
            let dx = to.x - c.x;
            if dx.abs() < POINTER_RADIUS && from.y >= c.y && to.y <= c.y + POINTER_RADIUS {
                let top = c.y + (POINTER_RADIUS * POINTER_RADIUS - dx * dx).sqrt();
                if from.y >= top && to.y <= top {
                    return Some(Hit { pos: Vec2::new(to.x, top), ledge: None, pointer: true });
                }
            }
        }
        let mut best: Option<(f64, f64, SurfaceId)> = None;
        for s in &self.surfaces {
            let t = s.frame.max_y();
            if !(from.y >= t && to.y <= t) || best.is_some_and(|b| b.0 >= t) {
                continue;
            }
            let x = cross_x(from, to, t);
            let inset = s.corner_radius * 0.4;
            if x > s.frame.min_x() + inset && x < s.frame.max_x() - inset && !blocked(&s.blockers, x, t) {
                best = Some((t, x, s.id));
            }
        }
        if let Some((t, x, id)) = best {
            return Some(Hit { pos: Vec2::new(x, t), ledge: Some(id), pointer: false });
        }
        if to.y <= GROUND_Y {
            return Some(Hit { pos: Vec2::new(to.x, GROUND_Y), ledge: None, pointer: false });
        }
        None
    }

    fn splash(&mut self, hit: &Hit, d: &Drop) {
        let power = (d.vel.len() / 1300.0).min(1.3) * (0.4 + d.width * 0.45);
        let per = if hit.pointer { 3.5 } else { 2.5 };
        let room = MAX_DROPLETS.saturating_sub(self.droplets.len());
        let count = ((self.rng.unit() + power * per) as usize).min(room);
        for _ in 0..count {
            let a = if hit.pointer { self.rng.range(0.2, 2.9416) } else { self.rng.range(0.35, 2.7916) };
            let speed = self.rng.range(70.0, 200.0) * power * self.slow;
            let wmul = self.rng.range(0.7, 1.1);
            self.droplets.push(Droplet {
                pos: hit.pos + Vec2::new(0.0, 1.0),
                vel: Vec2::new(a.cos() * speed + d.vel.x * 0.15, a.sin() * speed * 1.1),
                width: (d.width * wmul).max(0.8),
                alpha: (d.alpha * 1.4).min(0.7),
                floor: hit.pos.y - if hit.pointer { 30.0 } else { 2.0 },
                glow: 0.0,
                age: 0.0,
            });
        }
        if !hit.pointer && self.ripples.len() < MAX_RIPPLES && (d.width > 1.0 || self.rng.unit() < 0.4) {
            let reach = 3.0 + d.width * 3.5 + self.rng.range(0.0, 2.0);
            let life = self.rng.range(0.25, 0.45);
            self.ripples.push(Ripple { center: hit.pos, reach, alpha: d.alpha * 1.1, life, age: 0.0 });
        }
        let Some(id) = hit.ledge else { return };
        let Some(l) = self.ledges.get_mut(&id) else { return };
        l.water += d.width * 0.35;
        if l.water > l.drip_at {
            let amount = l.water * 0.5;
            let frame = l.frame;
            let left = hit.pos.x < frame.mid_x();
            l.water *= 0.5;
            l.drip_at = self.rng.range(3.0, 9.0);
            let corner = self.surfaces.iter().find(|s| s.id == id).map_or(0.0, |s| s.corner_radius);
            let dir = if left { -1.0 } else { 1.0 };
            let x = if left { frame.min_x() - 0.5 } else { frame.max_x() + 0.5 };
            let out = self.rng.range(4.0, 14.0);
            let fall = self.rng.range(0.0, 30.0);
            self.drops.push(Drop {
                pos: Vec2::new(x, frame.max_y() - corner * 0.6),
                vel: Vec2::new(dir * out, -fall),
                width: (1.3 + amount * 0.12).min(2.6),
                alpha: 0.5,
                drift: 0.6,
                lands: true,
                dripping: true,
            });
        }
        if let Some(l) = self.ledges.get_mut(&id) {
            l.water = l.water.min(WATER_CAP);
        }
    }

    fn advance_drops(&mut self, dt: f64, cursor: Option<Vec2>) {
        let wind = self.wind_now();
        let collider = cursor.map(|c| c + POINTER_OFFSET);
        let target = self.target();
        let old = std::mem::take(&mut self.drops);
        let total = old.len();
        let mut kept = Vec::with_capacity(total);
        for (i, mut d) in old.into_iter().enumerate() {
            let prev = d.pos;
            if d.dripping {
                d.vel.y = (d.vel.y - GRAVITY * dt).max(-FASTEST_DRIP * self.slow);
                d.vel.x += (wind * d.drift * 0.3 - d.vel.x) * (2.0 * dt).min(1.0);
            } else {
                d.vel.x += (wind * d.drift - d.vel.x) * (1.5 * dt).min(1.0);
            }
            d.pos = d.pos + d.vel * dt;
            let mut gone = false;
            if d.lands
                && let Some(hit) = self.land(prev, d.pos, collider)
            {
                self.splash(&hit, &d);
                gone = true;
            }
            if !gone {
                let span = self.width + 2.0 * WRAP_MARGIN;
                if d.pos.x < -WRAP_MARGIN {
                    d.pos.x += span;
                } else if d.pos.x > self.width + WRAP_MARGIN {
                    d.pos.x -= span;
                }
                gone = d.pos.y < -40.0;
            }
            if !gone {
                kept.push(d);
            } else if !d.dripping && kept.len() + (total - i - 1) + self.drops.len() < target {
                let y = self.height + self.rng.range(0.0, 60.0);
                let nd = self.spawn_drop(y);
                self.drops.push(nd);
            }
        }
        kept.append(&mut self.drops);
        self.drops = kept;
    }

    fn stir(&mut self, dt: f64, cursor: Option<Vec2>) {
        if let (Some(c), Some(last)) = (cursor, self.last_cursor) {
            let vel = (c - last) * (1.0 / dt);
            if vel.len() > 20.0 {
                for d in &mut self.drops {
                    let dist = d.pos.dist(c);
                    if dist < WAKE_RADIUS {
                        d.pos = d.pos + vel * ((1.0 - dist / WAKE_RADIUS) * WAKE_FACTOR * dt);
                    }
                }
            }
        }
        self.last_cursor = cursor;
    }

    fn advance_particles(&mut self, dt: f64) {
        for d in &mut self.droplets {
            d.vel.y -= GRAVITY * dt;
            d.pos = d.pos + d.vel * dt;
            d.age += dt;
        }
        self.droplets.retain(|d| !((d.vel.y < 0.0 && d.pos.y < d.floor) || d.age > 0.9 || d.pos.y < -10.0));
        for r in &mut self.ripples {
            r.age += dt;
        }
        self.ripples.retain(|r| r.age < r.life);
    }

    fn advance_strikes(&mut self, dt: f64) {
        for s in &mut self.strikes {
            s.age += dt;
        }
        self.strikes.retain(|s| s.age <= s.life);
        if self.lightning && self.clock >= self.next_strike {
            self.next_strike = self.clock + self.rng.range(8.0, 26.0);
            self.bolt();
            if self.rng.unit() < 0.25 {
                self.next_strike = self.clock + self.rng.range(0.8, 1.8);
            }
        }
    }

    fn bolt(&mut self) {
        let pulses = if self.reduce {
            vec![0.0]
        } else if self.rng.unit() < 0.6 {
            vec![0.0, self.rng.range(0.08, 0.14), self.rng.range(0.22, 0.34)]
        } else {
            vec![0.0, self.rng.range(0.1, 0.2)]
        };
        let visible = self.rng.unit() < 0.75;
        let strength = match (visible, self.reduce) {
            (false, false) => 0.06,
            (false, true) => 0.02,
            (true, false) => 0.11,
            (true, true) => 0.03,
        };
        let life = pulses.last().copied().unwrap_or(0.0) + 0.5;
        let segs = if visible { self.bolt_segments() } else { Vec::new() };
        self.strikes.push(Strike { pulses, age: 0.0, life, strength, segs });
    }

    fn bolt_segments(&mut self) -> Vec<Seg> {
        let (w, h) = (self.width, self.height);
        let start = Vec2::new(self.rng.range(0.1 * w, 0.9 * w), h + 10.0);
        let x = start.x + self.rng.range(-220.0, 220.0);
        let end = self.land(Vec2::new(x, h), Vec2::new(x, -1.0), None).map_or(Vec2::new(x, GROUND_Y), |hit| hit.pos);
        let path = jagged(&mut self.rng, start, end, 6, 0.2);
        let mut segs: Vec<Seg> = path
            .windows(2)
            .enumerate()
            .map(|(i, p)| Seg { a: p[0], b: p[1], width: 2.4 - i as f64 / BOLT_POINTS, alpha: 1.0 })
            .collect();
        for _ in 0..self.rng.whole(2, 5) {
            let k = self.rng.whole(1, 45) as usize;
            let sign = if self.rng.coin() { 1.0 } else { -1.0 };
            let ang = self.rng.range(0.35, 0.9) * sign;
            let dir = (path[k + 1] - path[k]).normalized();
            let (s, c) = ang.sin_cos();
            let dir = Vec2::new(dir.x * c - dir.y * s, dir.x * s + dir.y * c);
            let tip = path[k] + dir * self.rng.range(70.0, 240.0);
            let branch = jagged(&mut self.rng, path[k], tip, 4, 0.25);
            for (j, p) in branch.windows(2).enumerate() {
                let f = j as f64 / BRANCH_POINTS;
                segs.push(Seg { a: p[0], b: p[1], width: 1.2 - 0.6 * f, alpha: 0.55 - 0.4 * f });
            }
        }
        let room = MAX_DROPLETS.saturating_sub(self.droplets.len());
        for _ in 0..18.min(room) {
            let a = self.rng.range(0.2, 2.9416);
            let speed = self.rng.range(150.0, 480.0) * self.slow;
            let width = self.rng.range(1.0, 1.8);
            self.droplets.push(Droplet {
                pos: end + Vec2::new(0.0, 1.0),
                vel: Vec2::new(a.cos() * speed, a.sin() * speed),
                width,
                alpha: 0.9,
                floor: end.y - 40.0,
                glow: 1.0,
                age: 0.0,
            });
        }
        if self.ripples.len() < MAX_RIPPLES {
            self.ripples.push(Ripple { center: end, reach: 26.0, alpha: 0.6, life: 0.6, age: 0.0 });
        }
        segs
    }

    fn top_up(&mut self) {
        let n = self.target().saturating_sub(self.drops.len()).min(SPAWN_BURST);
        for _ in 0..n {
            let y = self.height + self.rng.range(0.0, 0.2 * self.height);
            let d = self.spawn_drop(y);
            self.drops.push(d);
        }
    }

    fn fling(&mut self, s: &Surface, vel: Vec2, k: f64, water: f64) {
        let room = MAX_DROPLETS.saturating_sub(self.droplets.len());
        let count = (((k * water * 1.5) as usize) + 1).min(room).min(40);
        for _ in 0..count {
            let x = self.rng.range(s.frame.min_x(), s.frame.max_x());
            let m = self.rng.range(0.3, 0.7);
            let jx = self.rng.range(-40.0, 40.0);
            let jy = self.rng.range(40.0, 160.0);
            let width = self.rng.range(0.9, 1.8);
            self.droplets.push(Droplet {
                pos: Vec2::new(x, s.frame.max_y() + 1.0),
                vel: vel * m + Vec2::new(jx, jy),
                width,
                alpha: 0.55,
                floor: -10.0,
                glow: 0.0,
                age: 0.0,
            });
        }
    }

    fn pour(&mut self, l: &Ledge) {
        for _ in 0..((l.water / 2.0) as usize).min(14) {
            let x = self.rng.range(l.frame.min_x(), l.frame.max_x());
            let dy = self.rng.range(-2.0, 2.0);
            let vx = self.rng.range(-10.0, 10.0);
            let vy = -self.rng.range(0.0, 60.0);
            let width = self.rng.range(1.3, 2.2);
            self.drops.push(Drop {
                pos: Vec2::new(x, l.frame.max_y() + dy),
                vel: Vec2::new(vx, vy),
                width,
                alpha: 0.5,
                drift: 0.6,
                lands: true,
                dripping: true,
            });
        }
    }
}

impl Falling for RainField {
    fn set_share(&mut self, share: f64) {
        self.share = share;
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
        // NaN fails both comparisons; reject it too so it cannot poison the clock.
        if dt <= 0.0 || dt.is_nan() {
            return;
        }
        self.clock += dt;
        self.blow.step(dt, self.clock, self.wind, self.wind_t.unwrap_or(self.clock));
        self.stir(dt, cursor);
        self.advance_drops(dt, cursor);
        self.advance_particles(dt);
        self.advance_strikes(dt);
        self.top_up();
    }

    fn update(&mut self, surfaces: &[Surface], interval: f64, to_other_space: bool) {
        let dt = interval.max(1.0 / 120.0);
        let gone: Vec<SurfaceId> =
            self.ledges.keys().filter(|id| !surfaces.iter().any(|s| s.id == **id)).copied().collect();
        for id in gone {
            if let Some(l) = self.ledges.remove(&id)
                && !to_other_space
            {
                self.pour(&l);
            }
        }
        for s in surfaces {
            let Some(l) = self.ledges.get(&s.id) else {
                let drip_at = self.rng.range(3.0, 9.0);
                self.ledges.insert(s.id, Ledge { frame: s.frame, water: 0.0, drip_at });
                continue;
            };
            let (old, mut water) = (l.frame, l.water);
            let disp = Vec2::new(s.frame.mid_x() - old.mid_x(), s.frame.max_y() - old.max_y());
            let speed = disp.len() / dt;
            let same = old.w == s.frame.w && old.h == s.frame.h;
            if same && speed > FLING_SPEED && speed < JUMP_SPEED && water > 0.5 {
                let k = ((speed - FLING_SPEED) / 1500.0 + 0.3).min(1.0);
                self.fling(s, disp * (1.0 / dt), k, water);
                water *= 1.0 - k;
            }
            if let Some(l) = self.ledges.get_mut(&s.id) {
                l.frame = s.frame;
                l.water = water;
            }
        }
        self.surfaces = surfaces.to_vec();
    }

    fn draw(&self, layer: Layer, out: &mut DrawList) {
        if layer == Layer::Above {
            return;
        }
        out.scene[0] = self.width as f32;
        out.scene[1] = self.height as f32;
        out.scene[2] = self.clock as f32;
        let flash = self.flash();
        if flash > FLASH_MIN {
            out.push(Pass {
                kind: PassKind::RainFlash,
                uniforms: vec![flash as f32, self.height as f32],
                buffers: Vec::new(),
                vertices: 4,
                instances: 1,
            });
        }
        let ripples: Vec<RainRipple> = self
            .ripples
            .iter()
            .map(|r| {
                let p = (r.age / r.life).max(0.0);
                RainRipple {
                    center: [r.center.x as f32, r.center.y as f32],
                    radius: (r.reach * (0.25 + 0.75 * p.sqrt())) as f32,
                    alpha: (r.alpha * (1.0 - p).powi(2)) as f32,
                }
            })
            .collect();
        out.instanced(PassKind::RainRipples, Vec::new(), &ripples);

        let mut strokes: Vec<RainStroke> = Vec::with_capacity(self.drops.len() + self.droplets.len());
        for d in &self.drops {
            let s = d.vel.len();
            let dir = if s == 0.0 { Vec2::new(0.0, -1.0) } else { d.vel.normalized() };
            let len = (s * 0.022).max(d.width * 1.5);
            strokes.push(stroke(d.pos, d.pos - dir * len, d.width, d.alpha, 0.0));
        }
        for d in &self.droplets {
            let back = d.vel * 0.016;
            let tail = if back.len() > d.width { d.pos - back } else { d.pos - Vec2::new(0.0, 0.01) };
            let fade = 1.0 - (d.age / 0.9).min(1.0);
            strokes.push(stroke(d.pos, tail, d.width, d.alpha * fade, d.glow));
        }
        for s in &self.strikes {
            let b = s.brightness();
            for g in &s.segs {
                strokes.push(stroke(g.a, g.b, g.width * 7.0, 0.2 * b * g.alpha, 2.0));
                strokes.push(stroke(g.a, g.b, g.width, b * g.alpha, 1.0));
            }
        }
        out.instanced(PassKind::RainStrokes, Vec::new(), &strokes);
    }

    fn set_lightning(&mut self, on: bool) {
        self.lightning = on;
    }
}

fn stroke(head: Vec2, tail: Vec2, width: f64, alpha: f64, glow: f32) -> RainStroke {
    RainStroke {
        head: [head.x as f32, head.y as f32],
        tail: [tail.x as f32, tail.y as f32],
        width: width as f32,
        alpha: alpha as f32,
        glow,
        pad: 0.0,
    }
}

#[cfg(test)]
mod tests;
