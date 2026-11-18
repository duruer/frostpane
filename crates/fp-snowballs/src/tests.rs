use super::*;
use fp_core::geom::Rect4;
use fp_core::surface::{ScreenSnap, Surface};
use fp_snowman::{Face, Hat};
use serde_json::json;

const DT: f64 = 1.0 / 60.0;

fn gkey() -> LedgeKey {
    LedgeKey::new(Vec2::ZERO, SurfaceId::Ground)
}
fn dkey() -> LedgeKey {
    LedgeKey::new(Vec2::ZERO, SurfaceId::Dock)
}
fn wkey(id: i64) -> LedgeKey {
    LedgeKey::new(Vec2::ZERO, SurfaceId::Window(id))
}

fn window(id: i64, x: f64, y: f64, w: f64, h: f64) -> Surface {
    let mut s = Surface::new(SurfaceId::Window(id), Frame::new(x, y, w, h));
    s.owner = "app.exe".into();
    s
}
/// Window 5: x 200..500, top 340.
fn win() -> Surface {
    window(5, 200.0, 300.0, 300.0, 40.0)
}
/// Dock: whole width, top 40.
fn dock() -> Surface {
    Surface::new(SurfaceId::Dock, Frame::new(0.0, 0.0, 1000.0, 40.0))
}
fn snap(shown: bool, ground: bool, surfaces: Vec<Surface>) -> ScreenSnap {
    ScreenSnap { frame: Frame::new(0.0, 0.0, 1000.0, 700.0), shown, ground, surfaces }
}
fn ptr(x: f64, y: f64, down: bool, hot: Option<u64>) -> PointerInput {
    PointerInput {
        pos: Some(Vec2::new(x, y)),
        left_down: down,
        right_down: false,
        hot: hot.map(|id| SpriteKey::new(Owner::Ball, id)),
    }
}

struct Sim {
    balls: Balls,
    ledges: Ledges,
    now: f64,
    out: Vec<BallEvent>,
    ptr: PointerInput,
}

impl Sim {
    fn with(shown: bool, ground: bool, surfaces: Vec<Surface>) -> Sim {
        let mut ledges = Ledges::default();
        ledges.read(&[snap(shown, ground, surfaces)], 0.0);
        Sim { balls: Balls::new(), ledges, now: 0.0, out: Vec::new(), ptr: PointerInput::default() }
    }
    fn new(surfaces: Vec<Surface>) -> Sim {
        Sim::with(true, true, surfaces)
    }
    fn step(&mut self, n: usize) {
        for _ in 0..n {
            self.now += DT;
            self.balls.step(DT, self.now, &self.ledges, &self.ptr, &mut self.out);
        }
    }
    fn put(&mut self, x: f64, y: f64, r: f64, support: Support) -> u64 {
        put(&mut self.balls, x, y, r, support)
    }
    fn ball(&self, id: u64) -> &Ball {
        self.balls.list.iter().find(|b| b.id == id).unwrap()
    }
    fn bursts(&self) -> usize {
        self.out.iter().filter(|e| matches!(e, BallEvent::Burst { .. })).count()
    }
}

fn put(balls: &mut Balls, x: f64, y: f64, r: f64, support: Support) -> u64 {
    let id = balls.next_id;
    balls.next_id += 1;
    balls.list.push(Ball::new(id, Vec2::new(x, y), r, support));
    id
}

/// A stack standing on `key` at `x`; returns the ids bottom first.
fn stack(balls: &mut Balls, x: f64, bottom_y: f64, key: LedgeKey, radii: &[f64]) -> Vec<u64> {
    let saved = Saved::bare(radii.to_vec(), (1..=radii.len() as u64).collect(), vec![0.0; radii.len()], None);
    let first = balls.push_stack(&saved, x, bottom_y, Support::Ledge(key), false);
    (first..first + radii.len() as u64).collect()
}

fn gathered(ball: Option<u64>, area: f64, x: f64, y: f64) -> Gathered {
    Gathered { ball, area, point: Vec2::new(x, y) }
}

// ---- absorb ----

#[test]
fn grow_formula_and_cap() {
    assert!((grown(10.0, 0.0) - 10.0).abs() < 1e-12);
    assert!((grown(0.0, 100.0) - (150.0 / PI).sqrt()).abs() < 1e-12);
    assert_eq!(grown(60.0, 1e6), MAX_RADIUS);
}

#[test]
fn absorb_grows_a_known_ball() {
    let mut s = Sim::new(vec![]);
    let id = s.put(100.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.absorb(&[gathered(Some(id), 30.0, 100.0, 14.0)], &s.ledges, 1.0, false);
    assert!((s.ball(id).r - (100.0 + 45.0 / PI).sqrt()).abs() < 1e-9);
    assert!(s.balls.seed.is_none());
}

#[test]
fn absorb_seeds_and_spawns() {
    let mut s = Sim::new(vec![]);
    s.balls.absorb(&[gathered(None, 40.0, 500.0, 100.0)], &s.ledges, 1.0, false);
    assert_eq!(s.balls.count(), 0);
    s.balls.absorb(&[gathered(None, 40.0, 510.0, 100.0)], &s.ledges, 1.2, false);
    assert_eq!(s.balls.count(), 0);
    s.balls.absorb(&[gathered(None, 20.0, 520.0, 100.0)], &s.ledges, 1.4, false);
    assert_eq!(s.balls.count(), 1);
    let b = &s.balls.list[0];
    assert!((b.r - (100.0_f64 * 1.5 / PI).sqrt()).abs() < 1e-9);
    assert_eq!(b.seed, 97);
    assert_eq!(b.support, Support::Air);
    assert_eq!(b.c, Vec2::new(520.0, 100.0 + b.r));
    assert!(s.balls.seed.is_none());
}

#[test]
fn seed_expires_or_moves_away() {
    let mut s = Sim::new(vec![]);
    s.balls.absorb(&[gathered(None, 60.0, 100.0, 100.0)], &s.ledges, 1.0, false);
    s.balls.absorb(&[gathered(None, 60.0, 100.0, 100.0)], &s.ledges, 1.6, false);
    assert_eq!(s.balls.count(), 0);
    assert_eq!(s.balls.seed.unwrap().area, 60.0);
    s.balls.absorb(&[gathered(None, 60.0, 300.0, 100.0)], &s.ledges, 1.7, false);
    assert_eq!(s.balls.seed.unwrap().area, 60.0);
}

#[test]
fn unknown_target_ball_seeds_instead() {
    let mut s = Sim::new(vec![]);
    s.balls.absorb(&[gathered(Some(99), 95.0, 500.0, 100.0)], &s.ledges, 1.0, false);
    assert_eq!(s.balls.count(), 1);
}

#[test]
fn seed_waits_while_dragging_or_full() {
    let mut s = Sim::new(vec![]);
    s.balls.absorb(&[gathered(None, 95.0, 500.0, 100.0)], &s.ledges, 1.0, true);
    assert_eq!(s.balls.count(), 0);
    assert!(s.balls.seed.is_some());
    let id = s.put(900.0, 14.0, 6.0, Support::Air);
    s.balls.drag = Some(Drag { id, grab: Vec2::ZERO });
    s.balls.absorb(&[gathered(None, 95.0, 500.0, 100.0)], &s.ledges, 1.1, false);
    assert_eq!(s.balls.count(), 1);
    s.balls.drag = None;
    for k in 0..29 {
        s.put(10.0 + k as f64, 500.0, 6.0, Support::Air);
    }
    assert_eq!(s.balls.count(), 30);
    s.balls.absorb(&[gathered(None, 95.0, 500.0, 100.0)], &s.ledges, 1.2, false);
    assert_eq!(s.balls.count(), 30);
}

#[test]
fn seed_grows_a_nearby_ball() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.absorb(&[gathered(None, 95.0, 510.0, 14.0)], &s.ledges, 1.0, false);
    assert_eq!(s.balls.count(), 1);
    assert!(s.ball(id).r > 10.0);
}

