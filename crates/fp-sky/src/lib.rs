//! Starfall and fireflies for one monitor.
//!
//! Spec: other-weather 1, 1.1 (Blow strength 1 for starfall), 2.3.1-2.3.5
//! (stars, meteors, shared hit test, render data), 2.4.1-2.4.4 (fireflies:
//! population, spawn, motion around a still pointer, render data), open
//! question 3 (40/60 strike split: keep).
//!
//! Output passes: `StarTrails`, `Stars` (additive) / `Fireflies` (additive).

use fp_core::falling::Falling;
use fp_core::geom::Vec2;
use fp_core::gpu::{DrawList, FireflySprite, Layer, PassKind, StarSprite, StarTrail};
use fp_core::rng::{Lcg64, Rng};
use fp_core::surface::{Surface, SurfaceId, blocked};
use fp_core::wind::{Blow, Wind};

use std::f64::consts::{PI, TAU};

const STARS_PER_MEGAPOINT: f64 = 28.0;
const WAKE_RADIUS: f64 = 90.0;
const STRIKE_SHARE: f64 = 0.4;
const GROUND_Y: f64 = 3.0;
const FADE_TIME: f64 = 1.0;
const HUES: [f64; 4] = [0.0, 0.0, 0.5, 1.0];
const BURST_CAP: usize = 500;
const TRAIL_SPARK_CAP: usize = 400;

const FIREFLIES_PER_MEGAPOINT: f64 = 42.0;
const GATHER_AFTER: f64 = 1.2;
const GATHER_REACH: f64 = 420.0;
const SCATTER_REACH: f64 = 160.0;
const SCATTER_SPEED: f64 = 450.0;

#[cfg(test)]
mod tests;

/// Density target (other-weather 1): `int(per_megapoint * share * W * H / 1e6)`.
/// A non-finite product (infinite share or size) yields 0 instead of
/// `usize::MAX`, so a bad share can never allocate without bound.
fn pop_target(per_megapoint: f64, share: f64, w: f64, h: f64) -> usize {
    let n = per_megapoint * share * w * h / 1e6;
    if n.is_finite() { n as usize } else { 0 }
}

fn visible_ledges(surfaces: &[Surface]) -> impl Iterator<Item = &Surface> {
    surfaces.iter().filter(|s| s.id != SurfaceId::Ground)
}

/// Shared hit test (2.3.4): highest ledge top crossed by the segment, else the
/// ground at y = 3. Returns the hit point.
fn hit_test(ledges: &[Surface], from: Vec2, to: Vec2) -> Option<Vec2> {
    let x_at = |y: f64| {
        let d = from.y - to.y;
        let t = if d.abs() < 1e-12 { 0.0 } else { (from.y - y) / d };
        from.x + (to.x - from.x) * t
    };
    let mut best: Option<Vec2> = None;
    for s in visible_ledges(ledges) {
        let top = s.frame.max_y();
        if !(from.y >= top && to.y <= top) {
            continue;
        }
        let x = x_at(top);
        let inset = s.corner_radius * 0.5;
        let inside = x >= s.frame.min_x() + inset && x <= s.frame.max_x() - inset;
        if inside && !blocked(&s.blockers, x, top) && best.is_none_or(|b| top > b.y) {
            best = Some(Vec2::new(x, top));
        }
    }
    if best.is_none() && from.y >= GROUND_Y && to.y <= GROUND_Y {
        best = Some(Vec2::new(x_at(GROUND_Y), GROUND_Y));
    }
    best
}

#[derive(Clone, Copy, Debug)]
struct Rest {
    id: SurfaceId,
    along: f64,
    until: f64,
}

#[derive(Clone, Debug)]
struct Star {
    pos: Vec2,
    v: Vec2,
    fall: f64,
    size: f64,
    angle: f64,
    spin: f64,
    twinkle: f64,
    rate: f64,
    alpha: f64,
    hue: f64,
    rest: Option<Rest>,
}

#[derive(Clone, Debug)]
struct Meteor {
    head: Vec2,
    v: Vec2,
    dir: Vec2,
    age: f64,
    life: f64,
    length: f64,
    width: f64,
    brightness: f64,
    hue: f64,
    bright: bool,
    hit_at: Option<f64>,
    last_spark_age: f64,
}

#[derive(Clone, Debug)]
struct Spark {
    pos: Vec2,
    v: Vec2,
    age: f64,
    life: f64,
    size: f64,
    gravity: f64,
    hue: f64,
    flash: bool,
}

