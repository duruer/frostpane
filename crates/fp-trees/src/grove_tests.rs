//! Behaviour tests for `Grove` (deterministic: fixed seeds, explicit times).

use super::*;
use fp_core::geom::Rect4;
use fp_core::surface::Surface;
use serde_json::json;

const DT: f64 = 1.0 / 60.0;

fn screen_at(x: f64, surfaces: Vec<Surface>) -> ScreenSnap {
    ScreenSnap { frame: Frame::new(x, 0.0, 1000.0, 700.0), shown: true, ground: true, surfaces }
}

fn dock(x: f64, w: f64) -> Surface {
    Surface::new(SurfaceId::Dock, Frame::new(x, 0.0, w, 40.0))
}

fn win(id: i64, x: f64, y: f64, owner: &str) -> Surface {
    let mut s = Surface::new(SurfaceId::Window(id), Frame::new(x, y, 300.0, 200.0));
    s.owner = owner.to_string();
    s
}

struct Rig {
    g: Grove,
    now: f64,
    snaps: Vec<ScreenSnap>,
    ev: Vec<TreeEvent>,
    pointer: PointerInput,
}

impl Rig {
    fn new(max: u32, surfaces: Vec<Surface>) -> Rig {
        Rig {
            g: Grove::new(max),
            now: 0.0,
            snaps: vec![screen_at(0.0, surfaces)],
            ev: Vec::new(),
            pointer: PointerInput::default(),
        }
    }

    fn tick(&mut self, snowing: bool) {
        self.now += DT;
        self.g.step(DT, self.now, &self.snaps, &self.pointer, snowing, &mut self.ev);
    }

    fn run(&mut self, seconds: f64, snowing: bool) {
        for _ in 0..(seconds / DT).round() as usize {
            self.tick(snowing);
        }
    }

    /// Planted tree, fully grown.
    fn tree(&mut self, seed: u64) -> u64 {
        self.tick(false);
        assert!(self.g.plant(self.now, seed, &mut self.ev));
        let id = self.g.trees.last().unwrap().id;
        self.run(0.6, false);
        id
    }

    fn key(id: u64) -> SpriteKey {
        SpriteKey::new(Owner::Tree, id)
    }

    fn press(&mut self, id: u64, at: Vec2) {
        self.pointer = PointerInput { pos: Some(at), left_down: true, right_down: false, hot: Some(Rig::key(id)) };
        self.tick(false);
    }

    fn drag(&mut self, to: Vec2) {
        self.pointer.pos = Some(to);
        self.tick(false);
    }

    fn release(&mut self) {
        self.pointer.left_down = false;
        self.tick(false);
    }

    fn t(&self, id: u64) -> &Tree {
        self.g.tree(id).unwrap()
    }

    fn t_mut(&mut self, id: u64) -> &mut Tree {
        self.g.tree_mut(id).unwrap()
    }
}

fn snowy(rig: &mut Rig, id: u64) {
    let t = rig.t_mut(id);
    for p in t.patches.iter_mut() {
        p.depth = p.fullest;
    }
    t.cap = 1.0;
}

#[test]
fn planting_needs_a_shelf_then_grows() {
    let mut rig = Rig::new(100, vec![]);
    assert!(!rig.g.plant(0.0, 5, &mut rig.ev));
    rig.tick(false);
    assert!(rig.g.plant(0.0, 5, &mut rig.ev));
    assert_eq!(rig.ev, vec![TreeEvent::Style { id: 1, quip: None }]);
    assert_eq!(rig.g.card_open(), Some(1));
    let t = rig.t(1);
    assert_eq!(t.growth, 0.01);
    assert_eq!(t.foot.y, 4.0);
    assert_eq!(t.look, TreeLook::default());
    assert!(t.foot.x >= 124.0 && t.foot.x <= 876.0);
    let mut sprites = Vec::new();
    rig.g.sprites(&mut sprites);
    assert_eq!(sprites.len(), 1);
    assert_eq!(sprites[0].hit, HitMode::Never);
    rig.run(0.5, false);
    assert_eq!(rig.t(1).growth, 1.0);
    assert_eq!(rig.g.count(), 1);
    assert_eq!(rig.g.look(1), Some(&TreeLook::default()));
    assert!(rig.g.look(9).is_none());
    // visual scale: overshoot while growing, 1 when grown
    let mut t = Tree::new(1, TreeLook::default(), 1, Vec2::ZERO);
    t.growth = 0.5;
    assert!(t.visual_scale() > 0.5);
    t.growth = 0.01;
    assert!(t.visual_scale() >= 0.01);
    assert_eq!(rig.t(1).visual_scale(), 1.0);
    let mut sprites = Vec::new();
    rig.g.sprites(&mut sprites);
    assert_eq!(sprites[0].hit, HitMode::Alpha);
    assert_eq!(sprites[0].opacity, 1.0);
}

#[test]
fn cap_refusals_then_limit() {
    let mut rig = Rig::new(2, vec![]);
    rig.tick(false);
    let a = rig.tree(1);
    let b = rig.tree(2);
    rig.ev.clear();
    rig.t_mut(b).doomed = true;
    assert!(!rig.g.plant(0.0, 3, &mut rig.ev));
    assert_eq!(rig.ev, vec![TreeEvent::Style { id: a, quip: Some(1) }]);
    assert!(!rig.g.plant(0.0, 3, &mut rig.ev));
    assert_eq!(rig.ev[1], TreeEvent::Style { id: a, quip: Some(2) });
    assert!(!rig.g.plant(0.0, 3, &mut rig.ev));
    assert_eq!(rig.ev[2], TreeEvent::LimitReached);
    // counter restarted: next refusal is quip 1 again
    assert!(!rig.g.plant(0.0, 3, &mut rig.ev));
    assert_eq!(rig.ev[3], TreeEvent::Style { id: a, quip: Some(1) });
    // a doomed-only grove has nobody to open a card on
    rig.t_mut(a).doomed = true;
    rig.ev.clear();
    assert!(!rig.g.plant(0.0, 3, &mut rig.ev));
    assert!(rig.ev.is_empty());
    // felling trees do not count
    rig.t_mut(a).felling = true;
    assert_eq!(rig.g.count(), 1);
    assert!(rig.g.plant(0.0, 4, &mut rig.ev));
    // waiting trees count
    let mut r2 = Rig::new(1, vec![]);
    r2.tick(false);
    r2.g.restore(&json!([{"Place": 0}]), 0.0);
    assert!(!r2.g.plant(0.0, 1, &mut r2.ev));
}