#[test]
fn spawn_lands_on_the_highest_ledge_in_reach() {
    let mut s = Sim::new(vec![dock(), win()]);
    s.balls.absorb(&[gathered(None, 95.0, 300.0, 330.0)], &s.ledges, 1.0, false);
    let b = &s.balls.list[0];
    assert_eq!(b.support, Support::Ledge(wkey(5)));
    assert!((b.c.y - (339.0 + b.r)).abs() < 1e-9);
    let mut covered = win();
    covered.blockers.push(Rect4::new(290.0, 330.0, 310.0, 360.0));
    let mut s = Sim::new(vec![covered]);
    s.balls.absorb(&[gathered(None, 95.0, 300.0, 330.0)], &s.ledges, 1.0, false);
    assert_eq!(s.balls.list[0].support, Support::Air);
    s.balls.absorb(&[gathered(None, 95.0, 900.0, 330.0)], &s.ledges, 3.0, false);
    assert_eq!(s.balls.list[1].support, Support::Air);
}

#[test]
fn targets_are_loose_balls_only() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 400.0, 24.0, gkey(), &[20.0, 10.0]);
    let single = s.put(700.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.put(800.0, 300.0, 10.0, Support::Air);
    let carried = s.put(900.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.list[4].carried = true;
    let gone = s.put(950.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.list.last_mut().unwrap().gone = true;
    let t = s.balls.targets();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].ball, single);
    assert_eq!(t[0].reach, 24.0);
    assert!(![ids[0], ids[1], carried, gone].contains(&t[0].ball));
}

// ---- physics ----

#[test]
fn step_without_time_does_nothing() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 100.0, 10.0, Support::Air);
    s.balls.step(0.0, 1.0, &s.ledges, &PointerInput::default(), &mut s.out);
    assert_eq!(s.ball(id).c, Vec2::new(500.0, 100.0));
}

#[test]
fn ball_lands_on_the_dock_and_rests() {
    let mut s = Sim::new(vec![dock()]);
    let id = s.put(600.0, 100.0, 10.0, Support::Air);
    s.step(60);
    let b = s.ball(id);
    assert_eq!(b.support, Support::Ledge(dkey()));
    assert!((b.c.y - 49.0).abs() < 1e-9);
    assert!(!b.hidden);
}

