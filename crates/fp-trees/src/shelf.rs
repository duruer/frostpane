//! Shelves: the ledges trees stand on (trees 3.5 = snowballs 3.4). Built
//! locally from `ScreenSnap`s so this crate does not depend on fp-snowballs;
//! the rules are identical (ground `Top = MinY + 5`, velocity, `Jumped`).

use fp_core::geom::{Frame, Rect4, Vec2};
use fp_core::surface::{LedgeKey, ScreenSnap, SurfaceId, blocked};

/// Ground height above the monitor bottom.
pub const GROUND_HEIGHT: f64 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    pub key: LedgeKey,
    pub min_x: f64,
    pub max_x: f64,
    pub top: f64,
    pub middle: f64,
    pub vel: Vec2,
    pub jumped: bool,
    /// Global blocker rects.
    pub blockers: Vec<Rect4>,
    pub shown: bool,
    pub ground: bool,
    pub owner: String,
    /// Global frame of the monitor this shelf belongs to.
    pub screen: Frame,
}

impl Shelf {
    /// Blocked at `x`: a blocker contains `(x, top + 1)`.
    pub fn blocked_at(&self, x: f64) -> bool {
        blocked(&self.blockers, x, self.top + 1.0)
    }

    pub fn spans(&self, x: f64) -> bool {
        x >= self.min_x && x <= self.max_x
    }

    pub fn is_dock(&self) -> bool {
        self.key.id == SurfaceId::Dock
    }
}

/// Shelf with the smallest `key` (first wins ties); `None` for no shelves.
pub fn min_key<'a>(shelves: impl Iterator<Item = &'a Shelf>, key: impl Fn(&Shelf) -> f64) -> Option<&'a Shelf> {
    let mut best: Option<(&Shelf, f64)> = None;
    for s in shelves {
        let k = key(s);
        if best.is_none_or(|(_, bk)| k < bk) {
            best = Some((s, k));
        }
    }
    best.map(|(s, _)| s)
}

#[derive(Clone, Copy, Debug)]
struct Track {
    key: LedgeKey,
    middle: f64,
    top: f64,
    /// When the geometry last changed.
    changed: f64,
    /// When this shelf was last built.
    seen: f64,
    vel: Vec2,
}

/// Keeps what is needed to turn successive snapshots into shelf velocities.
#[derive(Debug, Default)]
pub struct Ledges {
    tracks: Vec<Track>,
}

impl Ledges {
    /// Shelves for this step. A velocity is measured when a shelf moved
    /// since it was last seen (at most 0.3 s ago); a jump of more than 160 pt
    /// at over 6000 pt/s is a teleport (`jumped`, velocity 0).
    pub fn build(&mut self, now: f64, snaps: &[ScreenSnap]) -> Vec<Shelf> {
        let mut shelves = Vec::new();
        let mut tracks = Vec::new();
        for snap in snaps {
            let f = snap.frame;
            let origin = f.min();
            shelves.push(Shelf {
                key: LedgeKey::new(origin, SurfaceId::Ground),
                min_x: f.min_x(),
                max_x: f.max_x(),
                top: f.min_y() + GROUND_HEIGHT,
                middle: f.mid_x(),
                vel: Vec2::ZERO,
                jumped: false,
                blockers: Vec::new(),
                shown: snap.shown && snap.ground,
                ground: true,
                owner: String::new(),
                screen: f,
            });
            for s in snap.surfaces.iter().filter(|s| s.id != SurfaceId::Ground) {
                let g = s.frame.offset(origin.x, origin.y);
                let key = LedgeKey::new(origin, s.id);
                let (middle, top) = (g.mid_x(), g.max_y());
                let (vel, jumped, changed) = self.measure(key, middle, top, now);
                tracks.push(Track { key, middle, top, changed, seen: now, vel });
                shelves.push(Shelf {
                    key,
                    min_x: g.min_x().max(f.min_x()),
                    max_x: g.max_x().min(f.max_x()),
                    top,
                    middle,
                    vel,
                    jumped,
                    blockers: s.blockers.iter().map(|r| r.offset(origin.x, origin.y)).collect(),
                    shown: snap.shown,
                    ground: false,
                    owner: s.owner.clone(),
                    screen: f,
                });
            }
        }
        self.tracks = tracks;
        shelves
    }