#[test]
fn spot_avoids_the_dock_and_existing_trees() {
    // dock in the middle: trees go to the sides
    for seed in 1..20 {
        let mut rig = Rig::new(100, vec![dock(400.0, 200.0)]);
        rig.tick(false);
        assert!(rig.g.plant(0.0, seed, &mut rig.ev));
        let x = rig.t(1).foot.x;
        assert!(!(400.0 - 124.0 + 0.001..600.0 + 124.0).contains(&x), "{x}");
        assert!(rig.t(1).support.unwrap().id == SurfaceId::Ground);
    }
    // dock fills the screen: tree on the dock
    let mut rig = Rig::new(100, vec![dock(10.0, 980.0)]);
    rig.tick(false);
    assert!(rig.g.plant(0.0, 3, &mut rig.ev));
    assert_eq!(rig.t(1).support.unwrap().id, SurfaceId::Dock);
    assert_eq!(rig.t(1).foot.y, 39.0);
    // narrow dock leaves no side room: whole ground
    let mut rig = Rig::new(100, vec![dock(100.0, 200.0)]);
    rig.tick(false);
    assert!(rig.g.plant(0.0, 3, &mut rig.ev));
    assert_eq!(rig.t(1).support.unwrap().id, SurfaceId::Ground);
    // a second tree stays away from the first
    let first = rig.t(1).foot.x;
    assert!(rig.g.plant(0.0, 4, &mut rig.ev));
    assert!((rig.t(2).foot.x - first).abs() > 100.0);
}

#[test]
fn spot_follows_the_pointer_screen() {
    let mut g = Grove::new(0);
    let snaps = [screen_at(0.0, vec![]), screen_at(1000.0, vec![])];
    let mut ev = Vec::new();
    let p = PointerInput { pos: Some(Vec2::new(1500.0, 300.0)), ..PointerInput::default() };
    g.step(DT, 0.0, &snaps, &p, false, &mut ev);
    assert!(!g.plant(0.0, 7, &mut ev));
    ev.clear();
    g.set_max(10);
    assert!(g.plant(0.0, 7, &mut ev));
    assert!(g.trees[0].foot.x > 1000.0);
    // pointer off every screen: first shown ground
    let mut g = Grove::new(10);
    let p = PointerInput { pos: Some(Vec2::new(-500.0, 300.0)), ..PointerInput::default() };
    g.step(DT, 0.0, &snaps, &p, false, &mut ev);
    assert!(g.plant(0.0, 7, &mut ev));
    assert!(g.trees[0].foot.x < 1000.0);
    // nothing shown: first ground
    let mut hidden = snaps.clone();
    hidden[0].shown = false;
    hidden[1].shown = false;
    let mut g = Grove::new(10);
    g.step(DT, 0.0, &hidden, &PointerInput::default(), false, &mut ev);
    assert!(g.plant(0.0, 7, &mut ev));
    assert!(g.trees[0].foot.x < 1000.0);
}

#[test]
fn click_toggles_the_card_and_press_rules() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    rig.g.close_card();
    assert_eq!(rig.g.card_open(), None);
    rig.ev.clear();
    let foot = rig.t(id).foot;
    // a press without a hot sprite does nothing
    rig.pointer = PointerInput { pos: Some(foot), left_down: true, right_down: false, hot: None };
    rig.tick(false);
    assert!(rig.t(id).held.is_none());
    rig.pointer.left_down = false;
    rig.tick(false);
    // click opens
    rig.press(id, foot + Vec2::new(0.0, 50.0));
    assert!(rig.t(id).held.is_some());
    // a pointer-less frame while pressed changes nothing
    rig.pointer.pos = None;
    rig.tick(false);
    rig.pointer.pos = Some(foot);
    // small wiggle stays a click
    rig.drag(foot + Vec2::new(2.0, 51.0));
    assert!(!rig.t(id).lifted);
    rig.release();
    assert_eq!(rig.ev, vec![TreeEvent::Style { id, quip: None }]);
    assert_eq!(rig.g.card_open(), Some(id));
    // click again closes
    rig.press(id, foot);
    rig.release();
    assert_eq!(rig.g.card_open(), None);
    // doomed trees ignore presses
    rig.t_mut(id).doomed = true;
    rig.press(id, foot);
    assert!(rig.t(id).held.is_none());
    rig.pointer.left_down = false;
    rig.tick(false);
    let mut sprites = Vec::new();
    rig.g.sprites(&mut sprites);
    assert_eq!(sprites[0].hit, HitMode::Never);
}

#[test]
fn second_press_while_held_is_ignored() {
    let mut rig = Rig::new(100, vec![]);
    let a = rig.tree(1);
    let b = rig.tree(2);
    let at = rig.t(a).foot;
    rig.press(a, at);
    assert!(rig.t(a).held.is_some());
    rig.g.prev_down = false;
    rig.pointer.hot = Some(Rig::key(b));
    rig.tick(false);
    assert!(rig.t(b).held.is_none());
    rig.pointer.left_down = false;
    rig.tick(false);
}