#[test]
fn ball_rolls_and_slows() {
    let mut s = Sim::new(vec![]);
    let id = s.put(100.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.list[0].v.x = 300.0;
    s.step(10);
    let b = s.ball(id);
    assert!(b.c.x > 100.0 && b.v.x < 300.0 && b.v.x > 0.0);
    assert!(b.angle < 0.0);
    assert!((b.c.y - 14.0).abs() < 1e-9);
}

#[test]
fn rolls_off_an_edge_and_lands_below() {
    let mut s = Sim::new(vec![dock(), win()]);
    let id = s.put(495.0, 349.0, 10.0, Support::Ledge(wkey(5)));
    s.balls.list[0].v.x = 600.0;
    s.step(5);
    assert_eq!(s.ball(id).support, Support::Air);
    s.step(200);
    assert_eq!(s.ball(id).support, Support::Ledge(dkey()));
}

#[test]
fn hard_fall_breaks_into_a_burst() {
    let mut s = Sim::new(vec![dock()]);
    s.put(600.0, 650.0, 10.0, Support::Air);
    s.step(120);
    assert_eq!(s.balls.count(), 0);
    let Some(BallEvent::Burst { origin, burst }) = s.out.first() else { panic!("no burst") };
    assert_eq!(*origin, Vec2::ZERO);
    assert!((burst.area - PI * 100.0 / 1.5).abs() < 1e-9);
    assert_eq!(burst.point.x, 600.0);
    assert!(burst.velocity.y < -1000.0);
}

#[test]
fn floor_catches_when_nothing_is_shown() {
    let mut s = Sim::with(true, false, vec![]);
    let id = s.put(500.0, 60.0, 10.0, Support::Air);
    s.step(30);
    assert_eq!(s.ball(id).support, Support::Ledge(gkey()));
    assert!(s.ball(id).hidden);
    s.put(500.0, 650.0, 10.0, Support::Air);
    s.step(120);
    assert_eq!(s.bursts(), 1);
}

#[test]
fn offscreen_balls_vanish_silently() {
    let mut s = Sim::new(vec![]);
    s.put(-500.0, 100.0, 10.0, Support::Air);
    s.step(1);
    assert_eq!(s.balls.count(), 0);
    assert!(s.out.is_empty());
}

#[test]
fn unknown_screens_keep_balls() {
    let mut s = Sim::new(vec![]);
    s.ledges = Ledges::default();
    let id = s.put(-500.0, 100.0, 10.0, Support::Air);
    s.step(1);
    assert!(s.balls.idx(id).is_some());
}

#[test]
fn blocked_ledge_hides_the_ball() {
    let mut w = win();
    w.blockers.push(Rect4::new(250.0, 330.0, 300.0, 400.0));
    let mut s = Sim::new(vec![dock(), w]);
    let id = s.put(290.0, 349.0, 10.0, Support::Ledge(wkey(5)));
    s.step(2);
    assert!(s.ball(id).hidden);
    let mut out = Vec::new();
    s.balls.sprites(&mut out);
    assert_eq!((out[0].opacity, out[0].hit), (0.0, HitMode::Never));
    let fall = s.put(255.0, 400.0, 10.0, Support::Air);
    s.step(120);
    assert_eq!(s.ball(fall).support, Support::Ledge(dkey()));
}

#[test]
fn hidden_ledges_are_not_landed_on() {
    let mut s = Sim::with(false, true, vec![dock()]);
    let id = s.put(600.0, 100.0, 10.0, Support::Air);
    s.step(60);
    assert_eq!(s.ball(id).support, Support::Ledge(gkey()));
}

#[test]
fn jumped_and_missing_ledges_drop_balls() {
    let mut s = Sim::new(vec![win()]);
    let id = s.put(300.0, 349.0, 10.0, Support::Ledge(wkey(5)));
    s.step(1);
    assert_eq!(s.ball(id).support, Support::Ledge(wkey(5)));
    s.ledges.read(&[snap(true, true, vec![window(5, 700.0, 300.0, 300.0, 40.0)])], 0.05);
    s.step(1);
    assert_eq!(s.ball(id).support, Support::Air);
    let other = s.put(300.0, 349.0, 10.0, Support::Ledge(wkey(9)));
    s.balls.list[1].v.y = 50.0;
    s.step(1);
    assert_eq!(s.ball(other).support, Support::Air);
}

#[test]
fn cursor_nudges_a_free_ball() {
    let mut s = Sim::new(vec![]);
    let id = s.put(600.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.ptr = ptr(590.0, 14.0, false, None);
    s.step(1);
    s.ptr = ptr(596.0, 14.0, false, None);
    s.step(1);
    assert!((s.ball(id).v.x - 180.0).abs() < 1e-6);
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 600.0, 24.0, gkey(), &[20.0, 10.0]);
    s.ptr = ptr(590.0, 24.0, false, None);
    s.step(1);
    s.ptr = ptr(596.0, 24.0, false, None);
    s.step(1);
    assert_eq!(s.ball(ids[0]).v.x, 0.0);
}

#[test]
fn plough_pushes_balls_in_front_of_the_blade() {
    let mut s = Sim::new(vec![dock()]);
    let id = s.put(300.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.shove(295.0, 1.0, Vec2::ZERO);
    s.step(1);
    let b = s.ball(id);
    assert!((b.c.x - 305.0).abs() < 1e-9 && b.v.x == 120.0);
    s.step(1);
    assert!(s.ball(id).v.x < 120.0);
    for (front, origin, dock_run) in [
        (500.0, Vec2::ZERO, false),
        (295.0, Vec2::new(5.0, 5.0), false),
        (295.0, Vec2::ZERO, true),
        (200.0, Vec2::ZERO, false),
    ] {
        let mut s = Sim::new(vec![dock()]);
        let id = s.put(300.0, 14.0, 10.0, Support::Ledge(gkey()));
        s.balls.shove_run(front, 1.0, origin, 1.0, dock_run);
        s.step(1);
        assert_eq!(s.ball(id).v.x, 0.0, "{front} {origin:?} {dock_run}");
    }
    let mut s = Sim::new(vec![dock()]);
    let id = s.put(300.0, 49.0, 10.0, Support::Ledge(dkey()));
    s.balls.shove_run(305.0, -1.0, Vec2::ZERO, 1.0, true);
    s.step(1);
    assert_eq!(s.ball(id).v.x, -120.0);
}

/// Window 5 (x 200..500) at `top`, plus window 6 at top 360 when `high`.
fn tilt(s: &mut Sim, top: f64, high: bool) {
    let mut surfaces = vec![window(5, 200.0, top - 40.0, 300.0, 40.0)];
    if high {
        surfaces.push(window(6, 200.0, 320.0, 300.0, 40.0));
    }
    s.now += DT;
    s.ledges.read(&[snap(true, true, surfaces)], s.now);
    s.balls.step(DT, s.now, &s.ledges, &s.ptr, &mut s.out);
}

fn launch(high: bool) -> (Sim, u64) {
    let mut s = Sim::new(vec![]);
    let mut top = 20.0;
    s.ledges.read(&[snap(true, true, vec![window(5, 200.0, top - 40.0, 300.0, 40.0)])], 0.0);
    let id = s.put(300.0, top - 1.0 + 10.0, 10.0, Support::Ledge(wkey(5)));
    for _ in 0..40 {
        top += 500.0 * DT;
        tilt(&mut s, top, high);
    }
    (s, id)
}

#[test]
fn window_stop_launches_and_the_ball_lands_again() {
    let (mut s, id) = launch(false);
    assert!(s.ball(id).air.is_none());
    let top = 20.0 + 40.0 * 500.0 * DT;
    let mut flew = false;
    for _ in 0..60 {
        tilt(&mut s, top, false);
        flew |= s.ball(id).air.is_some();
    }
    assert!(flew);
    assert!(s.ball(id).air.is_none());
    assert_eq!(s.ball(id).support, Support::Ledge(wkey(5)));
}

#[test]
fn a_hopping_ball_lands_on_a_higher_window() {
    let (mut s, id) = launch(true);
    let top = 20.0 + 40.0 * 500.0 * DT;
    for _ in 0..60 {
        tilt(&mut s, top, true);
    }
    assert_eq!(s.ball(id).support, Support::Ledge(wkey(6)));
    assert!(!s.ball(id).hidden);
}

#[test]
fn hard_hop_landings_break() {
    let mut s = Sim::new(vec![win()]);
    s.put(300.0, 349.0, 10.0, Support::Ledge(wkey(5)));
    s.step(1);
    let b = &mut s.balls.list[0];
    b.air = Some(345.0);
    b.rise = -1600.0;
    s.step(1);
    assert_eq!(s.balls.count(), 0);
    assert_eq!(s.bursts(), 1);
    let mut s = Sim::new(vec![window(5, 200.0, 300.0, 300.0, 40.0), window(6, 200.0, 320.0, 300.0, 40.0)]);
    s.put(300.0, 349.0, 10.0, Support::Ledge(wkey(5)));
    s.step(1);
    let b = &mut s.balls.list[0];
    b.air = Some(362.0);
    b.rise = -1600.0;
    s.step(1);
    assert_eq!(s.balls.count(), 0);
}

#[test]
fn stacking_lands_on_a_ball() {
    let mut s = Sim::new(vec![]);
    let base = s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    let top = s.put(600.0, 80.0, 10.0, Support::Air);
    s.step(40);
    let b = s.ball(top);
    assert_eq!(b.support, Support::Ball(base));
    assert!((b.c.y - (24.0 + 30.0 * 0.82)).abs() < 1e-9);
    assert_eq!(b.c.x, 600.0);
    let mut s = Sim::new(vec![]);
    s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    s.put(600.0, 650.0, 10.0, Support::Air);
    s.step(60);
    assert_eq!(s.balls.count(), 1);
    assert_eq!(s.bursts(), 1);
}

#[test]
fn side_push_slides_a_ball_off_another() {
    let mut s = Sim::new(vec![]);
    s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    let id = s.put(618.0, 40.0, 10.0, Support::Air);
    s.step(1);
    let b = s.ball(id);
    assert_eq!(b.v.x, 120.0);
    assert!(b.c.x > 618.0);
    let mut s = Sim::new(vec![]);
    s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    let id = s.put(582.0, 40.0, 10.0, Support::Air);
    s.step(1);
    assert_eq!(s.ball(id).v.x, -120.0);
}

#[test]
fn falling_ball_ignores_balls_it_cannot_land_on() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 600.0, 24.0, gkey(), &[20.0, 10.0]);
    s.put(600.0, 200.0, 10.0, Support::Air);
    let id = s.put(600.0, 120.0, 6.0, Support::Air);
    s.step(3);
    assert_eq!(s.ball(id).support, Support::Air);
    assert_eq!(s.ball(ids[1]).support, Support::Ball(ids[0]));
}

#[test]
fn thrown_ball_smashes_what_it_hits() {
    let mut s = Sim::new(vec![]);
    let hit = s.put(610.0, 24.0, 20.0, Support::Ledge(gkey()));
    s.put(610.0, 53.0, 10.0, Support::Ball(hit));
    s.put(590.0, 45.0, 10.0, Support::Air);
    s.balls.list[2].thrown = true;
    s.balls.list[2].v = Vec2::new(400.0, 0.0);
    s.step(1);
    assert_eq!(s.balls.count(), 0);
    assert_eq!(s.bursts(), 3);
}

