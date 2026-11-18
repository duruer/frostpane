//! Falling snow clumps shaken off trees, and the per-monitor overlay sprites
//! that carry them (trees 3.8).

use crate::TreeEvent;
use crate::shelf::{Shelf, min_key};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng};
use fp_core::scene::{HitMode, Owner, Sprite, SpriteKey, ZBand};
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, rgba};

/// Most clumps alive at once; spawns beyond are ignored.
pub const MAX_CLUMPS: usize = 90;
/// Sprite ids from here up belong to clump overlays (tree ids stay below).
pub const CLUMP_BASE: u64 = 1 << 48;
const GRAVITY: f64 = 1500.0;

/// A request to throw a clump.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn {
    pub point: Vec2,
    pub vel: Vec2,
    pub radius: f64,
}

#[derive(Clone, Debug)]
struct Clump {
    pos: Vec2,
    vel: Vec2,
    radius: f64,
    angle: f64,
    spin: f64,
    age: f64,
    breaks_at: Option<f64>,
    fade: Option<f64>,
    look: u8,
    overlay: usize,
}

#[derive(Clone, Debug)]
struct Overlay {
    origin: Vec2,
    screen: Frame,
    /// `(x0, x1, top)`; the area only grows.
    area: Option<(f64, f64, f64)>,
    rev: u64,
}

#[derive(Debug, Default)]
pub struct Clumps {
    list: Vec<Clump>,
    overlays: Vec<Overlay>,
}

impl Clumps {
    pub fn count(&self) -> usize {
        self.list.len()
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.overlays.clear();
    }

    /// Throws a clump from `s` (no-op without a ground shelf or at the cap).
    pub fn spawn(&mut self, shelves: &[Shelf], rng: &mut Lcg64, s: Spawn) {
        if self.list.len() >= MAX_CLUMPS {
            return;
        }
        let ground = || shelves.iter().filter(|h| h.ground);
        let Some(shelf) =
            ground().find(|h| h.spans(s.point.x)).or_else(|| min_key(ground(), |h| (h.middle - s.point.x).abs()))
        else {
            return;
        };
        let overlay = match self.overlays.iter().position(|o| o.origin == shelf.key.origin) {
            Some(i) => i,
            None => {
                self.overlays.push(Overlay { origin: shelf.key.origin, screen: shelf.screen, area: None, rev: 0 });
                self.overlays.len() - 1
            }
        };
        let vx = s.vel.x * rng.range(0.28, 0.62) + rng.range(-60.0, 60.0);
        let vy = s.vel.y * rng.range(0.15, 0.4) + rng.range(-30.0, 70.0);
        let spin = rng.range(-5.0, 5.0);
        let angle = rng.range(0.0, std::f64::consts::TAU);
        let breaks_at = (s.radius > 6.0).then(|| rng.range(0.12, 0.38));
        let look = rng.below(4) as u8;
        self.list.push(Clump {
            pos: s.point,
            vel: Vec2::new(vx, vy),
            radius: s.radius,
            angle,
            spin,
            age: 0.0,
            breaks_at,
            fade: None,
            look,
            overlay,
        });
    }

    fn overlay_at(&self, p: Vec2) -> usize {
        self.overlays.iter().position(|o| o.screen.contains(p)).unwrap_or(0)
    }

    fn births(&self, rng: &mut Lcg64, parent: &Clump, out: &mut Vec<Clump>) {
        for _ in 0..rng.whole(2, 4) {
            let vel = parent.vel + Vec2::new(rng.range(-80.0, 80.0), rng.range(-20.0, 40.0));
            let radius = parent.radius * rng.range(0.5, 0.72);
            let spin = rng.range(-8.0, 8.0);
            let angle = rng.range(0.0, std::f64::consts::TAU);
            let breaks_at = (radius > 8.0).then(|| rng.range(0.15, 0.4));
            let look = rng.below(4) as u8;
            let overlay = self.overlay_at(parent.pos);
            out.push(Clump {
                pos: parent.pos,
                vel,
                radius,
                angle,
                spin,
                age: 0.0,
                breaks_at,
                fade: None,
                look,
                overlay,
            });
        }
        for _ in 0..rng.whole(2, 5) {
            let vel = parent.vel + Vec2::new(rng.range(-110.0, 110.0), rng.range(-30.0, 70.0));
            let radius = rng.range(1.4, 2.8);
            let fade = Some(rng.range(0.35, 0.8));
            let spin = rng.range(-8.0, 8.0);
            let look = rng.below(4) as u8;
            let overlay = self.overlay_at(parent.pos);
            out.push(Clump {
                pos: parent.pos,
                vel,
                radius,
                angle: 0.0,
                spin,
                age: 0.0,
                breaks_at: None,
                fade,
                look,
                overlay,
            });
        }
    }