#[test]
fn drag_lift_throw_and_land() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    snowy(&mut rig, id);
    let other = rig.tree(9);
    rig.g.card = Some(id);
    let foot = rig.t(id).foot;
    rig.press(id, foot + Vec2::new(10.0, 30.0));
    rig.drag(foot + Vec2::new(10.0, 60.0));
    assert!(rig.t(id).lifted);
    assert_eq!(rig.g.card_open(), None);
    assert!(rig.t(id).support.is_none());
    // the lifted tree went to the front of the sprite order
    assert_eq!(rig.g.trees.last().unwrap().id, id);
    assert!(rig.g.trees.iter().any(|t| t.id == other));
    let mut sprites = Vec::new();
    rig.g.sprites(&mut sprites);
    assert_eq!(sprites.last().unwrap().hit, HitMode::Alpha);
    // drag fast: clumps shake off, tilt follows the motion
    for i in 0..30 {
        rig.drag(foot + Vec2::new(10.0 + 30.0 * i as f64, 200.0));
    }
    assert!(rig.t(id).tilt != 0.0);
    assert!(rig.g.clump_count() > 0);
    // release: thrown, falls and lands on the ground
    rig.release();
    assert!(rig.t(id).support.is_none());
    assert!(rig.t(id).vel.y <= 0.0);
    rig.run(3.0, false);
    let t = rig.t(id);
    assert_eq!(t.support.unwrap().id, SurfaceId::Ground);
    assert_eq!(t.foot.y, 4.0);
    assert!(t.foot.x >= 8.0 && t.foot.x <= 992.0);
    assert_eq!(t.across, t.foot.x - 500.0);
}

#[test]
fn hard_landing_shakes_snow_and_tilts() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    snowy(&mut rig, id);
    rig.t_mut(id).support = None;
    rig.t_mut(id).foot = Vec2::new(500.0, 600.0);
    let before = rig.t(id).snow_total();
    rig.run(1.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Ground);
    assert!(rig.t(id).snow_total() < before);
    // a tilt wobble that settles
    rig.run(4.0, false);
    assert_eq!(rig.t(id).tilt, 0.0);
}

#[test]
fn trees_ride_windows_and_fall_when_they_vanish() {
    let mut rig = Rig::new(100, vec![win(7, 300.0, 100.0, "app.exe")]);
    let id = rig.tree(1);
    // put the tree on the window
    rig.t_mut(id).support = Some(LedgeKey::new(Vec2::ZERO, SurfaceId::Window(7)));
    rig.t_mut(id).across = 0.0;
    rig.run(0.1, false);
    assert_eq!(rig.t(id).foot, Vec2::new(450.0, 299.0));
    // window moves: tree follows with the window's velocity
    rig.snaps[0].surfaces[0].frame.x = 330.0;
    rig.tick(false);
    assert_eq!(rig.t(id).foot.x, 480.0);
    assert!(rig.t(id).vel.x > 0.0);
    // blocked by a window above: hidden
    rig.snaps[0].surfaces[0].blockers.push(Rect4::new(0.0, 0.0, 1000.0, 1000.0));
    rig.tick(false);
    assert!(rig.t(id).hidden);
    let mut sprites = Vec::new();
    rig.g.sprites(&mut sprites);
    assert_eq!(sprites[0].opacity, 0.0);
    assert_eq!(sprites[0].hit, HitMode::Never);
    // hidden trees collect no snow
    rig.run(4.0, true);
    assert_eq!(rig.t(id).snow_total(), 0.0);
    rig.snaps[0].surfaces[0].blockers.clear();
    // window teleports: the tree falls to the ground
    rig.snaps[0].surfaces[0].frame = Frame::new(0.0, 500.0, 300.0, 200.0);
    rig.tick(false);
    assert!(rig.t(id).support.is_none());
    rig.run(2.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Ground);
    // window gone while standing on it
    rig.t_mut(id).support = Some(LedgeKey::new(Vec2::ZERO, SurfaceId::Window(7)));
    rig.snaps[0].surfaces.clear();
    rig.run(0.2, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Ground);
    // window shrinks past the foot: unsupported
    rig.snaps[0].surfaces.push(win(7, 600.0, 100.0, "app.exe"));
    rig.tick(false);
    rig.t_mut(id).support = Some(LedgeKey::new(Vec2::ZERO, SurfaceId::Window(7)));
    rig.t_mut(id).across = -400.0;
    rig.tick(false);
    assert!(rig.t(id).support.is_none());
}

#[test]
fn falling_trees_land_on_the_highest_window_top() {
    let mut rig = Rig::new(100, vec![win(7, 300.0, 100.0, "a.exe"), win(8, 300.0, 200.0, "b.exe")]);
    let id = rig.tree(1);
    rig.t_mut(id).support = None;
    rig.t_mut(id).foot = Vec2::new(450.0, 650.0);
    rig.run(2.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Window(8));
    assert_eq!(rig.t(id).foot.y, 399.0);
    // a blocker on the top lets it through to the lower window
    rig.snaps[0].surfaces[1].blockers.push(Rect4::new(0.0, 0.0, 1000.0, 1000.0));
    rig.t_mut(id).support = None;
    rig.t_mut(id).foot = Vec2::new(450.0, 650.0);
    rig.run(2.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Window(7));
    // hidden shelf: ignored too
    rig.snaps[0].shown = false;
    rig.t_mut(id).support = None;
    rig.t_mut(id).foot = Vec2::new(450.0, 650.0);
    rig.run(2.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Ground);
    // doomed trees only accept the dock
    rig.snaps[0].shown = true;
    rig.snaps[0].surfaces[1].blockers.clear();
    rig.t_mut(id).doomed = true;
    rig.t_mut(id).support = None;
    rig.t_mut(id).foot = Vec2::new(450.0, 650.0);
    rig.run(2.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Ground);
}