#[test]
fn thrown_ball_passes_hidden_dragged_and_resting_balls() {
    let mut w = window(5, 200.0, 300.0, 500.0, 40.0);
    w.blockers.push(Rect4::new(600.0, 330.0, 620.0, 360.0));
    let mut s = Sim::new(vec![w]);
    let a = s.put(610.0, 359.0, 20.0, Support::Ledge(wkey(5)));
    let thrown = s.put(590.0, 380.0, 10.0, Support::Air);
    s.balls.list[1].thrown = true;
    s.balls.list[1].v = Vec2::new(400.0, 0.0);
    s.step(1);
    assert!(s.ball(a).hidden);
    assert!(s.balls.idx(a).is_some() && s.balls.idx(thrown).is_some());
    s.balls.drag = Some(Drag { id: a, grab: Vec2::ZERO });
    s.balls.list[1].c = Vec2::new(590.0, 380.0);
    s.balls.list[1].v = Vec2::new(400.0, 0.0);
    s.balls.list[1].thrown = true;
    s.balls.list[0].hidden = false;
    s.balls.list[0].support = Support::Ball(thrown);
    s.ptr = PointerInput { pos: None, left_down: true, right_down: false, hot: None };
    s.step(1);
    assert!(s.balls.idx(thrown).is_some());
}

#[test]
fn collisions_push_loose_balls_apart() {
    let mut s = Sim::new(vec![]);
    let a = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    let b = s.put(505.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.list[0].v.x = 100.0;
    s.step(1);
    assert!(s.ball(b).c.x - s.ball(a).c.x > 12.0);
    assert!(s.ball(b).v.x > s.ball(a).v.x);
    let mut s = Sim::new(vec![]);
    let a = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    let b = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.step(1);
    assert!(s.ball(b).c.x > s.ball(a).c.x);
    assert_eq!(s.ball(a).v.x, 0.0);
}

#[test]
fn heavy_stacks_resist_collisions() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 10.0]);
    let single = s.put(525.0, 24.0, 20.0, Support::Ledge(gkey()));
    s.balls.list[0].v.x = 100.0;
    s.step(1);
    assert!(s.ball(single).v.x > 0.0);
    assert!(s.ball(ids[0]).c.x < 500.0 + 100.0 * DT);
    let mut s = Sim::new(vec![]);
    let single = s.put(475.0, 24.0, 20.0, Support::Ledge(gkey()));
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 10.0]);
    s.balls.list[0].v.x = 100.0;
    s.step(1);
    assert!(s.ball(single).v.x < 100.0);
    assert!(s.ball(ids[0]).c.x >= 500.0);
    let mut s = Sim::new(vec![]);
    let a = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 10.0]);
    let b = stack(&mut s.balls, 525.0, 24.0, gkey(), &[20.0, 10.0]);
    s.step(1);
    assert!(s.ball(a[1]).c.x < 500.0 && s.ball(b[1]).c.x > 525.0);
}

#[test]
fn stacks_of_three_dress_the_top_two() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 15.0, 10.0, 8.0]);
    s.balls.list[2].angle = 1.0;
    let look = Look { hat: Some(Hat::Viking), face: Face::Wink, ..Look::classic() };
    s.balls.restyle(ids[0], Some(look));
    s.step(1);
    let decor: Vec<Decor> = ids.iter().map(|&i| s.ball(i).decor).collect();
    assert_eq!(decor, vec![Decor::Plain, Decor::Plain, Decor::Body, Decor::Head]);
    assert_eq!(s.ball(ids[3]).decor_look, look);
    assert_eq!(s.ball(ids[2]).angle, 0.0);
    assert_eq!(s.ball(ids[1]).decor_look, Look::classic());
    s.balls.restyle(ids[0], Some(Look::classic()));
    assert_eq!(s.ball(ids[0]).look, None);
    s.balls.restyle(999, Some(look));
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 10.0]);
    s.step(1);
    assert_eq!(s.ball(ids[1]).decor, Decor::Plain);
}

#[test]
fn riders_follow_and_fall_off_their_base() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 10.0]);
    s.balls.list[0].v.x = 200.0;
    s.step(3);
    assert!((s.ball(ids[1]).c.x - s.ball(ids[0]).c.x).abs() < 1e-9);
    s.balls.list[0].gone = true;
    s.step(1);
    assert_eq!(s.ball(ids[1]).support, Support::Air);
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 10.0]);
    s.balls.list[1].c.x += 30.0;
    s.step(1);
    assert_eq!(s.ball(ids[1]).support, Support::Air);
}

#[test]
fn support_cycles_terminate() {
    let mut b = Balls::new();
    let x = put(&mut b, 0.0, 0.0, 10.0, Support::Air);
    let y = put(&mut b, 0.0, 0.0, 10.0, Support::Ball(x));
    b.list[0].support = Support::Ball(y);
    assert!(!b.rests(0, 99));
    assert!(b.rests(1, x));
    assert_eq!(b.depth(0), MAX_STACK);
    assert!(b.stack(0).len() > 2);
    assert!(b.bottom_of(0) < 2);
}

// ---- grab, drag, throw ----

#[test]
fn drag_and_throw() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.ptr = ptr(502.0, 16.0, true, Some(id));
    s.step(1);
    assert!(s.balls.dragging());
    assert_eq!(s.ball(id).support, Support::Air);
    for k in 1..=8 {
        s.ptr = ptr(502.0 + 60.0 * k as f64, 116.0, true, None);
        s.step(1);
    }
    assert!(s.ball(id).v.len() > 500.0);
    s.ptr = ptr(982.0, 116.0, false, None);
    s.step(1);
    assert!(!s.balls.dragging());
    let b = s.ball(id);
    assert!(b.thrown);
    assert!(b.v.len() <= 1560.0);
}

#[test]
fn slow_release_assists_stacking() {
    let mut s = Sim::new(vec![]);
    let base = s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    let id = s.put(630.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.ptr = ptr(630.0, 14.0, true, Some(id));
    s.step(1);
    s.ptr = ptr(612.0, 70.0, true, None);
    s.step(10);
    s.ptr = ptr(612.0, 70.0, false, None);
    s.step(1);
    assert!(!s.ball(id).thrown);
    s.step(30);
    assert_eq!(s.ball(id).support, Support::Ball(base));
}

#[test]
fn slow_release_without_a_base_just_falls() {
    let mut s = Sim::new(vec![]);
    let id = s.put(630.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.ptr = ptr(630.0, 14.0, true, Some(id));
    s.step(1);
    s.ptr = ptr(630.0, 14.0, false, None);
    s.step(1);
    assert!(!s.balls.dragging());
    assert!(!s.ball(id).thrown);
}

#[test]
fn dragging_without_a_pointer_holds_still() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 100.0, 10.0, Support::Air);
    s.balls.list[0].v = Vec2::new(80.0, -40.0);
    s.balls.list[0].hidden = true;
    s.balls.drag = Some(Drag { id, grab: Vec2::ZERO });
    s.ptr = PointerInput { pos: None, left_down: true, right_down: false, hot: None };
    s.step(1);
    let b = s.ball(id);
    assert_eq!(b.c, Vec2::new(500.0, 100.0));
    // 3.5A: velocity still smooths toward the (zero) displacement
    assert_eq!(b.v, Vec2::new(40.0, -20.0));
    assert!(!b.hidden);
}

