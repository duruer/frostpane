//! Lock-protected mailbox between the per-monitor snow simulations (render
//! threads) and the desktop world controller (UI thread). Snowballs 3.2 and 4,
//! snow-sim 3.12-3.14. Latest snapshot wins; bursts are consumed once.

use crate::geom::Vec2;
use crate::surface::ScreenSnap;
use std::sync::Mutex;

/// Max queued gather entries.
pub const GATHER_CAP: usize = 1000;
/// Gathering faster than this only scatters snow (pt/s).
pub const SLOWEST_GATHER: f64 = 350.0;
/// Share of carved snow absorbed by a nearby loose ball.
pub const TO_BALL: f64 = 0.75;
/// Share of carved snow that seeds a new ball.
pub const TO_SEED: f64 = 0.4;
/// Max bursts queued per screen per controller step.
pub const BURST_CAP: usize = 40;

/// Snow impact: shatter `area` of snow at `point` (screen-local).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burst {
    pub point: Vec2,
    pub area: f64,
    pub velocity: Vec2,
}

/// The plough blade the snow field should push this frame (snow-sim 3.13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blade {
    /// Screen-local x of the blade front.
    pub front: f64,
    /// +1 right, -1 left.
    pub way: f64,
    pub speed: f64,
    /// Snow height left behind.
    pub leave: f64,
    pub on_dock: bool,
}

/// Snow offered to the controller by the cursor carve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gathered {
    /// Target ball, or `None` to seed a new one.
    pub ball: Option<u64>,
    pub area: f64,
    /// World point.
    pub point: Vec2,
}

/// A loose ball that can absorb gathered snow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub ball: u64,
    /// World centre.
    pub center: Vec2,
    pub reach: f64,
}

#[derive(Default)]
struct Inner {
    screens: Option<Vec<ScreenSnap>>,
    gathered: Vec<Gathered>,
    targets: Vec<Target>,
    open: bool,
    bursts: Vec<(Vec2, Vec<Burst>)>,
    depths: Vec<(Vec2, f64)>,
    blade: Option<(Vec2, Blade)>,
}

/// The shared mailbox. Cheap to share via `Arc`.
#[derive(Default)]
pub struct SnowFeed {
    inner: Mutex<Inner>,
}