    /// Physics, landing bursts, breaking and overlay growth for one frame.
    pub fn step(&mut self, shelves: &[Shelf], rng: &mut Lcg64, dt: f64, events: &mut Vec<TreeEvent>) {
        let mut born = Vec::new();
        let mut kept = Vec::with_capacity(self.list.len());
        for mut c in std::mem::take(&mut self.list) {
            let old = c.pos;
            c.age += dt;
            c.vel.y -= GRAVITY * dt;
            c.vel.x *= (-1.3 * dt).exp();
            c.pos = c.pos + c.vel * dt;
            c.angle += c.spin * dt;
            if c.fade.is_some_and(|f| c.age >= f) {
                continue;
            }
            let lands = |h: &&Shelf| {
                h.shown && h.spans(c.pos.x) && old.y >= h.top && c.pos.y <= h.top && !h.blocked_at(c.pos.x)
            };
            if shelves.iter().any(|h| lands(&h)) {
                if c.radius >= 5.0 {
                    events.push(TreeEvent::Snow { at: c.pos, area: 3.0, velocity: Vec2::new(c.vel.x, 0.0) });
                }
                continue;
            }
            if c.pos.y < self.overlays[c.overlay].screen.min_y() - 30.0 {
                continue;
            }
            if c.breaks_at.is_some_and(|b| c.age >= b) {
                self.births(rng, &c, &mut born);
                continue;
            }
            kept.push(c);
        }
        self.list = kept;
        for b in born {
            if self.list.len() < MAX_CLUMPS {
                self.list.push(b);
            }
        }
        if self.list.is_empty() {
            self.overlays.clear();
            return;
        }
        for i in 0..self.overlays.len() {
            let mut ext: Option<(f64, f64, f64)> = None;
            for c in self.list.iter().filter(|c| c.overlay == i) {
                let r = c.radius * 1.2;
                let (lo, hi, top) = ext.unwrap_or((c.pos.x - r, c.pos.x + r, c.pos.y + r));
                ext = Some((lo.min(c.pos.x - r), hi.max(c.pos.x + r), top.max(c.pos.y + r)));
            }
            if let Some(e) = ext {
                self.grow_overlay(i, e);
            }
        }
    }

    fn grow_overlay(&mut self, i: usize, (lo, hi, top): (f64, f64, f64)) {
        let o = &mut self.overlays[i];
        o.rev += 1;
        let s = o.screen;
        let want = ((lo - 40.0).max(s.min_x()), (hi + 40.0).min(s.max_x()), (top + 40.0).min(s.max_y()));
        if let Some((a0, a1, at)) = o.area
            && a0 <= want.0
            && a1 >= want.1
            && at >= want.2
        {
            return;
        }
        let (a0, a1, at) = o.area.map_or(want, |(a0, a1, at)| (a0.min(want.0), a1.max(want.1), at.max(want.2)));
        o.area = Some(((a0 - 120.0).max(s.min_x()), (a1 + 120.0).min(s.max_x()), (at + 120.0).min(s.max_y())));
    }

    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        for (i, o) in self.overlays.iter().enumerate() {
            if let Some((x0, x1, top)) = o.area {
                out.push(Sprite {
                    key: SpriteKey::new(Owner::Tree, CLUMP_BASE + i as u64),
                    frame: Frame::new(x0, o.screen.min_y(), x1 - x0, top - o.screen.min_y()),
                    z: ZBand::Front,
                    hit: HitMode::Never,
                    rev: o.rev,
                    opacity: 1.0,
                });
            }
        }
    }

    /// Draws overlay `index` (sprite id minus [`CLUMP_BASE`]).
    pub fn paint(&self, index: usize, scale: f32) -> Option<Pixmap> {
        let o = self.overlays.get(index)?;
        let (x0, x1, top) = o.area?;
        let (w, h) = ((x1 - x0) * f64::from(scale), (top - o.screen.min_y()) * f64::from(scale));
        let mut pm = Pixmap::new(w.ceil() as u32, h.ceil() as u32)?;
        let mut sk = Sketch::new(&mut pm, scale);
        for c in self.list.iter().filter(|c| c.overlay == index) {
            let opacity = c.fade.map_or(1.0, |f| (1.0 - c.age / f).max(0.0));
            let s = (c.radius * 2.4 / 48.0) as f32;
            let at = ((c.pos.x - x0) as f32, (c.pos.y - o.screen.min_y()) as f32);
            sk.at(at.0, at.1, c.angle as f32, s, |sk| draw_blob(sk, c.look, opacity));
        }
        drop(sk);
        Some(pm)
    }
}