#[test]
fn only_ball_sprites_are_grabbed() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.ptr = PointerInput {
        pos: Some(Vec2::new(500.0, 14.0)),
        left_down: true,
        right_down: false,
        hot: Some(SpriteKey::new(Owner::Tree, id)),
    };
    s.step(1);
    assert!(!s.balls.dragging());
    s.ptr = PointerInput { pos: Some(Vec2::new(500.0, 14.0)), left_down: false, right_down: false, hot: None };
    s.step(1);
    s.ptr = PointerInput { pos: Some(Vec2::new(500.0, 14.0)), left_down: true, right_down: false, hot: None };
    s.step(1);
    s.ptr = PointerInput { pos: None, left_down: false, right_down: false, hot: None };
    s.step(1);
    s.ptr = PointerInput { pos: None, left_down: true, right_down: false, hot: Some(SpriteKey::new(Owner::Ball, id)) };
    s.step(1);
    assert!(!s.balls.dragging());
}

#[test]
fn grab_rules() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.balls.grab(77, Vec2::ZERO);
    assert!(!s.balls.dragging());
    s.balls.list[0].carried = true;
    s.balls.grab(id, Vec2::ZERO);
    assert!(!s.balls.dragging());
    s.balls.list[0].carried = false;
    s.balls.grab(id, Vec2::ZERO);
    assert!(s.balls.dragging());
    s.balls.grab(id, Vec2::ZERO);
    s.balls.lift(77, Vec2::ZERO);
}

#[test]
fn clicking_a_snowman_asks_for_its_card() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    s.ptr = ptr(500.0, 60.0, true, Some(ids[2]));
    s.step(1);
    assert!(s.balls.pressed.is_some() && !s.balls.dragging());
    s.balls.grab(ids[2], Vec2::ZERO);
    s.ptr = ptr(502.0, 60.0, false, None);
    s.step(1);
    assert!(s.out.contains(&BallEvent::Dress { bottom: ids[0] }));
    assert!(s.balls.pressed.is_none());
    s.out.clear();
    s.step(1);
    assert!(!s.out.iter().any(|e| matches!(e, BallEvent::Dress { .. })));
}

#[test]
fn moving_a_pressed_snowman_drags_it() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    s.ptr = ptr(500.0, 60.0, true, Some(ids[1]));
    s.step(1);
    s.ptr = ptr(502.0, 60.0, true, None);
    s.step(1);
    assert!(s.balls.pressed.is_some());
    s.ptr = ptr(520.0, 60.0, true, None);
    s.step(1);
    assert!(s.balls.dragging() && s.balls.pressed.is_none());
    assert_eq!(s.ball(ids[1]).support, Support::Air);
    assert_eq!(s.ball(ids[2]).support, Support::Ball(ids[1]));
}

#[test]
fn release_with_vanished_balls_is_harmless() {
    let mut s = Sim::new(vec![]);
    let mut out = Vec::new();
    s.balls.pressed = Some((42, Vec2::ZERO));
    s.balls.release(&mut out);
    s.balls.drag = Some(Drag { id: 42, grab: Vec2::ZERO });
    s.balls.release(&mut out);
    assert!(out.is_empty() && !s.balls.dragging());
}

// ---- smash and friends ----

#[test]
fn prey_smash_and_counts() {
    let mut s = Sim::new(vec![]);
    let three = stack(&mut s.balls, 300.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    stack(&mut s.balls, 600.0, 24.0, gkey(), &[20.0, 15.0]);
    stack(&mut s.balls, 700.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    s.balls.list[5].hidden = true;
    s.step(1);
    s.balls.list[5].hidden = true;
    let prey = s.balls.prey();
    assert_eq!(prey.len(), 1);
    assert_eq!(prey[0].0, three[0]);
    let parts = &prey[0].1;
    assert_eq!(parts.iter().map(|p| p.1).collect::<Vec<_>>(), [20.0, 15.0, 10.0]);
    assert_eq!(parts[0].0.x, 300.0);
    assert_eq!(parts[0].0.y - parts[0].1, 4.0, "foot");
    assert!(parts.windows(2).all(|w| w[1].0.y > w[0].0.y), "bottom to top");
    let mut out = Vec::new();
    s.balls.smash(12345, &mut out);
    assert!(out.is_empty());
    s.balls.smash(three[0], &mut out);
    assert_eq!(out.len(), 3);
    assert_eq!(s.balls.count(), 5);
    s.balls.smash(three[0], &mut out);
    assert_eq!(out.len(), 3);
}

#[test]
fn break_all_and_clear() {
    let mut s = Sim::new(vec![]);
    s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.put(520.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.put(540.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.step(1);
    s.balls.list[1].hidden = true;
    s.balls.list[2].c = Vec2::new(-900.0, 14.0);
    let id = s.balls.list[0].id;
    s.balls.drag = Some(Drag { id, grab: Vec2::ZERO });
    s.balls.restore(&json!([{"Radii": [10.0]}]), 0.0);
    s.balls.seed = Some(Seed { area: 1.0, point: Vec2::ZERO, time: 0.0 });
    s.balls.pressed = Some((1, Vec2::ZERO));
    let mut out = Vec::new();
    s.balls.break_all(&mut out);
    assert_eq!(s.balls.count(), 0);
    assert!(s.balls.waiting.is_empty() && s.balls.seed.is_none() && s.balls.pressed.is_none());
    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], BallEvent::Burst { .. }));
    assert!(!s.balls.dragging());
    s.put(1.0, 1.0, 10.0, Support::Air);
    s.balls.clear();
    assert_eq!(s.balls.count(), 0);
}

// ---- sprites and paint ----

#[test]
fn sprites_describe_the_windows() {
    let mut s = Sim::new(vec![dock()]);
    let a = s.put(600.0, 49.0, 10.0, Support::Ledge(dkey()));
    let b = s.put(700.0, 300.0, 10.0, Support::Air);
    s.step(1);
    let mut out = Vec::new();
    s.balls.sprites(&mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].key, SpriteKey::new(Owner::Ball, a));
    assert_eq!(out[0].z, ZBand::Front);
    assert_eq!(out[1].z, ZBand::Front);
    assert_eq!(out[0].frame.w, panel_size(Decor::Plain, 10.0));
    assert_eq!(out[0].frame.mid_x(), 600.0);
    assert_eq!(out[0].hit, HitMode::Alpha);
    assert_eq!(out[0].opacity, 1.0);
    s.balls.drag = Some(Drag { id: a, grab: Vec2::ZERO });
    let mut out = Vec::new();
    s.balls.sprites(&mut out);
    assert_eq!(out[1].key, SpriteKey::new(Owner::Ball, a));
    assert_eq!(out[0].key, SpriteKey::new(Owner::Ball, b));
}

#[test]
fn revs_follow_what_must_be_repainted() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.step(1);
    let r0 = s.ball(id).rev;
    s.step(1);
    assert_eq!(s.ball(id).rev, r0);
    s.balls.absorb(&[gathered(Some(id), 200.0, 500.0, 14.0)], &s.ledges, 1.0, false);
    s.step(1);
    let r1 = s.ball(id).rev;
    assert!(r1 > r0);
    s.balls.list[0].v.x = 400.0;
    s.step(20);
    assert!(s.ball(id).rev > r1);
    let ids = stack(&mut s.balls, 800.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    s.step(1);
    let r2 = s.ball(ids[2]).rev;
    s.balls.restyle(ids[0], Some(Look { face: Face::Love, ..Look::classic() }));
    s.step(1);
    assert!(s.ball(ids[2]).rev > r2);
}