impl SnowFeed {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut g)
    }

    // ---- tracker / renderer side ----

    /// Latest per-screen snapshot (replaces any untaken one).
    pub fn snow(&self, screens: Vec<ScreenSnap>) {
        self.with(|i| i.screens = Some(screens));
    }

    /// Offer carved snow at screen-local `point` on the screen whose global
    /// min corner is `origin`. Returns the absorbed area.
    pub fn gather(&self, area: f64, point: Vec2, velocity: Vec2, origin: Vec2) -> f64 {
        let world = origin + point;
        self.with(|i| {
            if i.gathered.len() >= GATHER_CAP || velocity.len() >= SLOWEST_GATHER {
                return 0.0;
            }
            if let Some(t) = i.targets.iter().find(|t| t.center.dist(world) < t.reach) {
                let a = area * TO_BALL;
                i.gathered.push(Gathered { ball: Some(t.ball), area: a, point: world });
                return a;
            }
            if !i.open {
                return 0.0;
            }
            let a = area * TO_SEED;
            i.gathered.push(Gathered { ball: None, area: a, point: world });
            a
        })
    }

    /// Bursts for this screen; consumed.
    pub fn bursts(&self, origin: Vec2) -> Vec<Burst> {
        self.with(|i| match i.bursts.iter().position(|(o, _)| *o == origin) {
            Some(k) => i.bursts.swap_remove(k).1,
            None => Vec::new(),
        })
    }

    /// Renderer reports this screen's ground snow depth.
    pub fn report_depth(&self, origin: Vec2, depth: f64) {
        self.with(|i| match i.depths.iter_mut().find(|(o, _)| *o == origin) {
            Some(e) => e.1 = depth,
            None => i.depths.push((origin, depth)),
        });
    }

    /// The active plough blade, if it is on this screen.
    pub fn blade(&self, origin: Vec2) -> Option<Blade> {
        self.with(|i| i.blade.filter(|(o, _)| *o == origin).map(|(_, b)| b))
    }

    // ---- controller side ----

    /// Drains `(latest screens if new, gathered entries)`.
    pub fn take(&self) -> (Option<Vec<ScreenSnap>>, Vec<Gathered>) {
        self.with(|i| (i.screens.take(), std::mem::take(&mut i.gathered)))
    }

    /// Republish loose-ball targets and whether new balls may spawn.
    pub fn publish_targets(&self, targets: Vec<Target>, open: bool) {
        self.with(|i| {
            i.targets = targets;
            i.open = open;
        });
    }

    /// Queue a burst for one screen (at most [`BURST_CAP`] waiting per screen).
    pub fn publish_burst(&self, origin: Vec2, b: Burst) {
        self.with(|i| {
            let k = match i.bursts.iter().position(|(o, _)| *o == origin) {
                Some(k) => k,
                None => {
                    i.bursts.push((origin, Vec::new()));
                    i.bursts.len() - 1
                }
            };
            let list = &mut i.bursts[k].1;
            if list.len() < BURST_CAP {
                list.push(b);
            }
        });
    }

    /// Last reported ground depth of a screen.
    pub fn depth(&self, origin: Vec2) -> Option<f64> {
        self.with(|i| i.depths.iter().find(|(o, _)| *o == origin).map(|(_, d)| *d))
    }

    /// Set or clear the single active blade.
    pub fn publish_blade(&self, blade: Option<(Vec2, Blade)>) {
        self.with(|i| i.blade = blade);
    }

    /// Reset everything except reported depths (snowballs 3.1 `Run`).
    pub fn reset(&self, open: bool) {
        self.with(|i| {
            i.screens = None;
            i.gathered.clear();
            i.targets.clear();
            i.bursts.clear();
            i.blade = None;
            i.open = open;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Frame;

    #[test]
    fn gather_rules() {
        let f = SnowFeed::new();
        let o = Vec2::new(100.0, 0.0);
        // closed: nothing seeds
        assert_eq!(f.gather(10.0, Vec2::ZERO, Vec2::ZERO, o), 0.0);
        f.publish_targets(vec![Target { ball: 7, center: Vec2::new(105.0, 0.0), reach: 10.0 }], true);
        assert_eq!(f.gather(10.0, Vec2::new(1.0, 0.0), Vec2::ZERO, o), 7.5);
        assert_eq!(f.gather(10.0, Vec2::new(50.0, 0.0), Vec2::ZERO, o), 4.0);
        assert_eq!(f.gather(10.0, Vec2::ZERO, Vec2::new(400.0, 0.0), o), 0.0);
        let (s, g) = f.take();
        assert!(s.is_none());
        assert_eq!(g[0].ball, Some(7));
        assert_eq!(g[1].ball, None);
        assert_eq!(g[1].point, Vec2::new(150.0, 0.0));
        for _ in 0..GATHER_CAP {
            f.gather(1.0, Vec2::new(500.0, 0.0), Vec2::ZERO, o);
        }
        assert_eq!(f.gather(1.0, Vec2::new(500.0, 0.0), Vec2::ZERO, o), 0.0);
    }

    #[test]
    fn snapshots_bursts_depth_blade() {
        let f = SnowFeed::new();
        let a = Vec2::ZERO;
        let b = Vec2::new(1920.0, 0.0);
        f.snow(vec![ScreenSnap { frame: Frame::new(0.0, 0.0, 1.0, 1.0), shown: true, ground: true, surfaces: vec![] }]);
        assert_eq!(f.take().0.unwrap().len(), 1);
        assert!(f.take().0.is_none());
        let burst = Burst { point: Vec2::ZERO, area: 500.0, velocity: Vec2::ZERO };
        for _ in 0..(BURST_CAP + 5) {
            f.publish_burst(a, burst);
        }
        f.publish_burst(b, burst);
        assert_eq!(f.bursts(a).len(), BURST_CAP);
        assert!(f.bursts(a).is_empty());
        assert_eq!(f.bursts(b).len(), 1);
        assert_eq!(f.depth(a), None);
        f.report_depth(a, 3.0);
        f.report_depth(a, 4.0);
        assert_eq!(f.depth(a), Some(4.0));
        let blade = Blade { front: 1.0, way: 1.0, speed: 120.0, leave: 2.0, on_dock: false };
        f.publish_blade(Some((b, blade)));
        assert_eq!(f.blade(b), Some(blade));
        assert_eq!(f.blade(a), None);
        f.publish_burst(a, burst);
        f.reset(true);
        assert!(f.bursts(a).is_empty());
        assert_eq!(f.blade(b), None);
        assert_eq!(f.depth(a), Some(4.0));
    }
}
