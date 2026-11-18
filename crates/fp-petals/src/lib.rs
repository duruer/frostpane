//! Sakura petals and autumn leaves for one monitor (one type, two kinds).
//!
//! Spec: other-weather 1, 1.1 (Blow strength 1), 2.2.1-2.2.7 (per-kind
//! parameters, wind, spawn, flight, resting, surface updates, render data),
//! open question 2 (order-dependent resting cap: keep as specced).
//!
//! Output pass: [`fp_core::gpu::PassKind::Petals`].

use std::f64::consts::{FRAC_PI_2, TAU};

use fp_core::falling::Falling;
use fp_core::geom::{Frame, Vec2};
use fp_core::gpu::{DrawList, Layer, PassKind, PetalSprite};
use fp_core::rng::{Lcg64, Rng};
use fp_core::surface::{Surface, SurfaceId, blocked};
use fp_core::wind::{Blow, Wind};

/// Seconds a resting sprite fades out at the end of its rest.
const FADE_TIME: f64 = 1.5;
/// Ground line (screen-local, Y up).
const GROUND_Y: f64 = 2.0;
/// Window speed range (pt/s) that flings resting sprites off a ledge.
const FLING_MIN: f64 = 450.0;
const FLING_MAX: f64 = 6000.0;
/// Pointer wake radius for falling sprites.
const WAKE_RADIUS: f64 = 90.0;
/// Pointer speed that loosens a resting sprite.
const LIFT_SPEED: f64 = 120.0;
/// Horizontal wrap margin.
const EDGE_MARGIN: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PetalKind {
    Sakura,
    Leaves,
}

/// Per-kind tuning (other-weather 2.2.1).
struct Params {
    per_megapoint: f64,
    most_resting: usize,
    resting_room: f64,
    ground_room: f64,
    lift_radius: f64,
    rest_lo: f64,
    rest_hi: f64,
}

impl PetalKind {
    fn params(self) -> Params {
        match self {
            PetalKind::Sakura => Params {
                per_megapoint: 90.0,
                most_resting: 160,
                resting_room: 45.0,
                ground_room: 45.0,
                lift_radius: 24.0,
                rest_lo: 6.0,
                rest_hi: 16.0,
            },
            PetalKind::Leaves => Params {
                per_megapoint: 55.0,
                most_resting: 260,
                resting_room: 32.0,
                ground_room: 9.0,
                lift_radius: 34.0,
                rest_lo: 14.0,
                rest_hi: 30.0,
            },
        }
    }
}

/// Where a resting sprite lies.
#[derive(Clone, Copy, Debug)]
struct Rest {
    on: SurfaceId,
    /// Offset from the ledge's left edge (ground: absolute x).
    along: f64,
    flip: f64,
    until: f64,
}

#[derive(Clone, Debug)]
struct Sprite {
    pos: Vec2,
    vel: Vec2,
    size: f64,
    angle: f64,
    spin: f64,
    tumble: f64,
    tumble_rate: f64,
    sway_phase: f64,
    sway: f64,
    sway_rate: f64,
    fall: f64,
    drift: f64,
    alpha: f64,
    soft: f64,
    tint: f64,
    shape: f64,
    lands: bool,
    rest: Option<Rest>,
}

pub struct PetalField {
    kind: PetalKind,
    width: f64,
    height: f64,
    slow: f64,
    rng: Lcg64,
    share: f64,
    wind: Wind,
    clock: f64,
    blow: Blow,
    /// Wall-clock time base of the changing wind (`None` = field clock).
    wind_t: Option<f64>,
    sprites: Vec<Sprite>,
    ledges: Vec<Surface>,
    last_cursor: Option<Vec2>,
}

/// Per-step values shared by all falling sprites.
struct Flight {
    dt: f64,
    wind: f64,
    cursor: Option<Vec2>,
    pointer_vel: Vec2,
    room_to_rest: bool,
}

/// Height of a resting sprite's centre above its contact point.
fn lie_height(size: f64, flip: f64) -> f64 {
    size * 0.72 * flip.abs() * 0.8
}

/// Release a sprite from its rest with a new velocity.
fn loosen(s: &mut Sprite, rng: &mut Lcg64, slow: f64, vel: Vec2) {
    s.rest = None;
    s.vel = vel;
    s.spin = rng.range(-5.0, 5.0) * slow;
    s.tumble = rng.range(0.0, TAU);
}

fn random_sign(rng: &mut Lcg64) -> f64 {
    if rng.coin() { 1.0 } else { -1.0 }
}

impl PetalField {
    pub fn new(kind: PetalKind, width: f64, height: f64, reduce_motion: bool, seed: u64) -> Self {
        let mut rng = Lcg64::new(seed);
        let clock = rng.range(0.0, 1000.0);
        let mut field = Self {
            kind,
            width,
            height,
            slow: if reduce_motion { 0.5 } else { 1.0 },
            rng,
            share: 1.0,
            wind: Wind::None,
            clock,
            blow: Blow::new(1.0),
            wind_t: None,
            sprites: Vec::new(),
            ledges: Vec::new(),
            last_cursor: None,
        };
        for _ in 0..field.target() {
            let y = field.rng.range(0.0, height * 1.1);
            field.spawn(y);
        }
        field
    }

    /// Wanted number of falling sprites (int truncation).
    fn target(&self) -> usize {
        (self.kind.params().per_megapoint * self.share * self.width * self.height / 1e6) as usize
    }

    fn falling(&self) -> usize {
        self.sprites.iter().filter(|s| s.rest.is_none()).count()
    }