#[test]
fn paint_draws_the_ball() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    s.step(1);
    let pm = s.balls.paint(id, 2.0).unwrap();
    let side = (panel_size(Decor::Plain, 10.0) * 2.0).ceil() as u32;
    assert_eq!((pm.width(), pm.height()), (side, side));
    assert!(pm.pixels().iter().any(|p| p.alpha() > 0));
    assert!(s.balls.paint(999, 1.0).is_none());
    assert!(s.balls.paint(id, 0.0).is_none());
}

// ---- persistence ----

fn standing(s: &mut Sim, x: f64, key: LedgeKey, bottom_y: f64) -> Vec<u64> {
    stack(&mut s.balls, x, bottom_y, key, &[20.0, 10.0])
}

#[test]
fn save_json_describes_standing_stacks() {
    let mut s = Sim::new(vec![dock(), win()]);
    let g = standing(&mut s, 700.0, gkey(), 24.0);
    standing(&mut s, 300.0, wkey(5), 359.0);
    standing(&mut s, 800.0, dkey(), 59.0);
    let look = Look { hat: Some(Hat::Crown), ..Look::classic() };
    s.balls.restyle(g[0], Some(look));
    s.step(1);
    s.put(900.0, 14.0, 7.0, Support::Ledge(wkey(99)));
    s.balls.list.push(Ball::new(80, Vec2::ZERO, 10.0, Support::Ledge(gkey())));
    s.balls.list.last_mut().unwrap().gone = true;
    let v = s.balls.save_json().unwrap();
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["Place"], 0);
    assert_eq!(list[0]["Across"], 700.0);
    assert_eq!(list[0]["Radii"], json!([20.0, 10.0]));
    assert_eq!(list[0]["Look"]["hat"], "crown");
    assert_eq!(list[1]["Place"], 2);
    assert_eq!(list[1]["Id"], 5);
    assert_eq!(list[1]["Owner"], "app.exe");
    assert_eq!(list[1]["Middle"], 350.0);
    assert_eq!(list[1]["Top"], 340.0);
    assert_eq!(list[1]["Across"], -50.0);
    assert_eq!(list[2]["Place"], 1);
    assert_eq!(list[2]["Across"], 800.0);
    assert!(list[2]["Look"].is_null());
}

#[test]
fn save_is_skipped_when_unsafe() {
    let mut s = Sim::new(vec![]);
    assert!(Balls::new().save_json().is_none());
    standing(&mut s, 700.0, gkey(), 24.0);
    s.step(1);
    assert!(s.balls.save_json().is_some());
    let flying = s.put(100.0, 300.0, 10.0, Support::Air);
    assert!(s.balls.save_json().is_none());
    s.balls.list.pop();
    let id = s.balls.list[0].id;
    s.balls.drag = Some(Drag { id, grab: Vec2::ZERO });
    assert!(s.balls.save_json().is_none());
    let _ = flying;
}

#[test]
fn autosave_reports_changes_every_three_seconds() {
    let mut s = Sim::new(vec![]);
    s.step(1);
    assert!(s.out.is_empty());
    standing(&mut s, 700.0, gkey(), 24.0);
    s.step(120);
    assert!(s.out.is_empty());
    s.step(80);
    assert_eq!(s.out, vec![BallEvent::Dirty]);
    s.step(400);
    assert_eq!(s.out.len(), 1);
    let ids = standing(&mut s, 400.0, gkey(), 24.0);
    s.balls.restyle(ids[0], Some(Look { face: Face::Cool, ..Look::classic() }));
    s.step(1);
    assert_eq!(s.out.len(), 2);
}

#[test]
fn restore_stands_stacks_up_again() {
    let mut s = Sim::new(vec![dock(), win()]);
    let ids = standing(&mut s, 300.0, wkey(5), 359.0);
    let look = Look { face: Face::Sleepy, ..Look::classic() };
    s.balls.restyle(ids[0], Some(look));
    s.balls.list[0].seed = 555;
    s.balls.list[0].angle = 0.5;
    standing(&mut s, 700.0, gkey(), 24.0);
    standing(&mut s, 800.0, dkey(), 59.0);
    s.step(1);
    let saved = s.balls.save_json().unwrap();
    let mut t = Sim::new(vec![dock(), win()]);
    t.balls.restore(&saved, 0.0);
    assert_eq!(t.balls.waiting.len(), 3);
    t.step(1);
    assert!(t.balls.waiting.is_empty());
    assert_eq!(t.balls.count(), 6);
    t.step(1);
    assert_eq!(t.balls.save_json().unwrap(), saved);
    let bottom = t.balls.list.iter().find(|b| b.support == Support::Ledge(wkey(5))).unwrap();
    assert_eq!((bottom.seed, bottom.angle, bottom.look), (555, 0.5, Some(look)));
}

fn one(place: i64, id: i64, owner: &str, middle: f64, top: f64, across: f64) -> serde_json::Value {
    json!([{"Place": place, "Id": id, "Owner": owner, "Middle": middle, "Top": top, "ScreenX": 0.0, "ScreenY": 0.0, "Across": across, "Radii": [20.0, 10.0]}])
}

#[test]
fn restore_places_and_defaults() {
    let mut t = Sim::new(vec![win()]);
    t.balls.restore(&one(2, 5, "app.exe", 350.0, 340.0, 9000.0), 0.0);
    t.step(1);
    let b = &t.balls.list[0];
    assert_eq!(b.c.x, 500.0);
    assert_eq!(b.seed, b.id * 2654435761 + 97);
    assert_eq!(b.angle, 0.0);
    let mut t = Sim::new(vec![]);
    let mut rec = one(0, 0, "", 0.0, 0.0, 100.0);
    rec[0]["ScreenX"] = json!(-5000.0);
    t.balls.restore(&rec, 0.0);
    t.step(1);
    assert_eq!(t.balls.list[0].support, Support::Ledge(gkey()));
    let mut t = Sim::new(vec![]);
    t.balls.restore(&one(1, 0, "", 0.0, 0.0, 300.0), 0.0);
    t.step(60);
    assert_eq!(t.balls.waiting.len(), 1);
    t.now = 7.0;
    t.step(1);
    assert!(t.balls.waiting.is_empty());
    assert_eq!(t.balls.list[0].support, Support::Ledge(gkey()));
    let mut t = Sim::new(vec![dock()]);
    t.balls.restore(&one(1, 0, "", 0.0, 0.0, 300.0), 0.0);
    t.step(1);
    assert_eq!(t.balls.list[0].support, Support::Ledge(dkey()));
}

