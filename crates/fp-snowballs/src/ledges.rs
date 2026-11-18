//! World ledges built from the tracker's screen snapshots (snowballs 3.4).

use fp_core::geom::{Frame, Rect4, Vec2};
use fp_core::surface::{LedgeKey, ScreenSnap, SurfaceId, blocked};

/// Height of the ground ledge above the bottom of a screen.
pub const GROUND_HEIGHT: f64 = 5.0;
/// Ledge velocities are only derived when the previous read is this recent.
const FRESH: f64 = 0.3;
/// Smallest time step used for a ledge velocity.
const MIN_DT: f64 = 1.0 / 120.0;
/// A ledge faster than this and farther than [`TELEPORT_MOVE`] has teleported.
const TELEPORT_SPEED: f64 = 6000.0;
const TELEPORT_MOVE: f64 = 160.0;

/// A ledge in world space (snowballs 3.4).
#[derive(Clone, Debug, PartialEq)]
pub struct Ledge {
    pub key: LedgeKey,
    pub min_x: f64,
    pub max_x: f64,
    pub top: f64,
    pub middle: f64,
    pub velocity: Vec2,
    pub jumped: bool,
    pub shown: bool,
    pub owner: String,
    /// Windows above the ledge top, in world points.
    pub blockers: Vec<Rect4>,
}

impl Ledge {
    /// `true` when a higher window covers the ledge top at `x` (3.4 `Blocked`).
    pub fn is_blocked(&self, x: f64) -> bool {
        blocked(&self.blockers, x, self.top + 1.0)
    }

    /// `true` when `x` lies within the ledge span (closed).
    pub fn spans(&self, x: f64) -> bool {
        x >= self.min_x && x <= self.max_x
    }
}

/// One selected screen as the balls need it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// Global frame (Y up).
    pub frame: Frame,
    pub shown: bool,
    pub ground: bool,
}

/// Builds world ledges from the latest snapshot, keeping velocities (3.4).
#[derive(Clone, Debug, Default)]
pub struct Ledges {
    list: Vec<Ledge>,
    screens: Vec<Screen>,
    read_at: Option<f64>,
    generation: u64,
}

impl Ledges {
    /// Rebuilds the ledge list from `screens`. `now` is monotonic seconds.
    pub fn read(&mut self, screens: &[ScreenSnap], now: f64) {
        let old = std::mem::take(&mut self.list);
        let fresh = self.read_at.is_some_and(|t| now - t < FRESH);
        let dt = self.read_at.map_or(MIN_DT, |t| (now - t).max(MIN_DT));
        self.screens.clear();
        for snap in screens {
            let frame = snap.frame;
            let origin = frame.min();
            self.screens.push(Screen { frame, shown: snap.shown, ground: snap.ground });
            self.list.push(Ledge {
                key: LedgeKey::new(origin, SurfaceId::Ground),
                min_x: frame.min_x(),
                max_x: frame.max_x(),
                top: frame.min_y() + GROUND_HEIGHT,
                middle: frame.mid_x(),
                velocity: Vec2::ZERO,
                jumped: false,
                shown: snap.shown && snap.ground,
                owner: String::new(),
                blockers: Vec::new(),
            });
            for surface in snap.surfaces.iter().filter(|s| s.id != SurfaceId::Ground) {
                let f = surface.frame.offset(origin.x, origin.y);
                let key = LedgeKey::new(origin, surface.id);
                let mut ledge = Ledge {
                    key,
                    min_x: f.min_x().max(frame.min_x()),
                    max_x: f.max_x().min(frame.max_x()),
                    top: f.max_y(),
                    middle: f.mid_x(),
                    velocity: Vec2::ZERO,
                    jumped: false,
                    shown: snap.shown,
                    owner: surface.owner.clone(),
                    blockers: surface.blockers.iter().map(|r| r.offset(origin.x, origin.y)).collect(),
                };
                let before = old.iter().find(|l| l.key == key).filter(|_| fresh);
                if let Some(prev) = before {
                    let step = Vec2::new(ledge.middle - prev.middle, ledge.top - prev.top);
                    let velocity = step * (1.0 / dt);
                    if velocity.len() > TELEPORT_SPEED && step.len() > TELEPORT_MOVE {
                        ledge.jumped = true;
                    } else {
                        ledge.velocity = velocity;
                    }
                }
                self.list.push(ledge);
            }
        }
        self.read_at = Some(now);
        self.generation += 1;
    }

    /// Every ledge: ground ledges first, then each screen's surfaces.
    pub fn all(&self) -> &[Ledge] {
        &self.list
    }

    /// The selected screens of the latest read.
    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    /// Bumped by every [`Ledges::read`].
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn find(&self, key: LedgeKey) -> Option<&Ledge> {
        self.list.iter().find(|l| l.key == key)
    }