#[test]
fn snow_builds_only_while_snowing_and_caps_out() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    let total = |r: &Rig| r.t(id).snow_total();
    rig.run(7.0, false);
    assert_eq!(total(&rig), 0.0);
    rig.run(30.0, true);
    let early = total(&rig);
    assert!(early > 0.0 && rig.t(id).cap > 0.0);
    rig.run(1200.0, true);
    assert!(total(&rig) > early);
    assert!(rig.t(id).patches.iter().all(|p| p.depth <= p.fullest + 1e-12));
    assert_eq!(rig.t(id).cap, 1.0);
    // pressed trees collect none
    let foot = rig.t(id).foot;
    for p in rig.t_mut(id).patches.iter_mut() {
        p.depth = 0.0;
    }
    rig.press(id, foot);
    rig.run(10.0, true);
    assert_eq!(total(&rig), 0.0);
    rig.release();
}

#[test]
fn shake_off_rules() {
    let mut rng = Lcg64::new(1);
    let mut spawns = Vec::new();
    let mut t = Tree::new(1, TreeLook::default(), 4, Vec2::new(100.0, 4.0));
    // too little snow: nothing happens
    t.shake_off(&mut rng, 1.0, &mut spawns);
    assert!(spawns.is_empty() && !t.snow_dirty);
    for p in t.patches.iter_mut() {
        p.depth = p.fullest;
        p.cling = 0.0;
    }
    t.cap = 1.0;
    t.tilt = 0.2;
    let total = t.snow_total();
    t.shake_off(&mut rng, 0.3, &mut spawns);
    assert!(t.snow_total() < total);
    assert!(!spawns.is_empty());
    assert!(t.shed > 0.0 && t.shed <= t.shed_limit + 1e-9);
    assert!(spawns.iter().all(|s| s.radius >= 2.5 && s.radius <= t.shape.unit() * 1.2 + 1e-9));
    // the shed limit stops a long shake
    t.shed = t.shed_limit;
    let n = spawns.len();
    t.shake_off(&mut rng, 1.0, &mut spawns);
    assert_eq!(spawns.len(), n);
    // enough snow, but patches that never let go: attempts run out
    let mut t = Tree::new(1, TreeLook::default(), 4, Vec2::ZERO);
    for p in t.patches.iter_mut() {
        p.depth = p.fullest;
        p.cling = 1.0;
    }
    t.cap = 0.0;
    let mut sp = Vec::new();
    t.shake_off(&mut rng, 1.0, &mut sp);
    assert!(sp.is_empty());
    // a cap hit throws the cap in two
    t.cap = 1.0;
    let mut rng = Lcg64::new(3);
    for _ in 0..20 {
        t.shed = 0.0;
        t.cap = 1.0;
        t.shake_off(&mut rng, 0.3, &mut sp);
    }
    assert!(sp.iter().any(|s| s.radius == 4.0));
}

#[test]
fn shake_stops_when_slow_and_resets_the_shed() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    rig.t_mut(id).shed = 0.5;
    rig.t_mut(id).shaken_at = rig.now - 1.0;
    rig.tick(false);
    assert_eq!(rig.t(id).shed, 0.0);
}

#[test]
fn restyle_keeps_average_snow() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    for p in rig.t_mut(id).patches.iter_mut() {
        p.depth = 0.2;
    }
    let rev = rig.t(id).rev;
    rig.g.restyle(id, TreeLook { level: 9, topper: None, trims: vec![Trim::Stars, Trim::Bows] });
    let t = rig.t(id);
    assert_eq!(t.look.level, 5);
    assert_eq!(t.shape.level, 5);
    assert_eq!(t.patches.len(), generate_patches(&TreeShape::from_level(5), 1).len());
    assert!(t.patches.iter().all(|p| p.depth <= p.fullest + 1e-12 && p.depth > 0.0));
    assert!(t.tilt_speed > 1.0);
    assert!(t.rev > rev);
    // same level: patches untouched
    let depths: Vec<f64> = t.patches.iter().map(|p| p.depth).collect();
    rig.g.restyle(id, TreeLook { level: 5, topper: Some(Topper::Bell), trims: vec![] });
    let after: Vec<f64> = rig.t(id).patches.iter().map(|p| p.depth).collect();
    assert_eq!(depths, after);
    // unknown id
    rig.g.restyle(99, TreeLook::default());
    rig.ev.clear();
    rig.tick(false);
    assert!(rig.ev.contains(&TreeEvent::Dirty));
}

#[test]
fn chopping_fells_a_doomed_tree() {
    let mut rig = Rig::new(100, vec![dock(0.0, 1000.0)]);
    let id = rig.tree(1);
    snowy(&mut rig, id);
    rig.g.doom(99);
    assert!(rig.g.doomed().is_none());
    rig.g.card = Some(id);
    rig.g.doom(id);
    assert_eq!(rig.g.card_open(), None);
    assert!(rig.t(id).doomed);
    assert!(rig.g.standing().is_empty());
    let (tid, point, reach) = rig.g.yeti_target().unwrap();
    assert_eq!(tid, id);
    assert_eq!(point.y, rig.t(id).foot.y + rig.t(id).shape.height * 0.3);
    assert!(reach > 24.0);
    assert_eq!(rig.g.doomed(), Some((id, point, reach)));
    rig.g.chop(99);
    rig.g.chop_from(id, point.x - 100.0);
    assert_eq!(rig.t(id).fall_way, -1.0);
    assert_eq!(rig.t(id).chops, 1);
    rig.g.chop_from(id, point.x + 100.0);
    assert_eq!(rig.t(id).fall_way, 1.0);
    rig.g.chop(id);
    assert!(rig.t(id).felling);
    rig.ev.clear();
    rig.tick(false);
    assert!(rig.ev.iter().any(|e| matches!(e, TreeEvent::Snow { area, .. } if *area == 24.0)));
    // felled trees no longer chop or count
    rig.g.chop(id);
    assert_eq!(rig.t(id).chops, 3);
    assert!(rig.g.doomed().is_none());
    assert_eq!(rig.g.count(), 0);
    rig.run(0.2, false);
    assert!(rig.t(id).tilt > 0.0);
    rig.g.card = Some(id);
    rig.run(1.5, false);
    assert!(rig.g.tree(id).is_none());
    assert_eq!(rig.g.card_open(), None);
}