#[test]
fn restore_finds_a_moved_window_or_gives_up() {
    let mut t = Sim::new(vec![win()]);
    t.balls.restore(&one(2, 99, "app.exe", 360.0, 345.0, 0.0), 0.0);
    t.step(1);
    assert_eq!(t.balls.list[0].support, Support::Ledge(wkey(5)));
    let mut t = Sim::new(vec![win(), window(6, 200.0, 300.0, 300.0, 40.0)]);
    let mut far = one(2, 99, "app.exe", 900.0, 345.0, 0.0);
    far.as_array_mut().unwrap().push(one(2, 5, "other.exe", 350.0, 340.0, 0.0)[0].clone());
    t.balls.restore(&far, 0.0);
    t.step(1);
    assert_eq!(t.balls.waiting.len(), 2);
    t.now = 100.0;
    t.step(1);
    assert_eq!(t.balls.waiting.len(), 2);
    t.now = 200.0;
    t.step(1);
    assert!(t.balls.waiting.is_empty());
    assert_eq!(t.balls.count(), 0);
    let mut t = Sim::new(vec![window(5, 200.0, 300.0, 300.0, 40.0), window(6, 240.0, 310.0, 300.0, 40.0)]);
    t.balls.restore(&one(2, 99, "app.exe", 395.0, 351.0, 0.0), 0.0);
    t.step(1);
    assert_eq!(t.balls.list[0].support, Support::Ledge(wkey(6)));
}

#[test]
fn restore_is_lenient() {
    let mut b = Balls::new();
    b.restore(&json!("nope"), 3.0);
    assert!(b.waiting.is_empty());
    b.restore(&json!([1, {"Radii": []}, {"Radii": [10.0]}]), 3.0);
    assert_eq!(b.waiting.len(), 1);
    assert_eq!(b.waiting_since, 3.0);
    let mut s = Sim::new(vec![]);
    s.balls.restore(&one(2, 5, "none", 0.0, 0.0, 0.0), 0.0);
    s.step(1);
    s.balls.last_bring = s.now;
    s.step(1);
    assert_eq!(s.balls.waiting.len(), 1);
}

// ---- cargo ----

#[test]
fn standing_snowmen_lists_stacks_of_two() {
    let mut s = Sim::new(vec![]);
    let ids = standing(&mut s, 300.0, gkey(), 24.0);
    s.put(500.0, 14.0, 10.0, Support::Ledge(gkey()));
    stack(&mut s.balls, 700.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    s.put(800.0, 300.0, 10.0, Support::Air);
    s.balls.list[2].carried = true;
    s.balls.list[5].gone = true;
    let list = s.balls.standing_snowmen();
    assert_eq!(list[0], (ids[0], Vec2::new(300.0, 4.0)));
    assert_eq!(list.len(), 2);
}

#[test]
fn carry_moves_the_whole_stack() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    s.balls.carry(999, Some(Vec2::ZERO));
    let p = Vec2::new(100.0, 300.0);
    s.balls.carry(ids[0], Some(p));
    assert!(s.ball(ids[0]).carried && s.ball(ids[2]).carried);
    assert_eq!(s.ball(ids[0]).support, Support::Air);
    assert_eq!(s.ball(ids[1]).support, Support::Ball(ids[0]));
    for _ in 0..100 {
        s.balls.carry(ids[0], Some(p));
    }
    assert_eq!(s.ball(ids[0]).c, Vec2::new(100.0, 320.0));
    let spacing = (20.0 + 15.0) * 0.82;
    assert!((s.ball(ids[1]).c.y - (320.0 + spacing)).abs() < 1e-9);
    s.step(5);
    assert_eq!(s.ball(ids[0]).c, Vec2::new(100.0, 320.0));
    assert!(s.balls.save_json().is_none());
    assert!(s.balls.standing_snowmen().is_empty());
    s.balls.carry(ids[0], None);
    s.step(120);
    assert_eq!(s.ball(ids[0]).support, Support::Ledge(gkey()));
}

#[test]
fn take_vanish_board_and_stand() {
    let mut s = Sim::new(vec![]);
    let ids = standing(&mut s, 300.0, gkey(), 24.0);
    let look = Look { hat: Some(Hat::Santa), ..Look::classic() };
    s.balls.restyle(ids[0], Some(look));
    s.step(1);
    assert!(s.balls.take(999).is_none());
    let cargo = s.balls.take(ids[0]).unwrap();
    assert_eq!(s.balls.count(), 0);
    assert_eq!(cargo["Look"]["hat"], "santa");
    assert_eq!(cargo["Radii"], json!([20.0, 10.0]));
    let p = Vec2::new(200.0, 300.0);
    assert!(s.balls.board(&json!(5), p).is_none());
    let id = s.balls.board(&cargo, p).unwrap();
    assert!(s.ball(id).carried);
    assert_eq!(s.ball(id).look, Some(look));
    s.balls.carry(id, Some(p));
    assert_eq!(s.ball(id).c, Vec2::new(200.0, 320.0));
    s.balls.vanish(999);
    s.balls.drag = Some(Drag { id, grab: Vec2::ZERO });
    s.balls.pressed = Some((id, Vec2::ZERO));
    s.balls.vanish(id);
    assert_eq!(s.balls.count(), 0);
    assert!(!s.balls.dragging() && s.balls.pressed.is_none());
    for k in 0..29 {
        s.put(10.0 + k as f64, 500.0, 6.0, Support::Air);
    }
    assert!(s.balls.board(&cargo, p).is_none());
    s.balls.clear();
    let frame = Frame::new(0.0, 0.0, 1000.0, 700.0);
    assert!(!s.balls.stand(&json!(7), 400.0, frame, &s.ledges));
    assert!(!s.balls.stand(&cargo, 400.0, Frame::new(-3000.0, 0.0, 10.0, 10.0), &s.ledges));
    assert!(s.balls.stand(&cargo, 400.0, frame, &s.ledges));
    assert!(s.balls.force_save);
    s.step(1);
    assert_eq!(s.balls.count(), 2);
    assert_eq!(s.balls.list[0].support, Support::Ledge(gkey()));
    assert_eq!(s.balls.list[0].c.x, 400.0);
    assert!(s.out.contains(&BallEvent::Dirty));
}

#[test]
fn defaults_and_misc() {
    let b = Balls::default();
    assert_eq!((b.count(), b.dragging()), (0, false));
    assert_eq!(sign(-0.5), -1.0);
    assert_eq!(sign(0.0), 1.0);
}

#[test]
fn dangling_supports_end_the_walks() {
    let mut b = Balls::new();
    let id = put(&mut b, 0.0, 0.0, 10.0, Support::Ball(404));
    assert_eq!(b.bottom_of(0), 0);
    assert!(!b.rests(0, 1));
    assert_eq!(b.depth(0), 0);
    assert_eq!(b.stack(0), vec![0]);
    // shattering twice reports once
    let mut out = Vec::new();
    b.screens.push(Screen { frame: Frame::new(-10.0, -10.0, 100.0, 100.0), shown: true, ground: true });
    b.shatter(0, &mut out);
    b.shatter(0, &mut out);
    assert_eq!(out.len(), 1);
    let _ = id;
}

#[test]
fn the_highest_candidate_wins() {
    // spawn: ground (top 5) and dock (top 40) are both within reach
    let mut s = Sim::new(vec![dock()]);
    s.balls.absorb(&[gathered(None, 95.0, 600.0, 30.0)], &s.ledges, 1.0, false);
    assert_eq!(s.balls.list[0].support, Support::Ledge(dkey()));
    // landing: two windows crossed in one step
    let mut s = Sim::new(vec![window(5, 200.0, 300.0, 300.0, 42.0), window(6, 200.0, 300.0, 300.0, 44.0)]);
    let id = s.put(300.0, 355.0, 10.0, Support::Air);
    s.balls.list[0].v.y = -580.0;
    s.step(1);
    assert_eq!(s.ball(id).support, Support::Ledge(wkey(6)));
    // stack assist: two balls below the released one
    let mut s = Sim::new(vec![]);
    s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    let upper = s.put(610.0, 70.0, 12.0, Support::Ledge(gkey()));
    let id = s.put(605.0, 150.0, 10.0, Support::Air);
    s.balls.drag = Some(Drag { id, grab: Vec2::ZERO });
    s.ptr = ptr(605.0, 150.0, false, None);
    s.step(1);
    assert_eq!(s.ball(id).c.x, 610.0);
    let _ = upper;
}