    /// The ground ledge of the screen whose frame starts at `origin`.
    pub fn ground_of(&self, origin: Vec2) -> Option<&Ledge> {
        self.list.iter().find(|l| l.key.id == SurfaceId::Ground && l.key.origin == origin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::surface::Surface;

    fn snap(shown: bool, ground: bool, surfaces: Vec<Surface>) -> ScreenSnap {
        ScreenSnap { frame: Frame::new(100.0, 50.0, 800.0, 600.0), shown, ground, surfaces }
    }

    fn window(id: i64, x: f64, y: f64) -> Surface {
        let mut s = Surface::new(SurfaceId::Window(id), Frame::new(x, y, 200.0, 100.0));
        s.owner = "app.exe".into();
        s.blockers.push(Rect4::new(0.0, 0.0, 10.0, 500.0));
        s
    }

    #[test]
    fn builds_ground_and_surfaces_in_world_space() {
        let mut l = Ledges::default();
        let dock = Surface::new(SurfaceId::Dock, Frame::new(-50.0, 0.0, 2000.0, 40.0));
        let junk = Surface::new(SurfaceId::Ground, Frame::new(0.0, 0.0, 1.0, 1.0));
        l.read(&[snap(true, true, vec![window(7, 20.0, 100.0), dock, junk])], 1.0);
        assert_eq!(l.generation(), 1);
        assert_eq!(l.screens().len(), 1);
        let all = l.all();
        assert_eq!(all.len(), 3);
        let g = &all[0];
        assert_eq!((g.min_x, g.max_x, g.top, g.middle), (100.0, 900.0, 55.0, 500.0));
        assert!(g.shown && g.blockers.is_empty() && g.owner.is_empty());
        let w = &all[1];
        assert_eq!((w.min_x, w.max_x, w.top, w.middle), (120.0, 320.0, 250.0, 220.0));
        assert_eq!(w.owner, "app.exe");
        assert_eq!(w.blockers[0], Rect4::new(100.0, 50.0, 110.0, 550.0));
        let d = &all[2];
        assert_eq!((d.min_x, d.max_x), (100.0, 900.0));
        assert_eq!(l.find(d.key).unwrap().key.id, SurfaceId::Dock);
        assert!(l.find(LedgeKey::new(Vec2::ZERO, SurfaceId::Dock)).is_none());
        assert!(l.ground_of(Vec2::new(100.0, 50.0)).is_some());
        assert!(l.ground_of(Vec2::ZERO).is_none());
    }

    #[test]
    fn shown_flags() {
        let mut l = Ledges::default();
        l.read(&[snap(true, false, vec![window(1, 0.0, 0.0)])], 0.0);
        assert!(!l.all()[0].shown);
        assert!(l.all()[1].shown);
        l.read(&[snap(false, true, vec![window(1, 0.0, 0.0)])], 0.0);
        assert!(!l.all()[0].shown && !l.all()[1].shown);
    }

    #[test]
    fn blocked_and_spans() {
        let mut l = Ledges::default();
        let mut s = Surface::new(SurfaceId::Window(1), Frame::new(0.0, 0.0, 100.0, 100.0));
        s.blockers.push(Rect4::new(10.0, 90.0, 20.0, 110.0));
        l.read(&[snap(true, true, vec![s])], 0.0);
        // blocker in world space: x 110..120, y 140..160; top = 150
        let w = &l.all()[1];
        assert!(w.is_blocked(115.0));
        assert!(!w.is_blocked(125.0));
        assert!(w.spans(100.0) && w.spans(200.0) && !w.spans(99.0) && !w.spans(201.0));
    }

    #[test]
    fn velocity_from_consecutive_reads() {
        let mut l = Ledges::default();
        l.read(&[snap(true, true, vec![window(1, 0.0, 0.0)])], 1.0);
        assert_eq!(l.all()[1].velocity, Vec2::ZERO);
        // moved 6 right, 3 up in 0.1 s -> (60, 30)
        l.read(&[snap(true, true, vec![window(1, 6.0, 3.0)])], 1.1);
        let w = &l.all()[1];
        assert!((w.velocity.x - 60.0).abs() < 1e-9 && (w.velocity.y - 30.0).abs() < 1e-9);
        assert!(!w.jumped);
        // a tiny dt is clamped to 1/120
        l.read(&[snap(true, true, vec![window(1, 7.0, 3.0)])], 1.1);
        assert!((l.all()[1].velocity.x - 120.0).abs() < 1e-9);
        // a stale previous read gives no velocity
        l.read(&[snap(true, true, vec![window(1, 100.0, 3.0)])], 5.0);
        assert_eq!(l.all()[1].velocity, Vec2::ZERO);
        // new ledge key: no velocity
        l.read(&[snap(true, true, vec![window(2, 100.0, 3.0)])], 5.05);
        assert_eq!(l.all()[1].velocity, Vec2::ZERO);
    }

    #[test]
    fn teleport_sets_jumped() {
        let mut l = Ledges::default();
        l.read(&[snap(true, true, vec![window(1, 0.0, 0.0)])], 1.0);
        l.read(&[snap(true, true, vec![window(1, 500.0, 0.0)])], 1.05);
        let w = &l.all()[1];
        assert!(w.jumped);
        assert_eq!(w.velocity, Vec2::ZERO);
        // fast but short: not a teleport
        l.read(&[snap(true, true, vec![window(1, 500.0, 0.0)])], 2.0);
        l.read(&[snap(true, true, vec![window(1, 600.0, 0.0)])], 2.001);
        assert!(!l.all()[1].jumped);
    }
}