    fn resting_on(&self, id: SurfaceId) -> usize {
        self.sprites.iter().filter(|s| s.rest.is_some_and(|r| r.on == id)).count()
    }

    fn ledge(&self, id: SurfaceId) -> Option<&Surface> {
        self.ledges.iter().find(|l| l.id == id)
    }

    /// `RoomOn(id)`: capacity left on a ledge (0 for an unknown ledge).
    fn room_on(&self, id: SurfaceId) -> bool {
        let p = self.kind.params();
        let capacity = if id == SurfaceId::Ground {
            self.width / p.ground_room
        } else {
            self.ledge(id).map_or(0.0, |l| l.frame.w / p.resting_room)
        };
        (self.resting_on(id) as f64) < capacity
    }

    fn spawn(&mut self, y: f64) {
        let slow = self.slow;
        let petal = self.kind == PetalKind::Sakura;
        let width = self.width;
        let r = &mut self.rng;
        let near = r.unit() < 0.03;
        let d = if near { 1.0 } else { r.unit().powf(1.3) };
        let x = r.range(-EDGE_MARGIN, width + EDGE_MARGIN);
        let angle = r.range(0.0, TAU);
        let tumble = r.range(0.0, TAU);
        let sway_phase = r.range(0.0, TAU);
        let tint = r.unit();
        let sprite = if petal {
            Sprite {
                pos: Vec2::new(x, y),
                vel: Vec2::new(0.0, -40.0),
                size: if near { r.range(12.0, 15.0) } else { 4.0 + 4.0 * d + r.range(0.0, 1.0) },
                angle,
                spin: r.range(-1.6, 1.6) * slow,
                tumble,
                tumble_rate: r.range(1.2, 3.4) * random_sign(r) * slow,
                sway_phase,
                sway: (r.range(14.0, 30.0) + 16.0 * d) * slow,
                sway_rate: r.range(0.7, 1.6) * slow,
                fall: (if near { r.range(90.0, 120.0) } else { 26.0 + 38.0 * d + r.range(0.0, 10.0) }) * slow,
                drift: if near { 1.3 } else { 0.4 + 0.6 * d },
                alpha: if near { 0.6 } else { 0.75 + 0.25 * d },
                soft: if near { 0.35 } else { 0.0 },
                tint,
                shape: 0.0,
                lands: !near,
                rest: None,
            }
        } else {
            Sprite {
                pos: Vec2::new(x, y),
                vel: Vec2::new(0.0, -50.0),
                size: if near { r.range(22.0, 28.0) } else { 8.5 + 6.0 * d + r.range(0.0, 2.0) },
                angle,
                spin: r.range(-1.2, 1.2) * slow,
                tumble,
                tumble_rate: r.range(0.8, 2.2) * random_sign(r) * slow,
                sway_phase,
                sway: (r.range(22.0, 40.0) + 20.0 * d) * slow,
                sway_rate: r.range(0.5, 1.1) * slow,
                fall: (if near { r.range(110.0, 140.0) } else { 38.0 + 46.0 * d + r.range(0.0, 12.0) }) * slow,
                drift: if near { 1.3 } else { 0.4 + 0.6 * d },
                alpha: if near { 0.7 } else { 0.88 + 0.12 * d },
                soft: if near { 0.3 } else { 0.0 },
                tint,
                shape: if r.unit() < 0.65 { 1.0 } else { 2.0 },
                lands: !near,
                rest: None,
            }
        };
        self.sprites.push(sprite);
    }

    /// Ledge or ground contact of the move `old -> new` (2.2.4 landing).
    /// Highest ledge with room wins; otherwise the ground.
    fn find_hit(&self, old: Vec2, new: Vec2, room_to_rest: bool) -> Option<(SurfaceId, f64, f64)> {
        let mut best: Option<(SurfaceId, f64, f64)> = None;
        if room_to_rest {
            for l in &self.ledges {
                let top = l.frame.max_y();
                if old.y < top || new.y >= top {
                    continue;
                }
                let t = (old.y - top) / (old.y - new.y);
                let x = old.x + t * (new.x - old.x);
                let inset = l.corner_radius * 0.5;
                let inside = x >= l.frame.min_x() + inset && x <= l.frame.max_x() - inset;
                let higher = best.is_none_or(|b| top > b.2);
                if inside && higher && !blocked(&l.blockers, x, top) && self.room_on(l.id) {
                    best = Some((l.id, x, top));
                }
            }
        }
        if best.is_none() && new.y <= GROUND_Y {
            best = Some((SurfaceId::Ground, new.x, GROUND_Y));
        }
        best
    }

    /// One step of a resting sprite. Returns false when it is removed.
    fn lie(&mut self, s: &mut Sprite, rest: Rest, cursor: Option<Vec2>, pointer_vel: Vec2) -> bool {
        if self.clock >= rest.until {
            return false;
        }
        let mut liftable = true;
        if rest.on != SurfaceId::Ground {
            match self.ledge(rest.on).map(|l| l.frame) {
                Some(f) => {
                    s.pos = Vec2::new(f.min_x() + rest.along, f.max_y() + lie_height(s.size, rest.flip));
                }
                None => liftable = false,
            }
        }
        if let Some(c) = cursor.filter(|_| liftable) {
            let radius = self.kind.params().lift_radius;
            if pointer_vel.len() > LIFT_SPEED && c.dist(s.pos) < radius {
                let k = self.rng.range(0.25, 0.5);
                let up = self.rng.range(80.0, 200.0);
                let vel = Vec2::new(pointer_vel.x * k, pointer_vel.y * k + up);
                loosen(s, &mut self.rng, self.slow, vel);
            }
        }
        true
    }