    fn measure(&self, key: LedgeKey, middle: f64, top: f64, now: f64) -> (Vec2, bool, f64) {
        let Some(p) = self.tracks.iter().find(|t| t.key == key) else {
            return (Vec2::ZERO, false, now);
        };
        if p.middle == middle && p.top == top {
            let vel = if now - p.changed < 0.1 { p.vel } else { Vec2::ZERO };
            return (vel, false, p.changed);
        }
        // Spread the move over the time since the last change (or, after a
        // long rest, since the shelf was last seen).
        let since = if now - p.changed < 0.3 { p.changed } else { p.seen };
        let d = Vec2::new(middle - p.middle, top - p.top);
        let vel = d * (1.0 / (now - since).max(1.0 / 120.0));
        if vel.len() > 6000.0 && d.len() > 160.0 { (Vec2::ZERO, true, now) } else { (vel, false, now) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::surface::Surface;

    fn snap(surfaces: Vec<Surface>) -> ScreenSnap {
        ScreenSnap { frame: Frame::new(100.0, 50.0, 800.0, 600.0), shown: true, ground: true, surfaces }
    }

    fn win(id: i64, x: f64, y: f64) -> Surface {
        let mut s = Surface::new(SurfaceId::Window(id), Frame::new(x, y, 200.0, 100.0));
        s.blockers.push(Rect4::new(0.0, 0.0, 10.0, y + 102.0));
        s.owner = "app.exe".into();
        s
    }

    #[test]
    fn ground_and_window_geometry() {
        let mut l = Ledges::default();
        let snaps = [snap(vec![win(7, 700.0, 200.0), Surface::new(SurfaceId::Ground, Frame::default())])];
        let shelves = l.build(0.0, &snaps);
        assert_eq!(shelves.len(), 2);
        let g = &shelves[0];
        assert!(g.ground && g.shown && !g.is_dock());
        assert_eq!((g.min_x, g.max_x, g.top, g.middle), (100.0, 900.0, 55.0, 500.0));
        assert!(g.spans(100.0) && !g.spans(901.0));
        let w = &shelves[1];
        assert_eq!((w.min_x, w.max_x, w.top, w.middle), (800.0, 900.0, 350.0, 900.0));
        assert_eq!(w.owner, "app.exe");
        assert!(w.blocked_at(105.0) && !w.blocked_at(500.0));
        assert_eq!(w.blockers[0].x0, 100.0);
        let dock = Shelf { key: LedgeKey::new(Vec2::ZERO, SurfaceId::Dock), ..w.clone() };
        assert!(dock.is_dock());
    }

    #[test]
    fn min_key_picks_the_first_smallest() {
        let shelves =
            Ledges::default().build(0.0, &[snap(vec![win(1, 0.0, 100.0), win(2, 0.0, 100.0), win(3, 0.0, 50.0)])]);
        let lowest = min_key(shelves.iter(), |s| s.top).unwrap();
        assert_eq!(lowest.key.id, SurfaceId::Ground);
        let highest = min_key(shelves.iter(), |s| -s.top).unwrap();
        assert_eq!(highest.key.id, SurfaceId::Window(1));
        assert!(min_key(std::iter::empty(), |s| s.top).is_none());
    }

    #[test]
    fn ground_hidden_when_not_shown() {
        let mut s = snap(vec![]);
        s.ground = false;
        assert!(!Ledges::default().build(0.0, &[s])[0].shown);
    }

    #[test]
    fn velocity_and_jump() {
        let mut l = Ledges::default();
        l.build(0.0, &[snap(vec![win(7, 300.0, 200.0)])]);
        // moved 6 pt right in 0.05 s
        let s = l.build(0.05, &[snap(vec![win(7, 306.0, 200.0)])]);
        assert!((s[1].vel.x - 120.0).abs() < 1e-9 && !s[1].jumped);
        // unchanged shortly after keeps velocity
        let s = l.build(0.07, &[snap(vec![win(7, 306.0, 200.0)])]);
        assert!((s[1].vel.x - 120.0).abs() < 1e-9);
        // unchanged long after: stopped
        let s = l.build(0.4, &[snap(vec![win(7, 306.0, 200.0)])]);
        assert_eq!(s[1].vel, Vec2::ZERO);
        // moving again after a rest: spread over the last frame interval
        let s = l.build(0.42, &[snap(vec![win(7, 308.0, 200.0)])]);
        assert!((s[1].vel.x - 100.0).abs() < 1e-6, "{:?}", s[1].vel);
        let s = l.build(5.0, &[snap(vec![win(7, 400.0, 200.0)])]);
        assert!(s[1].vel.x > 0.0 && s[1].vel.x < 100.0 && !s[1].jumped);
        // teleport
        let s = l.build(5.01, &[snap(vec![win(7, 900.0, 300.0)])]);
        assert!(s[1].jumped && s[1].vel == Vec2::ZERO);
        // new window has no velocity
        let s = l.build(5.02, &[snap(vec![win(8, 900.0, 300.0)])]);
        assert!(!s[1].jumped);
    }
}