#[test]
fn doomed_window_trees_fall_and_hidden_ones_fell_quietly() {
    let mut rig = Rig::new(100, vec![win(7, 300.0, 100.0, "a.exe")]);
    let id = rig.tree(1);
    rig.t_mut(id).support = Some(LedgeKey::new(Vec2::ZERO, SurfaceId::Window(7)));
    rig.run(0.1, false);
    rig.g.doom(id);
    assert!(rig.t(id).support.is_none());
    rig.run(2.0, false);
    assert_eq!(rig.t(id).support.unwrap().id, SurfaceId::Ground);
    rig.t_mut(id).hidden = true;
    rig.ev.clear();
    for _ in 0..3 {
        rig.g.chop(id);
    }
    assert!(rig.t(id).felling);
    rig.ev.clear();
    rig.tick(false);
    assert_eq!(rig.ev, vec![TreeEvent::Dirty]);
}

#[test]
fn trees_fall_onto_the_nearest_of_two_screens() {
    let mut g = Grove::new(10);
    let snaps = [screen_at(0.0, vec![]), screen_at(1000.0, vec![])];
    let mut ev = Vec::new();
    g.step(DT, 0.0, &snaps, &PointerInput::default(), false, &mut ev);
    let mut t = Tree::new(1, TreeLook::default(), 1, Vec2::new(1500.0, 300.0));
    t.support = None;
    g.trees.push(t);
    g.step(DT, DT, &snaps, &PointerInput::default(), false, &mut ev);
    // between the screens it is pulled to the nearest edge
    g.trees[0].foot.x = 1004.0;
    g.step(DT, 2.0 * DT, &snaps, &PointerInput::default(), false, &mut ev);
    assert_eq!(g.trees[0].foot.x, 1008.0, "clamped 8 pt inside the screen it is over");
    for i in 0..120 {
        g.step(DT, 3.0 * DT + f64::from(i) * DT, &snaps, &PointerInput::default(), false, &mut ev);
    }
    let key = g.trees[0].support.unwrap();
    assert_eq!(key.origin, Vec2::new(1000.0, 0.0));
}

#[test]
fn blink_and_snow_redraw_bump_the_revision() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    let r0 = rig.t(id).rev;
    rig.run(1.0, false);
    assert!(rig.t(id).rev > r0, "blink toggles every 0.8 s");
    rig.t_mut(id).look = TreeLook { level: 2, topper: None, trims: vec![] };
    let r1 = rig.t(id).rev;
    rig.run(2.0, false);
    assert_eq!(rig.t(id).rev, r1, "no lights, nothing moving");
    rig.run(10.0, true);
    assert!(rig.t(id).rev > r1);
}

#[test]
fn save_shape_and_abort_rules() {
    let mut rig = Rig::new(100, vec![dock(400.0, 200.0), win(7, 300.0, 100.0, "app.exe")]);
    assert_eq!(rig.g.save_json(), Some(json!([])));
    let a = rig.tree(1);
    let b = rig.tree(2);
    let c = rig.tree(3);
    rig.t_mut(b).support = Some(LedgeKey::new(Vec2::ZERO, SurfaceId::Dock));
    rig.t_mut(b).across = 0.0;
    rig.t_mut(c).support = Some(LedgeKey::new(Vec2::ZERO, SurfaceId::Window(7)));
    rig.t_mut(c).across = -20.0;
    rig.run(0.1, false);
    let v = rig.g.save_json().unwrap();
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0]["Place"], 0);
    assert_eq!(list[0]["Across"], rig.t(a).foot.x);
    assert_eq!(list[0]["Owner"], "");
    assert_eq!(list[0]["Look"]["Topper"], "Star");
    assert_eq!(list[0]["Look"]["Trims"], json!(["Baubles", "Lights"]));
    assert_eq!(list[1]["Place"], 1);
    assert_eq!(list[2]["Place"], 2);
    assert_eq!(list[2]["Id"], 7);
    assert_eq!(list[2]["Owner"], "app.exe");
    assert_eq!(list[2]["Middle"], 450.0);
    assert_eq!(list[2]["Top"], 300.0);
    assert_eq!(list[2]["Across"], -20.0);
    assert!(list[0]["Depths"].is_string());
    // felling trees are left out
    rig.t_mut(a).felling = true;
    assert_eq!(rig.g.save_json().unwrap().as_array().unwrap().len(), 2);
    rig.t_mut(a).felling = false;
    // unknown owner aborts
    rig.snaps[0].surfaces[1].owner.clear();
    rig.tick(false);
    assert!(rig.g.save_json().is_none());
    // missing shelf aborts
    rig.snaps[0].surfaces.clear();
    rig.tick(false);
    assert!(rig.g.save_json().is_none());
    // unsupported aborts
    rig.t_mut(a).support = None;
    assert!(rig.g.save_json().is_none());
}

#[test]
fn autosave_cadence_and_unchanged_text() {
    let mut rig = Rig::new(100, vec![]);
    rig.tick(false);
    assert!(!rig.ev.contains(&TreeEvent::Dirty), "nothing to save, first autosave waits");
    rig.run(7.0, false);
    // an empty grove saves "[]" once, then never again
    assert_eq!(rig.ev.iter().filter(|e| **e == TreeEvent::Dirty).count(), 1);
    rig.ev.clear();
    rig.run(13.0, false);
    assert!(rig.ev.is_empty());
    // planting forces a save right away
    assert!(rig.g.plant(rig.now, 3, &mut rig.ev));
    rig.ev.clear();
    rig.tick(false);
    assert_eq!(rig.ev, vec![TreeEvent::Dirty]);
    // held trees block saving
    let id = rig.g.trees[0].id;
    let foot = rig.t(id).foot;
    rig.run(0.6, false);
    rig.press(id, foot);
    rig.g.force_save = true;
    rig.ev.clear();
    rig.tick(false);
    assert!(rig.ev.is_empty());
    rig.release();
}