/// One clump image in 48-unit space centred on the origin: a soft shadow,
/// a shaded pass and a lifted white pass over the same seven circles.
fn draw_blob(sk: &mut Sketch, look: u8, opacity: f64) {
    let circles = || {
        let mut rng = Lcg64::new(u64::from(look) * 97 + 3 + 5);
        (0..7)
            .map(|_| {
                let a = rng.range(0.0, std::f64::consts::TAU);
                let d = rng.range(0.05, 0.26) * 48.0;
                let r = rng.range(0.13, 0.22) * 48.0;
                ((a.cos() * d) as f32, (a.sin() * d) as f32, r as f32)
            })
            .collect::<Vec<_>>()
    };
    let lift = 0.03 * 48.0_f32;
    for (x, y, r) in circles() {
        sk.circle(x, y - lift, r, rgba(0.08, 0.14, 0.3, 0.18 * opacity));
    }
    for (x, y, r) in circles() {
        sk.circle(x, y, r, rgba(0.78, 0.85, 0.95, opacity));
    }
    for (x, y, r) in circles() {
        sk.circle(x, y - lift, r * 0.86, rgba(0.99, 0.995, 1.0, opacity));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf::Ledges;
    use fp_core::surface::{ScreenSnap, Surface, SurfaceId};

    fn shelves(with_window: bool) -> Vec<Shelf> {
        let surfaces = if with_window {
            vec![Surface::new(SurfaceId::Window(3), Frame::new(300.0, 200.0, 200.0, 50.0))]
        } else {
            vec![]
        };
        Ledges::default().build(
            0.0,
            &[ScreenSnap { frame: Frame::new(0.0, 0.0, 1000.0, 700.0), shown: true, ground: true, surfaces }],
        )
    }

    fn sp(x: f64, y: f64, r: f64) -> Spawn {
        Spawn { point: Vec2::new(x, y), vel: Vec2::new(100.0, 0.0), radius: r }
    }

    #[test]
    fn spawn_rules() {
        let mut rng = Lcg64::new(1);
        let mut c = Clumps::default();
        c.spawn(&[], &mut rng, sp(10.0, 100.0, 4.0));
        assert_eq!(c.count(), 0);
        let sh = shelves(false);
        c.spawn(&sh, &mut rng, sp(10.0, 100.0, 4.0));
        c.spawn(&sh, &mut rng, sp(5000.0, 100.0, 8.0));
        assert_eq!(c.count(), 2);
        assert_eq!(c.overlays.len(), 1);
        for _ in 0..200 {
            c.spawn(&sh, &mut rng, sp(10.0, 100.0, 4.0));
        }
        assert_eq!(c.count(), MAX_CLUMPS);
        c.clear();
        assert_eq!((c.count(), c.overlays.len()), (0, 0));
    }

    #[test]
    fn small_clump_lands_with_burst_and_overlay_clears() {
        let mut rng = Lcg64::new(2);
        let sh = shelves(true);
        let mut c = Clumps::default();
        let mut ev = Vec::new();
        c.spawn(&sh, &mut rng, sp(400.0, 300.0, 5.0));
        c.spawn(&sh, &mut rng, sp(100.0, 300.0, 4.0));
        c.list[0].breaks_at = None;
        c.list[1].breaks_at = None;
        let mut sprites = Vec::new();
        for _ in 0..200 {
            c.step(&sh, &mut rng, 0.016, &mut ev);
            if c.count() == 2 {
                sprites.clear();
                c.sprites(&mut sprites);
                assert_eq!(sprites.len(), 1);
            }
        }
        assert_eq!(c.count(), 0);
        assert!(c.overlays.is_empty());
        // the radius 5 one burst, the radius 4 one did not
        assert_eq!(ev.len(), 1);
        assert!(matches!(ev[0], TreeEvent::Snow { area, .. } if area == 3.0));
        // lands on the window top (y 250), not the ground
        let TreeEvent::Snow { at, .. } = ev[0] else { panic!("no burst") };
        assert!(at.y <= 250.0 && at.y > 240.0);
        let TreeEvent::Snow { velocity, .. } = ev[0] else { panic!("no burst") };
        assert_eq!(velocity.y, 0.0);
        assert!(velocity.x != 0.0, "landing keeps horizontal momentum");
    }

    #[test]
    fn blocked_window_top_lets_clumps_through() {
        let mut rng = Lcg64::new(3);
        let mut sh = shelves(true);
        sh[1].blockers.push(fp_core::geom::Rect4::new(0.0, 0.0, 1000.0, 1000.0));
        let mut c = Clumps::default();
        let mut ev = Vec::new();
        c.spawn(&sh, &mut rng, sp(400.0, 300.0, 5.0));
        c.list[0].breaks_at = None;
        for _ in 0..200 {
            c.step(&sh, &mut rng, 0.016, &mut ev);
        }
        let TreeEvent::Snow { at, .. } = ev[0] else { panic!("no burst") };
        assert!(at.y < 20.0);
    }

    #[test]
    fn falls_out_of_the_world() {
        let mut rng = Lcg64::new(4);
        let mut sh = shelves(false);
        sh[0].shown = false;
        let mut c = Clumps::default();
        let mut ev = Vec::new();
        c.spawn(&sh, &mut rng, sp(400.0, 100.0, 4.0));
        for _ in 0..100 {
            c.step(&sh, &mut rng, 0.016, &mut ev);
        }
        assert_eq!(c.count(), 0);
        assert!(ev.is_empty());
    }

    #[test]
    fn big_clump_breaks_into_children_and_puffs() {
        let mut rng = Lcg64::new(5);
        let sh = shelves(false);
        let mut c = Clumps::default();
        let mut ev = Vec::new();
        c.spawn(&sh, &mut rng, sp(400.0, 600.0, 20.0));
        c.list[0].breaks_at = Some(0.05);
        for _ in 0..4 {
            c.step(&sh, &mut rng, 0.016, &mut ev);
        }
        assert!(c.count() >= 4, "{}", c.count());
        assert!(c.list.iter().any(|x| x.fade.is_some()));
        assert!(c.list.iter().any(|x| x.fade.is_none()));
        // everything lands or expires
        for _ in 0..400 {
            c.step(&sh, &mut rng, 0.01, &mut ev);
        }
        assert_eq!(c.count(), 0);
    }

    #[test]
    fn overlay_area_grows_and_paints() {
        let mut rng = Lcg64::new(6);
        let sh = shelves(false);
        let mut c = Clumps::default();
        let mut ev = Vec::new();
        c.spawn(&sh, &mut rng, sp(500.0, 600.0, 8.0));
        c.spawn(&sh, &mut rng, sp(500.0, 600.0, 3.0));
        c.list[0].breaks_at = None;
        c.list[1].fade = Some(0.5);
        c.list[1].breaks_at = None;
        c.step(&sh, &mut rng, 0.016, &mut ev);
        let first = c.overlays[0].area.unwrap();
        for _ in 0..10 {
            c.step(&sh, &mut rng, 0.016, &mut ev);
        }
        let later = c.overlays[0].area.unwrap();
        assert!(later.0 <= first.0 && later.1 >= first.1 && later.2 >= first.2);
        assert!(c.overlays[0].rev >= 11);
        let mut sprites = Vec::new();
        c.sprites(&mut sprites);
        assert_eq!(sprites[0].key.id(), CLUMP_BASE);
        assert_eq!(sprites[0].hit, HitMode::Never);
        let pm = c.paint(0, 1.0).unwrap();
        assert!(pm.data().iter().any(|b| *b != 0));
        assert!(c.paint(1, 1.0).is_none());
        c.overlays[0].area = None;
        assert!(c.paint(0, 1.0).is_none());
    }

    #[test]
    fn child_goes_to_overlay_containing_its_point() {
        let mut c = Clumps::default();
        c.overlays.push(Overlay { origin: Vec2::ZERO, screen: Frame::new(0.0, 0.0, 10.0, 10.0), area: None, rev: 0 });
        c.overlays.push(Overlay {
            origin: Vec2::new(50.0, 0.0),
            screen: Frame::new(50.0, 0.0, 10.0, 10.0),
            area: None,
            rev: 0,
        });
        assert_eq!(c.overlay_at(Vec2::new(55.0, 5.0)), 1);
        assert_eq!(c.overlay_at(Vec2::new(500.0, 5.0)), 0);
    }
}