#[test]
fn hop_picks_the_highest_ledge() {
    let mut s = Sim::new(vec![
        window(5, 200.0, 300.0, 300.0, 40.0),
        window(6, 200.0, 310.0, 300.0, 40.0),
        window(7, 200.0, 320.0, 300.0, 40.0),
    ]);
    s.put(300.0, 349.0, 10.0, Support::Ledge(wkey(5)));
    s.step(1);
    let b = &mut s.balls.list[0];
    b.air = Some(360.0);
    b.rise = -620.0;
    s.step(1);
    assert_eq!(s.balls.list[0].support, Support::Ledge(wkey(7)));
}

// ---- verifier additions ----

#[test]
fn bad_time_steps_are_rejected_or_clamped() {
    let mut s = Sim::new(vec![]);
    let id = s.put(500.0, 300.0, 10.0, Support::Air);
    s.balls.step(f64::NAN, 1.0, &s.ledges, &PointerInput::default(), &mut s.out);
    s.balls.step(-1.0, 1.0, &s.ledges, &PointerInput::default(), &mut s.out);
    assert_eq!(s.ball(id).c, Vec2::new(500.0, 300.0));
    // a 1 s hitch moves the ball only as far as one 0.05 s step
    s.balls.step(1.0, 2.0, &s.ledges, &PointerInput::default(), &mut s.out);
    let b = s.ball(id);
    assert!((b.v.y - (-GRAVITY * MAX_DT)).abs() < 1e-9);
    assert!((b.c.y - (300.0 - GRAVITY * MAX_DT * MAX_DT)).abs() < 1e-9);
}

#[test]
fn absorb_ignores_broken_entries() {
    let mut s = Sim::new(vec![]);
    let id = s.put(100.0, 14.0, 10.0, Support::Ledge(gkey()));
    let bad = [
        gathered(Some(id), f64::NAN, 100.0, 14.0),
        gathered(Some(id), -50.0, 100.0, 14.0),
        gathered(Some(id), f64::INFINITY, 100.0, 14.0),
        gathered(None, 200.0, f64::NAN, 14.0),
        gathered(None, 200.0, 100.0, f64::INFINITY),
        gathered(None, 0.0, 100.0, 14.0),
    ];
    s.balls.absorb(&bad, &s.ledges, 1.0, false);
    assert_eq!(s.ball(id).r, 10.0);
    assert_eq!(s.balls.count(), 1);
    assert!(s.balls.seed.is_none());
}

#[test]
fn a_side_push_can_carry_a_ball_onto_a_ledge() {
    // window 9 spans x 620..900 with its top at 30 (ledge t = 29)
    let mut s = Sim::new(vec![window(9, 620.0, 0.0, 280.0, 30.0)]);
    s.put(600.0, 24.0, 20.0, Support::Ledge(gkey()));
    let id = s.put(615.0, 40.0, 10.0, Support::Air);
    s.balls.list[1].v = Vec2::new(0.0, -120.0);
    s.step(1);
    let b = s.ball(id);
    // pushed to x > 620 first, then the ledge test uses the new x and vx
    assert_eq!(b.support, Support::Ledge(wkey(9)));
    assert!(b.c.x > 620.0);
    assert!((b.c.y - 39.0).abs() < 1e-9);
    assert!((b.v.x - MIN_SHOVE * 0.6).abs() < 1e-9);
    assert_eq!(b.v.y, 0.0);
}

#[test]
fn unresolved_window_owner_aborts_the_save() {
    let mut nameless = win();
    nameless.owner.clear();
    let mut s = Sim::new(vec![nameless]);
    standing(&mut s, 700.0, gkey(), 24.0);
    s.step(1);
    assert!(s.balls.save_json().is_some());
    standing(&mut s, 300.0, wkey(5), 359.0);
    s.step(1);
    assert_eq!(s.balls.list[2].support, Support::Ledge(wkey(5)));
    assert!(s.balls.save_json().is_none());
    // the cadence still resets, so nothing is reported
    s.step(400);
    assert!(s.out.is_empty());
}

#[test]
fn clear_forgets_the_ledge_snapshot() {
    let mut s = Sim::new(vec![]);
    standing(&mut s, 700.0, gkey(), 24.0);
    s.step(1);
    s.balls.clear();
    assert!(s.balls.snap.is_empty() && s.balls.screens.is_empty());
    assert!(s.balls.save_json().is_none());
    assert!(s.balls.last_save.is_none());
    // a brand-new tracker whose generation matches the old one still syncs
    let mut fresh = Ledges::default();
    fresh.read(&[snap(true, true, vec![dock()])], 5.0);
    assert_eq!(fresh.generation(), s.ledges.generation());
    s.balls.step(DT, 5.0, &fresh, &PointerInput::default(), &mut s.out);
    assert_eq!(s.balls.snap.len(), 2);
    assert_eq!(s.balls.save_json().unwrap(), json!([]));
}

#[test]
fn carry_and_restyle_resolve_the_bottom() {
    let mut s = Sim::new(vec![]);
    let ids = stack(&mut s.balls, 500.0, 24.0, gkey(), &[20.0, 15.0, 10.0]);
    let look = Look { face: Face::Wink, ..Look::classic() };
    s.balls.restyle(ids[2], Some(look));
    assert_eq!(s.ball(ids[0]).look, Some(look));
    assert_eq!(s.ball(ids[2]).look, None);
    assert_eq!(s.balls.look(ids[2]), Some(look));
    assert_eq!(s.balls.look(ids[0]), Some(look));
    assert_eq!(s.balls.look(9999), None);
    s.balls.carry(ids[1], Some(Vec2::new(100.0, 300.0)));
    assert_eq!(s.ball(ids[0]).support, Support::Air);
    assert_eq!(s.ball(ids[1]).support, Support::Ball(ids[0]));
    assert_eq!(s.ball(ids[2]).support, Support::Ball(ids[1]));
}

#[test]
fn dressable_needs_a_standing_three_stack() {
    let mut s = Sim::new(vec![]);
    let three = stack(&mut s.balls, 100.0, 14.0, gkey(), &[26.0, 19.0, 13.0]);
    let two = stack(&mut s.balls, 400.0, 14.0, gkey(), &[26.0, 19.0]);
    assert!(s.balls.dressable(three[0]));
    assert!(!s.balls.dressable(three[1]), "OnBall bottom");
    assert!(!s.balls.dressable(two[0]), "2-stack");
    assert!(!s.balls.dressable(9999), "unknown id");
    let i = s.balls.idx(three[0]).unwrap();
    s.balls.list[i].carried = true;
    assert!(!s.balls.dressable(three[0]), "carried");
    s.balls.list[i].carried = false;
    s.balls.list[i].gone = true;
    assert!(!s.balls.dressable(three[0]), "gone");
}