/// Night sky: twinkling stars that rest on window tops plus shooting stars.
pub struct StarfallField {
    width: f64,
    height: f64,
    slow: f64,
    rng: Lcg64,
    share: f64,
    wind: Wind,
    blow: Blow,
    /// Wall-clock time base of the changing wind (`None` = field clock).
    wind_t: Option<f64>,
    clock: f64,
    last_cursor: Option<Vec2>,
    pointer_vel: Vec2,
    radiant: f64,
    next_meteor: f64,
    ledges: Vec<Surface>,
    stars: Vec<Star>,
    meteors: Vec<Meteor>,
    sparks: Vec<Spark>,
}

impl StarfallField {
    pub fn new(width: f64, height: f64, reduce_motion: bool, seed: u64) -> Self {
        let mut rng = Lcg64::new(seed);
        let radiant = if rng.coin() { -2.55 } else { -0.6 };
        let next_meteor = rng.range(0.5, 2.0);
        let mut f = Self {
            width,
            height,
            slow: if reduce_motion { 0.5 } else { 1.0 },
            rng,
            share: 1.0,
            wind: Wind::None,
            blow: Blow::new(1.0),
            wind_t: None,
            clock: 0.0,
            last_cursor: None,
            pointer_vel: Vec2::ZERO,
            radiant,
            next_meteor,
            ledges: Vec::new(),
            stars: Vec::new(),
            meteors: Vec::new(),
            sparks: Vec::new(),
        };
        for _ in 0..f.target() {
            let y = f.rng.range(0.0, height);
            f.spawn_star(y);
        }
        f
    }

    fn target(&self) -> usize {
        pop_target(STARS_PER_MEGAPOINT, self.share, self.width, self.height)
    }

    fn spawn_star(&mut self, y: f64) {
        let q = self.rng.unit().powf(1.4);
        let x = self.rng.range(0.0, self.width);
        let fall = (14.0 + 26.0 * q) * self.slow;
        let size = 3.0 + 5.0 * q;
        let angle = self.rng.range(0.0, PI / 2.0);
        let spin = self.rng.range(-0.4, 0.4) * self.slow;
        let twinkle = self.rng.range(0.0, TAU);
        let rate = self.rng.range(1.5, 4.0);
        let hue = HUES[self.rng.below(HUES.len() as u64) as usize];
        self.stars.push(Star {
            pos: Vec2::new(x, y),
            v: Vec2::new(0.0, -fall),
            fall,
            size,
            angle,
            spin,
            twinkle,
            rate,
            alpha: 0.45 + 0.5 * q,
            hue,
            rest: None,
        });
    }

    fn ledge(&self, id: SurfaceId) -> Option<&Surface> {
        self.ledges.iter().find(|s| s.id == id)
    }

    /// Returns true when the star should be removed.
    fn advance_star(&mut self, s: &mut Star, dt: f64) -> bool {
        if let Some(r) = s.rest {
            s.twinkle += s.rate * dt * 2.0;
            if self.clock >= r.until {
                return true;
            }
            if let Some(l) = self.ledge(r.id) {
                s.pos = Vec2::new(l.frame.min_x() + r.along, l.frame.max_y() + s.size * 0.35);
            }
            return false;
        }
        s.twinkle += s.rate * dt;
        s.angle += s.spin * dt;
        let target = Vec2::new((0.3 * s.twinkle).sin() * 6.0 + self.blow.speed * s.fall / (40.0 * self.slow), -s.fall);
        s.v = s.v + (target - s.v) * (1.5 * dt).min(1.0);
        if let Some(c) = self.last_cursor {
            let speed = self.pointer_vel.len();
            let d = s.pos.dist(c);
            if speed > 20.0 && d < WAKE_RADIUS {
                let k = 1.0 - d / WAKE_RADIUS;
                s.v = s.v + self.pointer_vel * (k * 2.0 * dt);
                s.spin += k * self.rng.range(-3.0, 3.0) * dt;
            }
        }
        let prev = s.pos;
        s.pos = s.pos + s.v * dt;
        if s.v.y < 0.0
            && let Some(h) = hit_test(&self.ledges, prev, s.pos)
        {
            let found = visible_ledges(&self.ledges)
                .find(|l| (l.frame.max_y() - h.y).abs() < 0.5 && l.frame.min_x() <= h.x && h.x <= l.frame.max_x());
            let (id, along) = match found {
                Some(l) => (l.id, h.x - l.frame.min_x()),
                None => (SurfaceId::Ground, h.x),
            };
            s.pos = h + Vec2::new(0.0, s.size * 0.35);
            let until = self.clock + self.rng.range(3.0, 8.0) + FADE_TIME;
            s.rest = Some(Rest { id, along, until });
            return false;
        }
        if s.pos.x < -40.0 {
            s.pos.x += self.width + 80.0;
        } else if s.pos.x > self.width + 40.0 {
            s.pos.x -= self.width + 80.0;
        }
        s.pos.y < -10.0
    }