    /// One step of a falling sprite. Returns false when it is removed.
    fn fly(&mut self, s: &mut Sprite, ctx: &Flight) -> bool {
        let dt = ctx.dt;
        s.tumble += s.tumble_rate * dt;
        s.sway_phase += s.sway_rate * dt;
        s.angle += s.spin * dt;
        let a = s.tumble.cos().abs();
        let side_bias = (2.0 * s.tumble).sin() * s.sway * 0.35;
        let target =
            Vec2::new(ctx.wind * s.drift + s.sway * s.sway_phase.cos() + side_bias, -s.fall * (0.6 + 0.7 * (1.0 - a)));
        let k = (2.2 * dt).min(1.0);
        s.vel.x += (target.x - s.vel.x) * k;
        s.vel.y += (target.y - s.vel.y) * k;
        if let Some(c) = ctx.cursor {
            let dist = c.dist(s.pos);
            if ctx.pointer_vel.len() > 20.0 && dist < WAKE_RADIUS {
                let f = 1.0 - dist / WAKE_RADIUS;
                s.vel.x += ctx.pointer_vel.x * f * 2.5 * dt;
                s.vel.y += ctx.pointer_vel.y * f * 2.5 * dt;
                s.spin += f * self.rng.range(-4.0, 4.0) * dt;
            }
        }
        let old = s.pos;
        s.pos = Vec2::new(s.pos.x + s.vel.x * dt, s.pos.y + s.vel.y * dt);
        let hit = if s.lands && s.vel.y < 0.0 { self.find_hit(old, s.pos, ctx.room_to_rest) } else { None };
        if let Some((id, x, y)) = hit {
            let room = id != SurfaceId::Ground || (ctx.room_to_rest && self.room_on(id));
            if room {
                self.settle(s, id, x, y);
            }
            return room;
        }
        let span = self.width + 2.0 * EDGE_MARGIN;
        if s.pos.x < -EDGE_MARGIN {
            s.pos.x += span;
        } else if s.pos.x > self.width + EDGE_MARGIN {
            s.pos.x -= span;
        }
        s.pos.y > -30.0
    }

    /// Lay a sprite down on its side at the contact point.
    fn settle(&mut self, s: &mut Sprite, id: SurfaceId, x: f64, y: f64) {
        let p = self.kind.params();
        let flip = self.rng.range(0.28, 0.5) * random_sign(&mut self.rng);
        s.pos = Vec2::new(x, y + lie_height(s.size, flip));
        s.angle = random_sign(&mut self.rng) * FRAC_PI_2 + self.rng.range(-0.2, 0.2);
        let along = match self.ledge(id) {
            Some(l) => x - l.frame.min_x(),
            None => x,
        };
        let rest_time = self.rng.range(p.rest_lo, p.rest_hi);
        s.rest = Some(Rest { on: id, along, flip, until: self.clock + rest_time + FADE_TIME });
    }

    /// Loosen every sprite resting on `id` that passes `pick`, with a velocity from `vel`.
    fn release_where(&mut self, id: SurfaceId, pick: impl Fn(&Rest) -> bool, vel: impl Fn(&mut Lcg64) -> Vec2) {
        let slow = self.slow;
        for s in &mut self.sprites {
            if s.rest.is_some_and(|r| r.on == id && pick(&r)) {
                let v = vel(&mut self.rng);
                loosen(s, &mut self.rng, slow, v);
            }
        }
    }
}