#[test]
fn restore_places_trees_on_their_shelves() {
    let saved = |place: u8, id: i64, owner: &str, middle: f64, top: f64, across: f64| {
        json!({"Place": place, "Id": id, "Owner": owner, "Middle": middle, "Top": top, "ScreenX": 0.0, "ScreenY": 0.0,
               "Across": across, "Look": {"Level": 1, "Topper": null, "Trims": ["Candy"]}, "Seed": 77, "Depths": "", "Cap": 255})
    };
    let list = json!([
        saved(0, 0, "", 0.0, 0.0, 300.0),
        saved(1, 0, "", 0.0, 0.0, 500.0),
        saved(2, 7, "app.exe", 450.0, 300.0, 30.0),
        saved(2, 99, "app.exe", 455.0, 305.0, -10.0),
        saved(2, 98, "other.exe", 0.0, 0.0, 0.0),
    ]);
    let mut rig = Rig::new(100, vec![dock(400.0, 200.0), win(7, 300.0, 100.0, "app.exe")]);
    rig.g.restore(&list, 0.0);
    assert_eq!(rig.g.waiting.len(), 5);
    rig.run(0.5, false);
    assert_eq!(rig.g.trees.len(), 4);
    assert_eq!(rig.g.waiting.len(), 1, "the unknown owner is still waiting");
    let xs: Vec<f64> = rig.g.trees.iter().map(|t| t.foot.x).collect();
    assert!(xs.contains(&300.0) && xs.contains(&500.0) && xs.contains(&480.0) && xs.contains(&440.0), "{xs:?}");
    assert!(rig.g.trees.iter().all(|t| t.growth == 1.0 && t.look.level == 1 && t.cap == 1.0));
    // the waiting tree is part of every save
    assert_eq!(rig.g.save_json().unwrap().as_array().unwrap().len(), 5);
    // 180 s later it falls back to the ground at saved.Middle + Across
    rig.run(181.0, false);
    assert!(rig.g.waiting.is_empty());
    assert!(rig.g.trees.iter().any(|t| t.foot.x == 8.0));
}

#[test]
fn restore_fallbacks_and_depth_rules() {
    let mut rig = Rig::new(100, vec![]);
    let n = generate_patches(&TreeShape::from_level(2), 5).len();
    let exact = b64_encode(&vec![255u8; n]);
    let mean = b64_encode(&[255u8, 255, 0, 0]);
    let list = json!([
        {"Place": 1, "Across": 600.0, "Seed": 5, "Depths": exact, "Cap": 51},
        {"Place": 0, "Across": 5000.0, "Seed": 5, "Depths": mean},
        {"Place": 0, "Across": 100.0, "Seed": 5, "Depths": null, "Look": null, "ScreenX": 5000.0},
        {"Place": 2, "Id": 5, "Owner": "x", "Middle": 500.0, "Across": 40.0, "Seed": 5, "Depths": "!!"},
    ]);
    rig.g.restore(&list, 0.0);
    rig.run(1.0, false);
    // the dock entry waits for its dock, the window one for its window
    assert_eq!(rig.g.trees.len(), 2);
    assert_eq!(rig.g.waiting.len(), 2);
    rig.run(6.0, false);
    assert_eq!(rig.g.trees.len(), 3);
    rig.run(180.0, false);
    assert_eq!(rig.g.trees.len(), 4);
    let t = rig.g.trees.iter().find(|t| t.cap > 0.19 && t.cap < 0.21).unwrap();
    assert!(t.patches.iter().all(|p| (p.depth - 1.0).abs() < 1e-12));
    let t = &rig.g.trees[0];
    assert_eq!(t.foot.x, 992.0, "clamped inside the screen");
    assert!(t.patches.iter().all(|p| p.depth > 0.0 && p.depth <= p.fullest + 1e-12));
    let t = &rig.g.trees[1];
    assert_eq!(t.snow_total(), 0.0);
    assert_eq!(t.look, TreeLook::default());
    let t = &rig.g.trees[3];
    assert_eq!(t.snow_total(), 0.0);
    assert_eq!(t.foot.x, 540.0);
    // garbage lists load as empty
    rig.g.restore(&json!({"nope": 1}), 0.0);
    assert!(rig.g.waiting.is_empty());
}

#[test]
fn restore_picks_the_nearest_same_owner_window() {
    let list =
        json!([{"Place": 2, "Id": 1, "Owner": "a.exe", "Middle": 450.0, "Top": 300.0, "Across": 0.0, "Seed": 1}]);
    let mut rig = Rig::new(
        100,
        vec![
            win(10, 300.0, 100.0, "a.exe"),
            win(11, 330.0, 110.0, "a.exe"),
            win(12, 0.0, 0.0, "b.exe"),
            win(13, 300.0, 100.0, "a.exe"),
        ],
    );
    rig.g.restore(&list, 0.0);
    rig.run(0.5, false);
    assert_eq!(rig.g.trees.len(), 1);
    assert_eq!(rig.g.trees[0].support.unwrap().id, SurfaceId::Window(10));
    // too far from the saved spot: no match
    let far = json!([{"Place": 2, "Id": 1, "Owner": "a.exe", "Middle": 50.0, "Top": 30.0, "Across": 0.0, "Seed": 1}]);
    let mut rig = Rig::new(100, vec![win(10, 300.0, 100.0, "a.exe")]);
    rig.g.restore(&far, 0.0);
    rig.run(0.5, false);
    assert_eq!(rig.g.waiting.len(), 1);
}