    fn step_stars(&mut self, dt: f64) {
        let mut stars = std::mem::take(&mut self.stars);
        stars.retain_mut(|s| !self.advance_star(s, dt));
        self.stars = stars;
        let not_resting = self.stars.iter().filter(|s| s.rest.is_none()).count();
        let fill = self.target().saturating_sub(not_resting).min(3);
        for _ in 0..fill {
            let y = self.height + self.rng.range(5.0, 40.0);
            self.spawn_star(y);
        }
    }

    fn launch(&mut self) {
        if self.rng.unit() < STRIKE_SHARE {
            self.strike();
        } else {
            self.free_meteor();
        }
    }

    fn hue_pick(&mut self) -> f64 {
        if self.rng.coin() { 0.0 } else { self.rng.range(0.5, 1.0) }
    }

    fn free_meteor(&mut self) {
        let angle = self.radiant + self.rng.range(-0.22, 0.22);
        let bright = self.rng.unit() < 0.15;
        let speed = self.rng.range(900.0, 1700.0) * self.slow;
        let life = self.rng.range(0.5, 1.0) * if bright { 1.3 } else { 1.0 };
        let travel = speed * life;
        let dir = Vec2::new(angle.cos(), angle.sin());
        let (w, h) = (self.width, self.height);
        let x = if dir.x < 0.0 {
            self.rng.range(0.25 * w, w + 0.3 * travel)
        } else {
            self.rng.range(-0.3 * travel, 0.75 * w)
        };
        let y = self.rng.range(0.45 * h, 1.02 * h);
        let length = self.rng.range(200.0, 420.0) * if bright { 1.4 } else { 1.0 };
        let width = if bright { self.rng.range(2.2, 3.0) } else { self.rng.range(1.1, 2.0) };
        let brightness = if bright { 1.0 } else { self.rng.range(0.5, 0.9) };
        let hue = self.hue_pick();
        self.meteors.push(Meteor {
            head: Vec2::new(x, y),
            v: dir * speed,
            dir,
            age: 0.0,
            life,
            length,
            width,
            brightness,
            hue,
            bright,
            hit_at: None,
            last_spark_age: 0.0,
        });
    }