impl Falling for PetalField {
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
        // No-op for dt <= 0; also rejects NaN and infinity so the clock stays finite.
        if !(dt > 0.0 && dt.is_finite()) {
            return;
        }
        self.clock += dt;
        let c = self.clock;
        let pointer_vel = match (cursor, self.last_cursor) {
            (Some(now), Some(prev)) => Vec2::new((now.x - prev.x) / dt, (now.y - prev.y) / dt),
            _ => Vec2::ZERO,
        };
        self.blow.step(dt, c, self.wind, self.wind_t.unwrap_or(c));
        let gusts = 18.0
            + 14.0 * (0.06 * c).sin()
            + 8.0 * (0.21 * c + 1.9).sin()
            + 70.0 * (0.09 * c + 0.7).sin().max(0.0).powi(6);
        let wind = gusts * self.slow + self.blow.speed;
        let most = self.kind.params().most_resting;
        let mut seen = 0usize;
        let mut i = self.sprites.len();
        while i > 0 {
            i -= 1;
            let mut s = self.sprites[i].clone();
            let keep = match s.rest {
                Some(rest) => self.lie(&mut s, rest, cursor, pointer_vel),
                None => {
                    let ctx = Flight { dt, wind, cursor, pointer_vel, room_to_rest: seen < most };
                    self.fly(&mut s, &ctx)
                }
            };
            if keep {
                seen += usize::from(s.rest.is_some());
                self.sprites[i] = s;
            } else {
                self.sprites.swap_remove(i);
            }
        }
        self.last_cursor = cursor;
        let missing = self.target().saturating_sub(self.falling()).min(4);
        for _ in 0..missing {
            let y = self.height + self.rng.range(10.0, 80.0);
            self.spawn(y);
        }
    }

    fn update(&mut self, surfaces: &[Surface], interval: f64, to_other_space: bool) {
        let span = interval.max(1.0 / 120.0);
        let new: Vec<Surface> = surfaces.iter().filter(|s| s.id != SurfaceId::Ground).cloned().collect();
        for n in &new {
            let old: Frame = self.ledge(n.id).map_or(n.frame, |o| o.frame);
            let (dx, dy) = (n.frame.min_x() - old.min_x(), n.frame.max_y() - old.max_y());
            let speed = dx.hypot(dy) / span;
            let same_size = old.w == n.frame.w && old.h == n.frame.h;
            if same_size && speed > FLING_MIN && speed < FLING_MAX {
                let (vx, vy) = (dx / span, dy / span);
                self.release_where(
                    n.id,
                    |_| true,
                    |rng| {
                        let k = rng.range(0.3, 0.6);
                        let jx = rng.range(-40.0, 40.0);
                        let jy = rng.range(60.0, 180.0);
                        Vec2::new(vx * k + jx, vy * k + jy)
                    },
                );
            } else if n.frame.w < old.w {
                let width = n.frame.w;
                self.release_where(n.id, |r| r.along > width, |rng| Vec2::new(rng.range(10.0, 30.0), 0.0));
            }
        }
        let gone: Vec<SurfaceId> =
            self.ledges.iter().map(|l| l.id).filter(|id| !new.iter().any(|n| n.id == *id)).collect();
        for id in gone {
            if to_other_space {
                self.sprites.retain(|s| s.rest.is_none_or(|r| r.on != id));
            } else {
                self.release_where(
                    id,
                    |_| true,
                    |rng| {
                        let x = rng.range(-20.0, 20.0);
                        let y = rng.range(-30.0, 10.0);
                        Vec2::new(x, y)
                    },
                );
            }
        }
        self.ledges = new;
    }

    fn draw(&self, layer: Layer, out: &mut DrawList) {
        if layer != Layer::Main {
            return;
        }
        out.scene[0] = self.width as f32;
        out.scene[1] = self.height as f32;
        out.scene[2] = self.clock as f32;
        let items: Vec<PetalSprite> = self
            .sprites
            .iter()
            .map(|s| {
                let (flip, alpha) = match s.rest {
                    Some(r) => {
                        let fade = ((r.until - self.clock) / FADE_TIME).clamp(0.0, 1.0);
                        let hidden = self.ledge(r.on).is_some_and(|l| blocked(&l.blockers, s.pos.x, s.pos.y));
                        (r.flip, if hidden { 0.0 } else { s.alpha * fade })
                    }
                    None => (s.tumble.cos(), s.alpha),
                };
                PetalSprite {
                    position: [s.pos.x as f32, s.pos.y as f32],
                    size: s.size as f32,
                    angle: s.angle as f32,
                    flip: flip as f32,
                    alpha: alpha as f32,
                    soft: s.soft as f32,
                    tint: s.tint as f32,
                    shape: s.shape as f32,
                    pad: 0.0,
                }
            })
            .collect();
        out.instanced(PassKind::Petals, Vec::new(), &items);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::geom::Rect4;

    fn field(kind: PetalKind, w: f64, h: f64) -> PetalField {
        let mut f = PetalField::new(kind, w, h, false, 7);
        f.sprites.clear();
        f.set_share(0.0);
        f
    }

    fn probe(x: f64, y: f64) -> Sprite {
        Sprite {
            pos: Vec2::new(x, y),
            vel: Vec2::new(0.0, -60.0),
            size: 10.0,
            angle: 0.0,
            spin: 0.0,
            tumble: 0.0,
            tumble_rate: 0.0,
            sway_phase: 0.0,
            sway: 0.0,
            sway_rate: 0.0,
            fall: 100.0,
            drift: 0.0,
            alpha: 1.0,
            soft: 0.0,
            tint: 0.5,
            shape: 0.0,
            lands: true,
            rest: None,
        }
    }

    fn resting(f: &PetalField, x: f64, y: f64, on: SurfaceId, along: f64, life: f64) -> Sprite {
        let mut s = probe(x, y);
        s.rest = Some(Rest { on, along, flip: 0.4, until: f.clock + life });
        s
    }

    fn sprite_from_bytes(b: &[u8]) -> PetalSprite {
        let f = |i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        PetalSprite {
            position: [f(0), f(1)],
            size: f(2),
            angle: f(3),
            flip: f(4),
            alpha: f(5),
            soft: f(6),
            tint: f(7),
            shape: f(8),
            pad: f(9),
        }
    }

    fn ledge(id: i64, x: f64, y: f64, w: f64, h: f64) -> Surface {
        Surface::new(SurfaceId::Window(id), Frame::new(x, y, w, h))
    }

    #[test]
    fn construction_and_accessors() {
        let mut f = PetalField::new(PetalKind::Sakura, 1920.0, 1080.0, false, 1);
        assert_eq!(f.sprites.len(), 186);
        assert_eq!(f.slow, 1.0);
        assert!((0.0..1000.0).contains(&f.clock));
        assert!(f.sprites.iter().all(|s| s.shape == 0.0 && s.pos.y <= 1080.0 * 1.1));
        let l = PetalField::new(PetalKind::Leaves, 1920.0, 1080.0, true, 1);
        assert_eq!(l.sprites.len(), 114);
        assert_eq!(l.slow, 0.5);
        f.set_share(2.0);
        assert_eq!(f.share(), 2.0);
        assert_eq!(f.target(), 373);
        f.set_share(-1.0);
        assert_eq!(f.target(), 0);
        f.set_wind(Wind::StrongRight);
        assert_eq!(f.wind, Wind::StrongRight);
        assert!(!f.draws_above());
    }

    #[test]
    fn spawn_ranges_for_both_kinds() {
        for (kind, near_size) in [(PetalKind::Sakura, (12.0, 15.0)), (PetalKind::Leaves, (22.0, 28.0))] {
            let mut f = field(kind, 1000.0, 1000.0);
            for _ in 0..600 {
                f.spawn(500.0);
            }
            let near: Vec<_> = f.sprites.iter().filter(|s| !s.lands).collect();
            assert!(!near.is_empty() && near.len() < 60);
            for s in &near {
                assert!(s.size >= near_size.0 && s.size < near_size.1);
                assert!(s.soft > 0.0 && s.drift == 1.3);
            }
            for s in f.sprites.iter().filter(|s| s.lands) {
                assert_eq!(s.soft, 0.0);
                assert!(s.pos.x >= -40.0 && s.pos.x < 1040.0 && s.rest.is_none());
            }
            if kind == PetalKind::Leaves {
                assert!(f.sprites.iter().any(|s| s.shape == 1.0));
                assert!(f.sprites.iter().any(|s| s.shape == 2.0));
            }
        }
    }

    #[test]
    fn spawn_constants_per_kind() {
        // (kind, start vy, far size, far fall, far alpha min, near alpha, near fall)
        let cases = [
            (PetalKind::Sakura, -40.0, (4.0, 9.0), (26.0, 74.0), 0.75, 0.6, (90.0, 120.0)),
            (PetalKind::Leaves, -50.0, (8.5, 16.5), (38.0, 96.0), 0.88, 0.7, (110.0, 140.0)),
        ];
        for (kind, vy, size, fall, alpha_min, near_alpha, near_fall) in cases {
            for slow in [false, true] {
                let mut f = PetalField::new(kind, 1000.0, 1000.0, slow, 11);
                f.sprites.clear();
                for _ in 0..2000 {
                    f.spawn(500.0);
                }
                let k = if slow { 0.5 } else { 1.0 };
                for s in &f.sprites {
                    assert_eq!(s.vel, Vec2::new(0.0, vy));
                    assert!(s.alpha <= 1.0 + 1e-12);
                    if s.lands {
                        assert!(s.size >= size.0 && s.size < size.1, "{}", s.size);
                        assert!(s.fall >= fall.0 * k && s.fall < fall.1 * k, "{}", s.fall);
                        assert!(s.alpha >= alpha_min);
                        assert!(s.drift >= 0.4 && s.drift <= 1.0);
                    } else {
                        assert_eq!(s.alpha, near_alpha);
                        assert!(s.fall >= near_fall.0 * k && s.fall < near_fall.1 * k);
                    }
                }
            }
        }
        let mut f = field(PetalKind::Leaves, 1000.0, 1000.0);
        for _ in 0..2000 {
            f.spawn(0.0);
        }
        let maple = f.sprites.iter().filter(|s| s.shape == 1.0).count() as f64 / 2000.0;
        assert!((0.6..0.7).contains(&maple), "{maple}");
    }

    #[test]
    fn ambient_wind_formula() {
        for slow in [false, true] {
            let mut f = PetalField::new(PetalKind::Sakura, 400.0, 600.0, slow, 5);
            f.sprites.clear();
            f.set_share(0.0);
            let mut s = probe(100.0, 500.0);
            s.drift = 1.0;
            f.sprites = vec![s];
            // dt 0.5 makes the velocity blend factor min(1, 2.2 dt) = 1
            f.step(0.5, None);
            let c = f.clock;
            let gusts = 18.0
                + 14.0 * (0.06 * c).sin()
                + 8.0 * (0.21 * c + 1.9).sin()
                + 70.0 * (0.09 * c + 0.7).sin().max(0.0).powi(6);
            let k = if slow { 0.5 } else { 1.0 };
            let v = f.sprites[0].vel;
            assert!((v.x - gusts * k).abs() < 1e-9, "{} vs {}", v.x, gusts * k);
            // face-on (|cos tumble| = 1): falls at 0.6 * fall
            assert!((v.y + 60.0).abs() < 1e-9);
        }
    }

    #[test]
    fn rest_duration_per_kind() {
        for (kind, lo, hi) in [(PetalKind::Sakura, 6.0, 16.0), (PetalKind::Leaves, 14.0, 30.0)] {
            let mut f = field(kind, 400.0, 600.0);
            for _ in 0..500 {
                let mut s = probe(50.0, 3.0);
                f.settle(&mut s, SurfaceId::Ground, 50.0, 2.0);
                let r = s.rest.unwrap();
                let life = r.until - f.clock;
                assert!(life >= lo + 1.5 && life < hi + 1.5, "{life}");
                let side = (s.angle.abs() - FRAC_PI_2).abs();
                assert!(side <= 0.2 + 1e-12);
            }
        }
    }

    #[test]
    fn lift_radius_per_kind() {
        for (kind, lifted) in [(PetalKind::Sakura, false), (PetalKind::Leaves, true)] {
            let mut f = field(kind, 400.0, 600.0);
            f.sprites = vec![resting(&f, 100.0, 2.0, SurfaceId::Ground, 100.0, 50.0)];
            // pointer passes 30 pt above the sprite at 1000 pt/s
            f.step(0.01, Some(Vec2::new(90.0, 32.0)));
            f.step(0.01, Some(Vec2::new(100.0, 32.0)));
            assert_eq!(f.sprites[0].rest.is_none(), lifted);
        }
    }

    #[test]
    fn step_ignores_non_positive_dt_and_tops_up() {
        let mut f = PetalField::new(PetalKind::Sakura, 1920.0, 1080.0, false, 3);
        let c = f.clock;
        f.step(0.0, None);
        f.step(-1.0, None);
        f.step(f64::NAN, None);
        f.step(f64::INFINITY, None);
        assert_eq!(f.clock, c);
        f.set_share(2.0);
        let n = f.sprites.len();
        f.step(1.0 / 60.0, None);
        assert_eq!(f.sprites.len(), n + 4);
        assert!(f.sprites.iter().skip(n).all(|s| s.pos.y > 1080.0));
    }

    #[test]
    fn flight_wrap_and_removal() {
        let mut f = field(PetalKind::Sakura, 200.0, 600.0);
        let mut a = probe(-39.9, 500.0);
        a.vel.x = -100.0;
        let mut b = probe(239.9, 500.0);
        b.vel.x = 100.0;
        let mut c = probe(100.0, -29.0);
        c.lands = false;
        f.sprites = vec![a, b, c];
        f.step(0.1, None);
        assert_eq!(f.sprites.len(), 2);
        assert!(f.sprites.iter().any(|s| s.pos.x > 200.0));
        assert!(f.sprites.iter().any(|s| s.pos.x < 0.0 && s.pos.x > -40.0));
    }

    #[test]
    fn lands_on_ledge_and_follows_it() {
        let mut f = field(PetalKind::Sakura, 400.0, 600.0);
        f.update(&[ledge(1, 50.0, 100.0, 100.0, 20.0)], 0.1, false);
        f.sprites = vec![probe(100.0, 123.0)];
        f.step(0.1, None);
        let r = f.sprites[0].rest.expect("rested");
        assert_eq!(r.on, SurfaceId::Window(1));
        assert!((r.along - 50.0).abs() < 1e-9);
        assert!((0.28..0.5).contains(&r.flip.abs()));
        assert!(f.sprites[0].pos.y > 120.0);
        f.update(&[ledge(1, 60.0, 110.0, 100.0, 20.0)], 1.0, false);
        f.step(0.01, None);
        let s = &f.sprites[0];
        assert!((s.pos.x - 110.0).abs() < 1e-9);
        assert!(s.pos.y > 130.0);
        assert!(s.rest.is_some());
    }

    #[test]
    fn find_hit_rules() {
        let mut f = field(PetalKind::Sakura, 400.0, 600.0);
        let mut a = ledge(1, 50.0, 100.0, 100.0, 20.0);
        a.corner_radius = 20.0;
        a.blockers.push(Rect4::new(60.0, 119.0, 70.0, 121.0));
        f.update(&[a.clone()], 0.1, false);
        let down = |x: f64, from: f64, to: f64| f.find_hit(Vec2::new(x, from), Vec2::new(x, to), true);
        assert!(down(100.0, 123.0, 117.0).is_some());
        assert!(down(55.0, 123.0, 117.0).is_none());
        assert!(down(145.0, 123.0, 117.0).is_none());
        assert!(down(65.0, 123.0, 117.0).is_none());
        assert!(down(100.0, 110.0, 100.0).is_none());
        assert!(down(100.0, 130.0, 125.0).is_none());
        assert!(f.find_hit(Vec2::new(100.0, 123.0), Vec2::new(100.0, 117.0), false).is_none());
        let g = f.find_hit(Vec2::new(10.0, 5.0), Vec2::new(12.0, 1.0), true).unwrap();
        assert_eq!((g.0, g.1, g.2), (SurfaceId::Ground, 12.0, 2.0));
        let b = ledge(2, 0.0, 190.0, 200.0, 10.0);
        for order in [[a.clone(), b.clone()], [b.clone(), a.clone()]] {
            f.update(&order, 0.1, false);
            let h = f.find_hit(Vec2::new(100.0, 210.0), Vec2::new(100.0, 110.0), true).unwrap();
            assert_eq!(h.0, SurfaceId::Window(2));
        }
        f.update(&[a.clone(), ledge(3, 0.0, 190.0, 40.0, 10.0)], 0.1, false);
        f.sprites.push(resting(&f, 0.0, 0.0, SurfaceId::Window(3), 0.0, 9.0));
        let h = f.find_hit(Vec2::new(20.0, 210.0), Vec2::new(20.0, 110.0), true);
        assert!(h.is_none());
        let h = f.find_hit(Vec2::new(100.0, 210.0), Vec2::new(100.0, 110.0), true).unwrap();
        assert_eq!(h.0, SurfaceId::Window(1));
    }

    #[test]
    fn ground_rules() {
        let mut f = field(PetalKind::Sakura, 200.0, 600.0);
        f.sprites = vec![probe(100.0, 3.0)];
        f.step(0.1, None);
        let r = f.sprites[0].rest.unwrap();
        assert_eq!(r.on, SurfaceId::Ground);
        assert!((r.along - 100.0).abs() < 1e-9);
        assert!((f.sprites[0].pos.y - (2.0 + lie_height(10.0, r.flip))).abs() < 1e-9);
        // ground full (200 / 45 = 4.4 -> 5 slots)
        let mut f = field(PetalKind::Sakura, 200.0, 600.0);
        for i in 0..5 {
            let s = resting(&f, 10.0 * i as f64, 2.0, SurfaceId::Ground, 0.0, 50.0);
            f.sprites.push(s);
        }
        f.sprites.insert(0, probe(100.0, 3.0));
        f.step(0.1, None);
        assert_eq!(f.sprites.len(), 5);
        // global cap: 160 resting seen before the falling one
        let mut f = field(PetalKind::Sakura, 100_000.0, 600.0);
        for i in 0..160 {
            let s = resting(&f, i as f64, 2.0, SurfaceId::Ground, 0.0, 50.0);
            f.sprites.push(s);
        }
        f.sprites.insert(0, probe(5000.0, 3.0));
        f.step(0.1, None);
        assert_eq!(f.sprites.len(), 160);
        // at the cap ledges are skipped too: the sprite falls through
        let mut f = field(PetalKind::Leaves, 100_000.0, 600.0);
        f.update(&[ledge(1, 0.0, 100.0, 1000.0, 20.0)], 0.1, false);
        for i in 0..260 {
            let s = resting(&f, i as f64, 2.0, SurfaceId::Ground, 0.0, 50.0);
            f.sprites.push(s);
        }
        f.sprites.insert(0, probe(100.0, 123.0));
        f.step(0.1, None);
        assert!(f.sprites[0].rest.is_none());
    }

    #[test]
    fn resting_lifecycle() {
        let mut f = field(PetalKind::Sakura, 400.0, 600.0);
        let s = resting(&f, 10.0, 2.0, SurfaceId::Ground, 10.0, 0.05);
        f.sprites = vec![s];
        f.step(0.1, None);
        assert!(f.sprites.is_empty());
        // orphan ledge: stays put and cannot be lifted
        let s = resting(&f, 77.0, 88.0, SurfaceId::Window(9), 5.0, 50.0);
        f.sprites = vec![s];
        f.step(0.01, Some(Vec2::new(70.0, 88.0)));
        f.step(0.01, Some(Vec2::new(77.0, 88.0)));
        assert_eq!(f.sprites[0].pos, Vec2::new(77.0, 88.0));
        assert!(f.sprites[0].rest.is_some());
        // ground sprite: slow pointer keeps it, fast pointer lifts it
        let s = resting(&f, 100.0, 2.0, SurfaceId::Ground, 100.0, 50.0);
        f.sprites = vec![s];
        f.step(0.1, Some(Vec2::new(99.0, 2.0)));
        f.step(0.1, Some(Vec2::new(100.0, 2.0)));
        assert!(f.sprites[0].rest.is_some());
        f.step(0.01, Some(Vec2::new(103.0, 2.0)));
        assert!(f.sprites[0].rest.is_none());
        assert!(f.sprites[0].vel.y >= 80.0 - 1e-9);
        // fast pointer but too far away
        let s = resting(&f, 300.0, 2.0, SurfaceId::Ground, 300.0, 50.0);
        f.sprites = vec![s];
        f.step(0.01, Some(Vec2::new(10.0, 2.0)));
        f.step(0.01, Some(Vec2::new(20.0, 2.0)));
        assert!(f.sprites[0].rest.is_some());
    }

    #[test]
    fn pointer_wake() {
        let mut f = field(PetalKind::Leaves, 400.0, 600.0);
        f.sprites = vec![probe(100.0, 300.0), probe(300.0, 300.0)];
        f.step(0.1, Some(Vec2::new(95.0, 300.0)));
        let before = f.sprites[0].vel;
        f.step(0.1, Some(Vec2::new(105.0, 300.0)));
        let after = f.sprites[0].vel;
        assert!(after.x > before.x + 10.0);
        assert!(f.sprites[0].spin != 0.0);
        assert_eq!(f.sprites[1].spin, 0.0);
        let spin = f.sprites[1].spin;
        f.step(0.1, Some(Vec2::new(105.0, 300.0)));
        assert_eq!(f.sprites[1].spin, spin);
    }

    #[test]
    fn surface_fling_narrow_and_removal() {
        let mut f = field(PetalKind::Sakura, 800.0, 600.0);
        let both = |x1: f64| [ledge(1, x1, 100.0, 100.0, 20.0), ledge(2, 300.0, 100.0, 100.0, 20.0)];
        f.update(&both(0.0), 0.1, false);
        let on1 = resting(&f, 10.0, 125.0, SurfaceId::Window(1), 10.0, 50.0);
        let on1b = resting(&f, 90.0, 125.0, SurfaceId::Window(1), 90.0, 50.0);
        let on2 = resting(&f, 310.0, 125.0, SurfaceId::Window(2), 10.0, 50.0);
        f.sprites = vec![on1, on1b, on2, probe(5.0, 400.0)];
        f.update(&both(10.0), 0.1, false);
        assert!(f.sprites.iter().take(3).all(|s| s.rest.is_some()));
        f.update(&both(5000.0), 0.1, false);
        assert!(f.sprites.iter().take(3).all(|s| s.rest.is_some()));
        f.update(&both(5100.0), 0.1, false);
        assert!(f.sprites[0].rest.is_none() && f.sprites[1].rest.is_none());
        assert!(f.sprites[2].rest.is_some());
        // window speed 100 / 0.1 = 1000 pt/s: v = 1000 * rand(0.3,0.6) + rand(-40,40), up rand(60,180)
        for s in f.sprites.iter().take(2) {
            assert!((260.0..640.0).contains(&s.vel.x), "{}", s.vel.x);
            assert!((60.0..180.0).contains(&s.vel.y), "{}", s.vel.y);
            assert!((-5.0..5.0).contains(&s.spin));
        }
        f.update(&both(5100.0), 0.0, false);
        let near = resting(&f, 5110.0, 125.0, SurfaceId::Window(1), 10.0, 50.0);
        let far = resting(&f, 5190.0, 125.0, SurfaceId::Window(1), 90.0, 50.0);
        f.sprites = vec![near, far];
        f.update(&[ledge(1, 5100.0, 100.0, 50.0, 20.0)], 0.1, false);
        assert!(f.sprites[0].rest.is_some());
        assert!(f.sprites[1].rest.is_none());
        assert!((10.0..30.0).contains(&f.sprites[1].vel.x) && f.sprites[1].vel.y == 0.0);
        f.update(&[Surface::new(SurfaceId::Ground, Frame::new(0.0, 0.0, 800.0, 2.0))], 0.1, false);
        assert!(f.ledges.is_empty());
    }

    #[test]
    fn removed_surface_releases_or_deletes() {
        for to_other in [false, true] {
            let mut f = field(PetalKind::Sakura, 800.0, 600.0);
            f.update(&[ledge(1, 0.0, 100.0, 100.0, 20.0)], 0.1, false);
            let a = resting(&f, 10.0, 125.0, SurfaceId::Window(1), 10.0, 50.0);
            let g = resting(&f, 10.0, 2.0, SurfaceId::Ground, 10.0, 50.0);
            f.sprites = vec![a, g, probe(0.0, 300.0)];
            f.update(&[], 0.1, to_other);
            assert!(f.ledges.is_empty());
            if to_other {
                assert_eq!(f.sprites.len(), 2);
            } else {
                assert_eq!(f.sprites.len(), 3);
                assert!(f.sprites[0].rest.is_none() && f.sprites[1].rest.is_some());
                let v = f.sprites[0].vel;
                assert!((-20.0..20.0).contains(&v.x) && (-30.0..10.0).contains(&v.y));
            }
        }
    }

    #[test]
    fn draw_data() {
        let mut f = field(PetalKind::Leaves, 400.0, 600.0);
        let mut out = DrawList::new(1.0, 1.0, 0.0);
        f.draw(Layer::Main, &mut out);
        assert!(out.is_empty());
        let mut blocked_ledge = ledge(1, 0.0, 100.0, 100.0, 20.0);
        blocked_ledge.blockers.push(Rect4::new(0.0, 0.0, 50.0, 500.0));
        f.update(&[blocked_ledge], 0.1, false);
        let mut fall = probe(10.0, 300.0);
        fall.shape = 2.0;
        let faded = resting(&f, 80.0, 125.0, SurfaceId::Window(1), 80.0, 0.75);
        let hidden = resting(&f, 20.0, 125.0, SurfaceId::Window(1), 20.0, 50.0);
        let grounded = resting(&f, 200.0, 2.0, SurfaceId::Ground, 200.0, 50.0);
        f.sprites = vec![fall, faded, hidden, grounded];
        f.draw(Layer::Above, &mut out);
        assert!(out.is_empty());
        f.draw(Layer::Main, &mut out);
        assert_eq!(out.passes.len(), 1);
        let p = &out.passes[0];
        assert_eq!((p.kind, p.instances, p.vertices), (PassKind::Petals, 4, 4));
        assert!(p.uniforms.is_empty());
        assert_eq!(out.scene[0], 400.0);
        assert_eq!(out.scene[1], 600.0);
        assert_eq!(out.scene[2], f.clock as f32);
        let items: Vec<PetalSprite> = p.buffers[0].chunks(40).map(sprite_from_bytes).collect();
        assert_eq!(items[0].alpha, 1.0);
        assert_eq!(items[0].flip, 1.0);
        assert_eq!(items[0].shape, 2.0);
        assert!((items[1].alpha - 0.5).abs() < 1e-6);
        assert_eq!(items[1].flip, 0.4);
        assert_eq!(items[2].alpha, 0.0);
        assert_eq!(items[3].alpha, 1.0);
    }

    #[test]
    fn long_run_is_stable_and_deterministic() {
        let run = |kind: PetalKind, seed: u64| {
            let mut f = PetalField::new(kind, 800.0, 500.0, false, seed);
            f.set_wind(Wind::Changing);
            let mut surfaces = vec![ledge(1, 100.0, 200.0, 300.0, 30.0), ledge(2, 500.0, 120.0, 200.0, 30.0)];
            for i in 0..4000 {
                let t = i as f64 / 60.0;
                if i % 10 == 0 {
                    surfaces[0].frame.x = 100.0 + 15.0 * (t * 0.5).sin();
                    f.update(&surfaces, 1.0 / 6.0, false);
                }
                let cursor =
                    (i % 7 != 0).then(|| Vec2::new(400.0 + 350.0 * (t * 3.0).sin(), 200.0 + 150.0 * (t * 2.1).cos()));
                f.step(1.0 / 60.0, cursor);
            }
            let mut out = DrawList::new(800.0, 500.0, 0.0);
            f.draw(Layer::Main, &mut out);
            (f.sprites.len(), out)
        };
        for kind in [PetalKind::Sakura, PetalKind::Leaves] {
            let (n, a) = run(kind, 5);
            let (m, b) = run(kind, 5);
            assert_eq!((n, &a), (m, &b));
            assert!(n > 0);
        }
    }

    #[test]
    fn changing_wind_follows_the_wall_clock() {
        use fp_core::falling::Falling;
        let run = |t: Option<f64>| {
            let mut f = PetalField::new(PetalKind::Sakura, 800.0, 500.0, false, 7);
            f.set_wind(fp_core::wind::Wind::Changing);
            f.set_wind_time(f64::NAN);
            assert_eq!(f.wind_t, None);
            if let Some(t) = t {
                f.set_wind_time(t);
                assert_eq!(f.wind_t, Some(t));
            }
            for _ in 0..30 {
                f.step(0.05, None);
            }
            f.blow.steady
        };
        // Two fields given the same wall time agree; the time base matters.
        assert_eq!(run(Some(40.0)), run(Some(40.0)));
        assert_ne!(run(Some(40.0)), run(Some(120.0)));
        assert!(run(None).is_finite());
    }
}