#[test]
fn stop_saves_and_forgets() {
    let mut rig = Rig::new(100, vec![]);
    rig.tree(1);
    rig.g.card = Some(1);
    rig.g.restore(&json!([{"Place": 0}]), 0.0);
    let saved = rig.g.stop().unwrap();
    assert_eq!(saved.as_array().unwrap().len(), 2);
    assert_eq!(rig.g.count(), 0);
    assert!(rig.g.waiting.is_empty() && rig.g.shelves.is_empty());
    assert_eq!(rig.g.card_open(), None);
    assert!(!rig.g.plant(0.0, 1, &mut rig.ev));
}

#[test]
fn train_cargo_round_trip() {
    let mut rig = Rig::new(2, vec![]);
    let id = rig.tree(1);
    let other = rig.tree(2);
    rig.t_mut(id).look = TreeLook { level: 3, topper: Some(Topper::Bell), trims: vec![Trim::Candy] };
    rig.t_mut(id).shape = TreeShape::from_level(3);
    rig.t_mut(id).patches = generate_patches(&TreeShape::from_level(3), 1);
    snowy(&mut rig, id);
    assert_eq!(rig.g.standing().len(), 2);
    rig.t_mut(other).growth = 0.5;
    assert_eq!(rig.g.standing().len(), 1);
    rig.t_mut(other).growth = 1.0;
    // carry the first tree: it follows the point and is not standing
    rig.g.card = Some(id);
    rig.g.carry(Some(id), Some(Vec2::new(300.0, 200.0)));
    assert_eq!(rig.g.card_open(), None);
    assert_eq!(rig.g.trees.last().unwrap().id, id);
    rig.tick(false);
    assert_eq!(rig.t(id).foot, Vec2::new(300.0, 200.0));
    assert!(rig.t(id).support.is_none());
    assert_eq!(rig.g.standing().len(), 1);
    // keeps its last foot without a new point, carrying again is a no-op
    rig.g.carry(Some(id), None);
    rig.tick(false);
    assert_eq!(rig.t(id).foot, Vec2::new(300.0, 200.0));
    // saving is blocked while carried, even when one is due
    assert!(rig.g.save_json().is_none());
    rig.g.force_save = true;
    rig.ev.clear();
    rig.tick(false);
    assert!(rig.ev.is_empty() && rig.g.force_save);
    // release: it falls and lands
    rig.g.carry(None, None);
    rig.run(2.0, false);
    assert!(rig.t(id).support.is_some());
    // take it away
    assert!(rig.g.take(99).is_none());
    rig.g.card = Some(id);
    let cargo = rig.g.take(id).unwrap();
    assert!(rig.g.take(id).is_none(), "already leaving");
    assert_eq!(rig.g.card_open(), None);
    rig.run(0.5, false);
    assert!(rig.g.tree(id).is_none());
    // board creates a carried tree even at the cap
    let boarded = rig.g.board(&cargo, Vec2::new(10.0, 20.0)).unwrap();
    assert!(rig.t(boarded).carried);
    assert_eq!(rig.t(boarded).look.level, 3);
    assert!(rig.t(boarded).cap > 0.0);
    assert!(rig.t(boarded).patches.iter().all(|p| p.depth <= p.fullest + 0.005));
    assert!(rig.g.board(&json!("junk"), Vec2::ZERO).is_none());
    // vanish
    rig.g.vanish(99);
    rig.g.card = Some(boarded);
    rig.g.vanish(boarded);
    assert_eq!(rig.g.card_open(), None);
    rig.tick(false);
    assert!(rig.g.tree(boarded).is_none());
    // plant the cargo: grows from 0.01, spreads away from the other tree
    let screen = Frame::new(0.0, 0.0, 1000.0, 700.0);
    rig.t_mut(other).foot.x = 500.0;
    let ox = 500.0;
    assert!(rig.g.plant_cargo(&cargo, ox, screen));
    let planted = rig.g.trees.last().unwrap();
    assert_eq!(planted.growth, 0.01);
    assert_eq!(planted.look.level, 3);
    assert!((planted.foot.x - ox).abs() >= 124.0 + 0.6 * 94.0, "{}", planted.foot.x);
    assert!(planted.foot.x >= 124.0 + 8.0 && planted.foot.x <= 1000.0 - 124.0 - 8.0);
    // wrong monitor or bad cargo
    assert!(!rig.g.plant_cargo(&cargo, 100.0, Frame::new(5000.0, 0.0, 10.0, 10.0)));
    assert!(!rig.g.plant_cargo(&json!(7), 100.0, screen));
    // trees on both sides still leave a spot
    let px = rig.g.trees.last().unwrap().foot.x;
    assert!(rig.g.plant_cargo(&cargo, px, screen));
    assert!(rig.g.plant_cargo(&cargo, ox - 1.0, screen));
}

#[test]
fn paint_and_sprites_for_trees_and_clumps() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(2);
    snowy(&mut rig, id);
    assert!(rig.g.paint(99, 1.0).is_none());
    let pm = rig.g.paint(id, 1.0).unwrap();
    let (w, h) = rig.t(id).shape.canvas();
    assert_eq!((pm.width(), pm.height()), (w.ceil() as u32, h.ceil() as u32));
    assert!(pm.pixels().iter().filter(|p| p.alpha() > 100).count() > 2000);
    rig.g.blink ^= 1;
    let other = rig.g.paint(id, 1.0).unwrap();
    assert!(pm.data() != other.data());
    // shake snow loose so a clump overlay exists
    for p in rig.t_mut(id).patches.iter_mut() {
        p.cling = 0.0;
    }
    rig.t_mut(id).vel = Vec2::new(2000.0, 0.0);
    let mut spawns = Vec::new();
    for _ in 0..5 {
        rig.g.trees[0].shed = 0.0;
        rig.g.trees[0].shake_off(&mut rig.g.rng, 0.3, &mut spawns);
    }
    for s in spawns {
        rig.g.clumps.spawn(&rig.g.shelves, &mut rig.g.rng, s);
    }
    rig.tick(false);
    let mut sprites = Vec::new();
    rig.g.sprites(&mut sprites);
    let clump = sprites.iter().find(|s| s.key.id() >= CLUMP_BASE).unwrap();
    assert!(rig.g.paint(clump.key.id(), 1.0).is_some());
    assert!(clump.frame.w > 0.0);
}