    fn strike(&mut self) {
        let w = self.width;
        let mut targets = vec![Vec2::new(self.rng.range(0.1 * w, 0.9 * w), GROUND_Y)];
        for i in 0..self.ledges.len() {
            let s = &self.ledges[i];
            if s.frame.w <= 120.0 {
                continue;
            }
            let (lo, hi, top) = (s.frame.min_x() + 30.0, s.frame.max_x() - 30.0, s.frame.max_y());
            let x = self.rng.range(lo, hi);
            if !blocked(&self.ledges[i].blockers, x, top) {
                targets.push(Vec2::new(x, top));
            }
        }
        let target = targets[self.rng.below(targets.len() as u64) as usize];
        let base = if self.radiant < -1.57 { -2.15 } else { -0.99 };
        let angle = base + self.rng.range(-0.15, 0.15);
        let dir = Vec2::new(angle.cos(), angle.sin());
        let speed = self.rng.range(1100.0, 1700.0) * self.slow;
        let life = self.rng.range(0.7, 1.0);
        let head = target - dir * (speed * life * 0.65);
        let length = self.rng.range(200.0, 380.0);
        let bright = self.rng.unit() < 0.35;
        let width = if bright { self.rng.range(2.2, 3.0) } else { self.rng.range(1.5, 2.2) };
        let brightness = if bright { 1.0 } else { self.rng.range(0.7, 0.95) };
        let hue = self.hue_pick();
        self.meteors.push(Meteor {
            head,
            v: dir * speed,
            dir,
            age: 0.0,
            life,
            length,
            width,
            brightness,
            hue,
            bright,
            hit_at: None,
            last_spark_age: 0.0,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn spark(&mut self, pos: Vec2, v: Vec2, life: f64, size: f64, gravity: f64, hue: f64, flash: bool) {
        self.sparks.push(Spark { pos, v, age: 0.0, life, size, gravity, hue, flash });
    }

    fn burst(&mut self, at: Vec2, bright: bool, dir_x: f64) {
        let size = if bright { 34.0 } else { 24.0 };
        self.spark(at + Vec2::new(0.0, 2.0), Vec2::ZERO, 0.45, size, 0.0, 0.0, true);
        let want: i64 = if bright { 30 } else { 18 };
        let n = want.min(BURST_CAP as i64 - self.sparks.len() as i64);
        for _ in 0..n.max(0) {
            let a = self.rng.range(0.15, 2.9916);
            let speed = self.rng.range(90.0, 380.0) * self.slow;
            let v = Vec2::new(a.cos() * speed + dir_x * self.rng.range(40.0, 160.0), a.sin() * speed);
            let life = self.rng.range(0.5, 1.1);
            let size = self.rng.range(2.2, 4.0);
            let hue = if self.rng.coin() { 0.5 } else { 0.0 };
            self.spark(at + Vec2::new(0.0, 1.0), v, life, size, 700.0, hue, false);
        }
    }

    /// Returns true to keep the meteor.
    fn advance_meteor(&mut self, m: &mut Meteor, dt: f64) -> bool {
        m.age += dt;
        if let Some(h) = m.hit_at {
            return m.age - h <= 0.16;
        }
        let prev = m.head;
        m.head = m.head + m.v * dt;
        if let Some(p) = hit_test(&self.ledges, prev, m.head) {
            m.head = p;
            m.hit_at = Some(m.age);
            self.burst(p, m.bright, m.dir.x);
            return true;
        }
        if m.bright && m.age - m.last_spark_age > 0.02 && self.sparks.len() < TRAIL_SPARK_CAP {
            m.last_spark_age = m.age;
            for _ in 0..2 {
                let at = m.head + (-m.dir) * self.rng.range(0.0, 20.0);
                let k = self.rng.range(20.0, 80.0);
                let jx = self.rng.range(-30.0, 30.0);
                let jy = self.rng.range(-30.0, 30.0);
                let v = (-m.dir) * k + Vec2::new(jx, jy);
                let life = self.rng.range(0.3, 0.7);
                let size = self.rng.range(2.0, 3.5);
                self.spark(at, v, life, size, 60.0, 0.5, false);
            }
        }
        if m.age >= m.life {
            if m.bright {
                self.spark(m.head, Vec2::ZERO, 0.7, 22.0, 0.0, 0.0, true);
            }
            return false;
        }
        true
    }

    fn step_meteors(&mut self, dt: f64) {
        if self.clock >= self.next_meteor {
            self.launch();
            let gap = if self.rng.unit() < 0.18 { self.rng.range(0.3, 0.8) } else { self.rng.range(1.8, 4.5) };
            self.next_meteor = self.clock + gap / (self.slow * self.share.max(0.5));
        }
        let mut ms = std::mem::take(&mut self.meteors);
        ms.retain_mut(|m| self.advance_meteor(m, dt));
        self.meteors = ms;
    }

    fn step_sparks(&mut self, dt: f64) {
        for s in &mut self.sparks {
            s.v = s.v * (-dt * if s.gravity > 100.0 { 0.8 } else { 2.0 }).exp();
            s.v.y -= s.gravity * dt;
            s.pos = s.pos + s.v * dt;
            s.age += dt;
        }
        self.sparks.retain(|s| s.age < s.life);
    }
}

impl Falling for StarfallField {
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
        // Also rejects NaN, which would poison the clock.
        if !(dt > 0.0 && dt.is_finite()) {
            return;
        }
        self.clock += dt;
        self.blow.step(dt, self.clock, self.wind, self.wind_t.unwrap_or(self.clock));
        self.pointer_vel = match (cursor, self.last_cursor) {
            (Some(c), Some(l)) => (c - l) * (1.0 / dt),
            _ => Vec2::ZERO,
        };
        self.last_cursor = cursor;
        self.step_meteors(dt);
        self.step_stars(dt);
        self.step_sparks(dt);
    }

    fn update(&mut self, surfaces: &[Surface], _interval: f64, _to_other_space: bool) {
        self.ledges = visible_ledges(surfaces).cloned().collect();
        let ledges = &self.ledges;
        for s in &mut self.stars {
            let gone = s.rest.is_some_and(|r| r.id != SurfaceId::Ground && !ledges.iter().any(|l| l.id == r.id));
            if gone {
                s.rest = None;
            }
        }
    }

    fn draw(&self, layer: Layer, out: &mut DrawList) {
        if layer != Layer::Main {
            return;
        }
        let mut sprites: Vec<StarSprite> = Vec::with_capacity(self.stars.len() + self.sparks.len());
        for s in &self.stars {
            let tf = 0.55 + 0.45 * (0.5 + 0.5 * s.twinkle.sin()).powi(3);
            let fade = match s.rest {
                Some(r) => {
                    let hidden = self.ledge(r.id).is_some_and(|l| blocked(&l.blockers, s.pos.x, s.pos.y));
                    if hidden { 0.0 } else { ((r.until - self.clock) / FADE_TIME).clamp(0.0, 1.0) }
                }
                None => (s.pos.y / 60.0).clamp(0.0, 1.0),
            };
            sprites.push(StarSprite {
                position: [s.pos.x as f32, s.pos.y as f32],
                size: (s.size * (0.8 + 0.3 * tf)) as f32,
                angle: s.angle as f32,
                alpha: (s.alpha * tf * fade) as f32,
                hue: s.hue as f32,
            });
        }
        for p in &self.sparks {
            let t = p.age / p.life;
            let (size, alpha, angle) = if p.flash {
                (p.size * (0.6 + 0.8 * t), (1.0 - t) * (1.0 - t), 0.4)
            } else {
                (p.size, 0.85 * (1.0 - t), 0.0)
            };
            sprites.push(StarSprite {
                position: [p.pos.x as f32, p.pos.y as f32],
                size: size as f32,
                angle: angle as f32,
                alpha: alpha as f32,
                hue: p.hue as f32,
            });
        }
        out.instanced(PassKind::Stars, Vec::new(), &sprites);

        let trails: Vec<StarTrail> = self.meteors.iter().map(trail_of).collect();
        out.instanced(PassKind::StarTrails, Vec::new(), &trails);
    }
}

fn trail_of(m: &Meteor) -> StarTrail {
    let t = m.age / m.life;
    let mut x = (t / 0.12).min(1.0) * if t > 0.6 { 1.0 - (t - 0.6) / 0.4 } else { 1.0 };
    let mut len = m.length * (m.age * 6.0).min(1.0);
    if let Some(h) = m.hit_at {
        let k = ((m.age - h) / 0.16).min(1.0);
        len *= 1.0 - k;
        x = 1.0 - 0.5 * k;
    }
    let tail = m.head - m.dir * len;
    StarTrail {
        head: [m.head.x as f32, m.head.y as f32],
        tail: [tail.x as f32, tail.y as f32],
        width: m.width as f32,
        alpha: (m.brightness * x.max(0.0)) as f32,
        hue: m.hue as f32,
    }
}

#[derive(Clone, Debug)]
struct Fly {
    home: Vec2,
    pos: Vec2,
    v: Vec2,
    phase: (f64, f64),
    rate: (f64, f64),
    speed: f64,
    blink_at: f64,
    period: f64,
    hue: f64,
    size: f64,
    orbit: f64,
    orbit_radius: f64,
    lit: f64,
}

/// Blink curve (2.4.3).
fn blink(clock: f64, blink_at: f64) -> f64 {
    let u = clock - blink_at;
    if !(0.0..2.2).contains(&u) {
        return 0.12;
    }
    let p = u / 2.2;
    let shape = if p < 0.3 { (p / 0.3).powf(1.5) } else { (1.0 - (p - 0.3) / 0.7).powf(1.6) };
    0.12 + 0.88 * shape
}

/// Drifting fireflies that circle a still pointer.
pub struct FireflyField {
    width: f64,
    height: f64,
    slow: f64,
    rng: Lcg64,
    share: f64,
    clock: f64,
    last_cursor: Option<Vec2>,
    still_since: f64,
    flies: Vec<Fly>,
}

impl FireflyField {
    pub fn new(width: f64, height: f64, reduce_motion: bool, seed: u64) -> Self {
        let mut f = Self {
            width,
            height,
            slow: if reduce_motion { 0.5 } else { 1.0 },
            rng: Lcg64::new(seed),
            share: 1.0,
            clock: 0.0,
            last_cursor: None,
            still_since: 0.0,
            flies: Vec::new(),
        };
        f.fit();
        f
    }

    fn count(&self) -> usize {
        pop_target(FIREFLIES_PER_MEGAPOINT, self.share, self.width, self.height).max(8)
    }

    fn fit(&mut self) {
        let n = self.count();
        self.flies.truncate(n);
        while self.flies.len() < n {
            let fly = self.new_fly();
            self.flies.push(fly);
        }
    }

    fn new_fly(&mut self) -> Fly {
        let r = &mut self.rng;
        let home = Vec2::new(r.range(0.0, self.width), self.height * r.unit().powf(1.8) * 0.85 + 20.0);
        let pos = home + Vec2::new(r.range(-60.0, 60.0), r.range(-40.0, 40.0));
        let phase = (r.range(0.0, TAU), r.range(0.0, TAU));
        let rate = (r.range(0.25, 0.7) * self.slow, r.range(0.2, 0.6) * self.slow);
        let speed = r.range(12.0, 26.0) * self.slow;
        let blink_at = self.clock + r.range(0.0, 5.0);
        let period = r.range(2.5, 5.0);
        let hue = r.unit();
        let size = r.range(1.0, 1.6);
        let orbit = r.range(0.0, TAU);
        let orbit_radius = r.range(28.0, 80.0);
        Fly {
            home,
            pos,
            v: Vec2::ZERO,
            phase,
            rate,
            speed,
            blink_at,
            period,
            hue,
            size,
            orbit,
            orbit_radius,
            lit: 0.12,
        }
    }
}

impl Falling for FireflyField {
    fn set_share(&mut self, share: f64) {
        self.share = share;
        self.fit();
    }

    fn share(&self) -> f64 {
        self.share
    }

    fn set_wind(&mut self, _wind: Wind) {}

    fn step(&mut self, dt: f64, cursor: Option<Vec2>) {
        // Also rejects NaN, which would poison the clock.
        if !(dt > 0.0 && dt.is_finite()) {
            return;
        }
        self.clock += dt;
        let pointer_speed = match (cursor, self.last_cursor) {
            (Some(c), Some(l)) => c.dist(l) / dt,
            _ => 0.0,
        };
        self.last_cursor = cursor;
        if cursor.is_none() || pointer_speed > 15.0 {
            self.still_since = self.clock;
        }
        let gather = cursor.is_some() && self.clock - self.still_since > GATHER_AFTER;
        let (w, h, slow, clock) = (self.width, self.height, self.slow, self.clock);
        for f in &mut self.flies {
            f.phase.0 += f.rate.0 * dt;
            f.phase.1 += f.rate.1 * dt;
            let (px, py) = f.phase;
            let wander = Vec2::new(px.sin() + 0.5 * (2.3 * px + 1.1).sin(), py.cos() + 0.5 * (1.7 * py + 2.0).sin())
                * f.speed
                + (f.home - f.pos) * 0.02;
            let mut target = wander;
            if let Some(c) = cursor {
                let d = f.pos - c;
                let dist = d.len();
                if gather && dist < GATHER_REACH {
                    f.orbit += dt * 0.5 * slow * if f.hue > 0.5 { 1.0 } else { -1.0 };
                    let ring = c + Vec2::new(f.orbit.cos(), f.orbit.sin()) * f.orbit_radius;
                    target = (ring - f.pos) * 0.9 + wander * 0.3;
                } else if pointer_speed > SCATTER_SPEED && dist > 0.1 && dist < SCATTER_REACH {
                    f.v = f.v + d * (1.0 / dist) * ((1.0 - dist / SCATTER_REACH) * 240.0 * slow);
                }
            }
            f.v = f.v + (target - f.v) * (1.6 * dt).min(1.0);
            f.pos = f.pos + f.v * dt;
            f.pos.x = f.pos.x.clamp(-20.0, w + 20.0);
            f.pos.y = f.pos.y.clamp(-20.0, h + 20.0);
            f.lit = blink(clock, f.blink_at);
            if clock >= f.blink_at + f.period {
                f.blink_at += f.period * self.rng.range(0.8, 1.25);
            }
        }
    }

    fn update(&mut self, _surfaces: &[Surface], _interval: f64, _to_other_space: bool) {}

    fn draw(&self, layer: Layer, out: &mut DrawList) {
        if layer != Layer::Main {
            return;
        }
        let sprites: Vec<FireflySprite> = self
            .flies
            .iter()
            .map(|f| FireflySprite {
                position: [f.pos.x as f32, f.pos.y as f32],
                glow: f.lit as f32,
                hue: f.hue as f32,
                size: f.size as f32,
                pad: 0.0,
            })
            .collect();
        out.instanced(PassKind::Fireflies, Vec::new(), &sprites);
    }
}