#[test]
fn style_card_placement() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    let work = Frame::new(0.0, 40.0, 1000.0, 660.0);
    rig.t_mut(id).foot = Vec2::new(200.0, 4.0);
    let o = rig.g.card_origin(id, work).unwrap();
    assert_eq!(o.x, 200.0 + 94.0 + 18.0 - 22.0);
    assert_eq!(o.y, 40.0 + 70.0 - 22.0);
    // too close to the right edge: on the left
    rig.t_mut(id).foot = Vec2::new(900.0, 4.0);
    assert_eq!(rig.g.card_origin(id, work).unwrap().x, 900.0 - 94.0 - 18.0 - 340.0 - 22.0);
    // no room on the left either: clamped to the margin
    rig.t_mut(id).foot = Vec2::new(300.0, 4.0);
    let narrow = Frame::new(0.0, 40.0, 500.0, 660.0);
    assert_eq!(rig.g.card_origin(id, narrow).unwrap().x, 8.0 - 22.0);
    // tall trees are held below the top edge
    rig.t_mut(id).foot = Vec2::new(200.0, 600.0);
    assert_eq!(rig.g.card_origin(id, work).unwrap().y, 700.0 - 420.0 - 40.0 - 22.0);
    assert!(rig.g.card_origin(99, work).is_none());
    assert_eq!(CARD_SIZE, (340.0, 420.0));
}

#[test]
fn raise_and_default() {
    let mut rig = Rig::new(100, vec![]);
    let a = rig.tree(1);
    let b = rig.tree(2);
    rig.g.raise(a);
    assert_eq!(rig.g.trees.last().unwrap().id, a);
    rig.g.raise(99);
    assert_eq!(rig.g.trees.last().unwrap().id, a);
    assert_ne!(a, b);
    assert_eq!(Grove::default().max, DEFAULT_MAX_PLACED);
}

#[test]
fn doom_newest_picks_the_hunt_target() {
    let mut rig = Rig::new(100, vec![]);
    assert!(!rig.g.doom_newest(), "no trees: normal yeti");
    let a = rig.tree(1);
    let b = rig.tree(2);
    let c = rig.tree(3);
    // newest visible tree is doomed (c hidden, so b)
    rig.t_mut(c).hidden = true;
    rig.g.card = Some(b);
    assert!(rig.g.doom_newest());
    assert!(rig.t(b).doomed && !rig.t(a).doomed && !rig.t(c).doomed);
    assert_eq!(rig.g.card_open(), None);
    // already doomed: nothing new
    assert!(rig.g.doom_newest());
    assert_eq!(rig.g.trees.iter().filter(|t| t.doomed).count(), 1);
    // felling trees are not targets; every standing tree hidden: newest one
    rig.t_mut(b).felling = true;
    rig.t_mut(a).hidden = true;
    assert!(rig.g.doom_newest());
    assert!(rig.t(c).doomed && !rig.t(a).doomed);
    // only felling trees left: normal yeti
    rig.t_mut(a).felling = true;
    rig.t_mut(c).felling = true;
    assert!(!rig.g.doom_newest());
}

#[test]
fn carrying_another_tree_without_a_point_keeps_its_own_foot() {
    let mut rig = Rig::new(100, vec![]);
    let a = rig.tree(1);
    let b = rig.tree(2);
    rig.g.carry(Some(a), Some(Vec2::new(300.0, 200.0)));
    rig.tick(false);
    assert_eq!(rig.t(a).foot, Vec2::new(300.0, 200.0));
    let b_foot = rig.t(b).foot;
    rig.g.carry(Some(b), None);
    rig.tick(false);
    assert!(rig.t(b).carried && !rig.t(a).carried);
    assert_eq!(rig.t(b).foot, b_foot, "must not jump to the old carry point");
    assert!(rig.t(b).support.is_none() && rig.t(b).vel == Vec2::ZERO);
}

#[test]
fn restyle_without_patches_and_paint_while_vanishing() {
    let mut rig = Rig::new(100, vec![]);
    let id = rig.tree(1);
    rig.t_mut(id).patches.clear();
    rig.g.restyle(id, TreeLook { level: 0, ..TreeLook::default() });
    let t = rig.t(id);
    assert!(!t.patches.is_empty());
    assert!(t.patches.iter().all(|p| p.depth == 0.0), "no NaN from an empty average");
    // a vanished tree is still paintable (scale 0) until the next step drops it
    rig.g.vanish(id);
    assert!(rig.g.paint(id, 1.0).is_some());
    rig.tick(false);
    assert!(rig.g.paint(id, 1.0).is_none());
}

#[test]
fn pending_events_are_flushed_by_the_next_step() {
    let mut rig = Rig::new(100, vec![]);
    rig.g.pending.push(TreeEvent::LimitReached);
    rig.tick(false);
    assert!(rig.ev.contains(&TreeEvent::LimitReached));
}

#[test]
fn card_anchor_is_the_ground_strip_of_the_foot() {
    let mut rig = Rig::new(3, vec![]);
    rig.snaps = vec![screen_at(0.0, vec![]), screen_at(1000.0, vec![])];
    let id = rig.tree(7);
    let foot = rig.g.tree(id).unwrap().foot;
    let a = rig.g.card_anchor(id).unwrap();
    let strip_x = if foot.x < 1000.0 { 500.0 } else { 1500.0 };
    assert_eq!(a, Vec2::new(strip_x, 10.0));
    // foot outside every strip: nearest centre
    rig.g.tree_mut(id).unwrap().foot.x = 5000.0;
    assert_eq!(rig.g.card_anchor(id).unwrap().x, 1500.0);
    assert!(rig.g.card_anchor(99).is_none());
    rig.g.shelves.clear();
    assert!(rig.g.card_anchor(id).is_none());
}
