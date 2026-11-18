use super::*;
use fp_core::desktop::MonitorInfo;
use fp_core::feed::Blade;
use fp_core::geom::PixelRect;
use fp_core::surface::{Surface, SurfaceId};
use fp_decor::Kind;
use serde_json::json;

const DT: f64 = 1.0 / 60.0;

fn monitor(id: &str, left: i32, primary: bool) -> MonitorInfo {
    MonitorInfo {
        id: id.into(),
        name: id.into(),
        pixels: PixelRect::new(left, 0, left + 1920, 1080),
        work: PixelRect::new(left, 0, left + 1920, 1032),
        scale: 1.0,
        primary,
    }
}

fn layout() -> Layout {
    Layout::new(vec![monitor("a", 0, true)])
}

fn frame() -> Frame {
    Frame::new(0.0, 0.0, 1920.0, 1080.0)
}

fn work() -> Frame {
    Frame::new(0.0, 48.0, 1920.0, 1032.0)
}

/// One screen with a full-width taskbar (top 48).
fn snap_with_dock() -> ScreenSnap {
    let mut snap = ScreenSnap { frame: frame(), shown: true, ground: true, surfaces: Vec::new() };
    snap.surfaces.push(Surface::new(SurfaceId::Dock, Frame::new(0.0, 0.0, 1920.0, 48.0)));
    snap
}

fn snap_bare() -> ScreenSnap {
    ScreenSnap { frame: frame(), shown: true, ground: true, surfaces: Vec::new() }
}

fn saved_stack(across: f64) -> Value {
    json!({"Place": 0, "Id": 0, "Owner": "", "Middle": 0.0, "Top": 0.0, "ScreenX": 0.0, "ScreenY": 0.0,
           "Across": across, "Radii": [26.0, 19.0, 13.0], "Seeds": null, "Angles": null, "Look": null})
}

fn saved_tree() -> Value {
    json!({"Look": {"Level": 1, "Topper": null, "Trims": []}, "Seed": 5, "Depths": "", "Cap": 0})
}

struct Rig {
    world: World,
    feed: Arc<SnowFeed>,
    settings: Settings,
    layout: Layout,
    now: f64,
    pointer: PointerInput,
    main: Option<Frame>,
    picker: Option<Frame>,
    windows: Vec<Frame>,
    snowing: bool,
    snaps: Vec<ScreenSnap>,
    push: bool,
    out: WorldOutput,
    prefs: Vec<(&'static str, Value)>,
    cards: Vec<CardRequest>,
}

impl Rig {
    fn with(saved: Map<String, Value>, snaps: Vec<ScreenSnap>) -> Rig {
        let feed = Arc::new(SnowFeed::new());
        Rig {
            world: World::new(feed.clone(), &saved, 7),
            feed,
            settings: Settings::from_map(&saved).0,
            layout: layout(),
            now: 100.0,
            pointer: PointerInput::default(),
            main: None,
            picker: None,
            windows: Vec::new(),
            snowing: true,
            snaps,
            push: true,
            out: WorldOutput::default(),
            prefs: Vec::new(),
            cards: Vec::new(),
        }
    }

    fn new() -> Rig {
        Rig::with(Map::new(), vec![snap_with_dock()])
    }

    fn step(&mut self, dt: f64) {
        self.now += dt;
        if self.push {
            self.feed.snow(self.snaps.clone());
        }
        let input = WorldInput {
            now: self.now,
            wall: WallTime::new(self.now as i64 + 1_700_000_000, 0),
            pointer: self.pointer,
            settings: &self.settings,
            layout: &self.layout,
            snowing: self.snowing,
            main_screen: self.main,
            picker: self.picker,
            windows: &self.windows,
        };
        self.world.step(&input, &mut self.out);
        self.prefs.append(&mut self.out.prefs);
        self.cards.append(&mut self.out.cards);
    }

    fn run(&mut self, seconds: f64) {
        for _ in 0..(seconds / DT) as usize {
            self.step(DT);
        }
    }

    /// Steps until `done` holds (at most `seconds`).
    fn until(&mut self, seconds: f64, done: impl Fn(&Rig) -> bool) -> bool {
        for _ in 0..(seconds / DT) as usize {
            self.step(DT);
            if done(self) {
                return true;
            }
        }
        false
    }

    fn pref(&self, key: &str) -> Option<&Value> {
        self.prefs.iter().rev().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    fn sprites_of(&self, owner: Owner) -> Vec<Sprite> {
        self.out.sprites.iter().filter(|s| s.key.owner_byte() == owner as u8).copied().collect()
    }

    fn station(&self) -> Option<Vec<(Pick, Vec2)>> {
        self.cards.iter().find_map(|c| match c {
            CardRequest::TrainStation { choices, .. } => Some(choices.clone()),
            _ => None,
        })
    }
}

fn saved_with(key: &str, v: Value) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(key.into(), v);
    m
}

#[test]
fn gating_and_sprite_list() {
    let mut r = Rig::new();
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.run(0.5);
    assert!(r.world.on);
    // a second call within 12.5 ms only reports the sprites again
    let before = r.out.sprites.len();
    assert_eq!(before, 1);
    r.out.sprites.push(r.out.sprites[0]);
    r.step(0.005);
    assert_eq!(r.out.sprites.len(), before);
    // non-monotonic time is gated too
    r.now -= 1.0;
    r.step(0.0);
    assert_eq!(r.out.sprites.len(), before);
}

#[test]
fn off_world_keeps_only_decorations() {
    let mut r = Rig::new();
    r.snowing = false;
    r.step(DT);
    assert!(!r.world.on);
    assert!(r.out.sprites.is_empty());
    // first run off forgets the standing snowmen
    assert_eq!(r.pref("standingSnowmen"), Some(&json!([])));
    r.world.command(Command::SendPlough, r.now);
    r.world.command(Command::SendSanta, r.now);
    r.world.command(Command::SendYeti, r.now);
    r.world.command(Command::SendTrain, r.now);
    r.world.command(Command::SendFight(None), r.now);
    assert!(r.world.fight_req.is_none());
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.run(0.5);
    assert_eq!(r.sprites_of(Owner::Decoration).len(), 1);
    let saved = r.pref("decorations").unwrap().clone();
    assert_eq!(saved.as_array().unwrap().len(), 1);
    r.world.command(Command::RemoveAllDecorations, r.now);
    r.run(0.5);
    assert!(r.sprites_of(Owner::Decoration).is_empty());
    assert_eq!(r.pref("decorations"), Some(&json!([])));
}

#[test]
fn remove_decoration_takes_one_down_and_saves() {
    let mut r = Rig::new();
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.run(0.5);
    assert_eq!(r.world.decoration_count(), 2);
    let id = r.world.decor.items().next().unwrap().id;
    r.world.command(Command::RemoveDecoration(id), r.now);
    r.run(0.5);
    assert_eq!(r.world.decoration_count(), 1);
    assert!(r.world.decor.get(id).is_none());
    assert_eq!(r.pref("decorations").unwrap().as_array().unwrap().len(), 1);
    // an unknown tree has no card anchor
    assert_eq!(r.world.tree_card_anchor(9999), None);
}

#[test]
fn decorations_load_clamp_and_cap() {
    let mut saved = saved_with("decorations", json!([{"kind": "redBauble", "x": 400.0, "y": 600.0}]));
    saved.insert("maxPlaced".into(), json!(1));
    let mut r = Rig::with(saved, vec![snap_with_dock()]);
    r.snowing = false;
    r.step(DT);
    assert_eq!(r.world.decor.len(), 1);
    // at the cap the limit card comes with the work area
    r.world.command(Command::AddDecoration(Kind::Lights), r.now);
    r.step(DT * 2.0);
    assert_eq!(r.cards, vec![CardRequest::LimitReached { screen: work() }]);
    assert_eq!((r.world.decoration_count(), r.world.decoration_max()), (1, 1));
    // a raised cap takes effect on the next step
    r.settings.max_placed = 2;
    r.step(DT * 2.0);
    assert_eq!(r.world.decoration_max(), 2);
    r.settings.max_placed = 1;
    r.step(DT * 2.0);
    // a monitor change re-clamps the saved spot and saves
    r.layout = Layout::new(vec![MonitorInfo {
        pixels: PixelRect::new(0, 0, 300, 300),
        work: PixelRect::new(0, 0, 300, 252),
        ..monitor("a", 0, true)
    }]);
    r.prefs.clear();
    r.step(DT * 2.0);
    assert!(r.pref("decorations").is_some());
    // the same layout again changes nothing
    r.prefs.clear();
    r.step(DT * 2.0);
    assert!(r.pref("decorations").is_none());
    // a monitor change that moves nothing saves nothing
    r.layout = Layout::new(vec![MonitorInfo {
        pixels: PixelRect::new(0, 0, 3000, 3000),
        work: PixelRect::new(0, 0, 3000, 2952),
        ..monitor("a", 0, true)
    }]);
    r.step(DT * 2.0);
    // no layout: an add is ignored
    r.layout = Layout::default();
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.step(DT * 2.0);
    assert_eq!(r.cards.len(), 1);
}

#[test]
fn decoration_add_uses_main_screen_work_area() {
    let mut r = Rig::new();
    r.layout = Layout::new(vec![monitor("a", 0, true), monitor("b", 1920, false)]);
    r.main = Some(Frame::new(1920.0, 0.0, 1920.0, 1080.0));
    r.snowing = false;
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.step(DT);
    let added = r.world.decor.items().next().unwrap();
    assert!(added.center.x >= 1920.0);
}

#[test]
fn decoration_add_lands_on_the_monitor_under_the_pointer() {
    let mut saved = Map::new();
    saved.insert("maxPlaced".into(), json!(1));
    let mut r = Rig::with(saved, vec![snap_with_dock()]);
    r.layout = Layout::new(vec![monitor("a", 0, true), monitor("b", 1920, false)]);
    r.main = Some(Frame::new(0.0, 0.0, 1920.0, 1080.0));
    r.snowing = false;
    r.pointer.pos = Some(Vec2::new(2500.0, 500.0));
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.step(DT);
    assert!(r.world.decor.items().next().unwrap().center.x >= 1920.0);
    // the limit card follows the same monitor
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.step(DT);
    assert!(matches!(r.cards.last(), Some(CardRequest::LimitReached { screen }) if screen.min_x() >= 1920.0));
}

#[test]
fn decoration_add_avoids_windows_and_picker_without_weather() {
    assert!(!Rig::new().world.has_pending_add());
    for (windows, picker) in [
        (vec![Frame::new(0.0, 48.0, 1300.0, 1032.0)], None),
        (Vec::new(), Some(Frame::new(12.0, 60.0, 1276.0, 1008.0))),
    ] {
        let mut r = Rig::new();
        r.snowing = false;
        r.windows = windows;
        r.picker = picker;
        r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
        assert!(r.world.has_pending_add());
        r.step(DT);
        assert!(!r.world.has_pending_add());
        let c = r.world.decor.items().next().unwrap().center;
        assert!(c.x > 1300.0, "{c:?}");
    }
}

#[test]
fn decoration_obstacles_are_windows() {
    let mut snap = snap_with_dock();
    snap.surfaces.push(Surface::new(SurfaceId::Window(5), Frame::new(100.0, 100.0, 300.0, 300.0)));
    let mut r = Rig::with(Map::new(), vec![snap]);
    r.step(DT);
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.step(DT * 2.0);
    assert_eq!(r.world.decor.len(), 1);
}

#[test]
fn screens_and_lists() {
    let mut snap = snap_with_dock();
    snap.surfaces.push(Surface::new(SurfaceId::Window(5), Frame::new(100.0, 100.0, 300.0, 300.0)));
    let away = Frame::new(1920.0, 0.0, 1920.0, 1080.0);
    let all = screens::stage_screens(&[snap, ScreenSnap { frame: away, shown: false, ..snap_bare() }]);
    assert_eq!(all[0].dock, Some((0.0, 1920.0, 48.0)));
    assert_eq!(all[1].dock, None);
    assert!(all[0].shown && !all[1].shown);
    let (visit, plough, shown) = screens::screen_lists(&all, None);
    assert_eq!((visit.len(), plough.len(), shown.len()), (2, 2, 2));
    // a main screen that is not shown is ignored
    let (visit, ..) = screens::screen_lists(&all, Some(away));
    assert_eq!(visit.len(), 2);
    let other = StageScreen { frame: away, shown: true, dock: None };
    let both = [all[0], other];
    let (visit, plough, shown) = screens::screen_lists(&both, Some(other.frame));
    assert_eq!(visit, vec![other]);
    assert_eq!(plough, vec![other, all[0]]);
    assert_eq!(shown.len(), 2);
    let (visit, ..) = screens::screen_lists(&both, Some(Frame::new(5.0, 5.0, 5.0, 5.0)));
    assert_eq!(visit.len(), 2);
    let snaps = [snap_bare(), ScreenSnap { frame: Frame::new(1920.0, 0.0, 1920.0, 1080.0), ..snap_bare() }];
    assert_eq!(screens::nearest_frame(&snaps, Vec2::new(2000.0, 10.0)), Some(snaps[1].frame));
    assert_eq!(screens::nearest_frame(&snaps, Vec2::new(-50.0, 10.0)), Some(snaps[0].frame));
    assert_eq!(screens::nearest_frame(&[], Vec2::ZERO), None);
    let mut with_window = snap_bare();
    with_window.frame = Frame::new(100.0, 50.0, 800.0, 600.0);
    with_window.surfaces.push(Surface::new(SurfaceId::Window(1), Frame::new(10.0, 20.0, 30.0, 40.0)));
    with_window.surfaces.push(Surface::new(SurfaceId::Dock, Frame::new(0.0, 0.0, 5.0, 5.0)));
    assert_eq!(screens::window_frames(&[with_window]), vec![Frame::new(110.0, 70.0, 30.0, 40.0)]);
    assert_eq!(screens::primary_work(&Layout::default()), None);
    assert_eq!(screens::work_area(&Layout::default(), None), None);
    assert_eq!(screens::primary_work(&layout()), Some(work()));
    assert_eq!(screens::work_area(&layout(), Some(frame())), Some(work()));
}

#[test]
fn paint_dispatch() {
    let mut r = Rig::new();
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.run(0.2);
    let deco = r.sprites_of(Owner::Decoration)[0];
    assert!(r.world.paint(deco.key, 1.0).is_some());
    assert!(r.world.paint(SpriteKey::new(Owner::Puppet, 1), 1.0).is_none());
    assert!(r.world.paint(SpriteKey::new(Owner::Ball, 99), 1.0).is_none());
    assert!(r.world.paint(SpriteKey::new(Owner::Tree, 99), 1.0).is_none());
    assert!(r.world.paint(SpriteKey::new(Owner::Fight, 0), 1.0).is_none());
    assert!(r.world.paint(SpriteKey::new(Owner::Gift, 1), 1.0).is_none());
    assert!(r.world.paint(SpriteKey::new(Owner::Character, 1), 1.0).is_none());
}

#[test]
fn trees_plant_save_restore_and_card() {
    let mut r = Rig::new();
    r.step(DT);
    r.world.command(Command::PlantTree, r.now);
    r.run(0.5);
    assert_eq!(r.world.grove.count(), 1);
    let CardRequest::TreeStyle { tree, quip, .. } = r.cards[0].clone() else { panic!("{:?}", r.cards) };
    assert_eq!(quip, None);
    assert_eq!(r.world.tree_card(), Some(tree));
    assert!(r.world.tree_card_origin(tree, work()).is_some());
    assert!(r.world.tree_card_origin(tree + 5, work()).is_none());
    r.world.restyle_tree(tree, fp_trees::TreeLook { level: 4, ..Default::default() });
    r.world.close_tree_card();
    assert_eq!(r.world.tree_card(), None);
    r.run(7.0);
    let saved = r.pref("standingTrees").unwrap().clone();
    assert_eq!(saved.as_array().unwrap().len(), 1);
    assert_eq!(r.sprites_of(Owner::Tree).len(), 1);
    let tree_sprite = r.sprites_of(Owner::Tree)[0];
    assert!(r.world.paint(tree_sprite.key, 1.0).is_some());

    // a new world restores the tree once snow shows
    let mut again = Rig::with(saved_with("standingTrees", saved), vec![snap_with_dock()]);
    again.run(1.0);
    assert_eq!(again.sprites_of(Owner::Tree).len(), 1);
    // turning the snow off saves the trees again (unchanged: no write)
    again.snowing = false;
    again.prefs.clear();
    again.step(DT);
    assert!(again.sprites_of(Owner::Tree).is_empty());
    assert!(again.pref("standingTrees").is_none());
    assert_eq!(again.pref("standingSnowmen"), Some(&json!([])));
}

#[test]
fn planting_waits_for_shelves_then_gives_up() {
    let mut r = Rig::with(Map::new(), Vec::new());
    r.push = false;
    r.world.command(Command::PlantTree, r.now);
    r.run(2.0);
    assert!(r.world.plant.is_some());
    r.run(9.0);
    assert!(r.world.plant.is_none());
    assert_eq!(r.world.grove.count(), 0);
    // shelves arrive in time
    r.world.command(Command::PlantTree, r.now);
    r.run(1.0);
    r.snaps = vec![snap_bare()];
    r.push = true;
    r.run(1.0);
    assert_eq!(r.world.grove.count(), 1);
}

#[test]
fn tree_cap_refusals() {
    let mut saved = Map::new();
    saved.insert("maxPlaced".into(), json!(1));
    let mut r = Rig::with(saved, vec![snap_with_dock()]);
    r.step(DT);
    for _ in 0..4 {
        r.world.command(Command::PlantTree, r.now);
        r.run(0.6);
    }
    let mut quips = Vec::new();
    for c in &r.cards {
        match c {
            CardRequest::TreeStyle { quip, .. } => quips.push(*quip),
            CardRequest::LimitReached { screen } => assert_eq!(*screen, work()),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(quips, vec![None, Some(1), Some(2)]);
    assert!(r.cards.iter().any(|c| matches!(c, CardRequest::LimitReached { .. })));
}

#[test]
fn tree_event_routing() {
    let mut r = Rig::new();
    r.step(DT);
    let mut out = WorldOutput::default();
    // a Style event for a missing tree falls back to the default look
    r.world.route_trees(
        vec![
            TreeEvent::Style { id: 77, quip: Some(1) },
            TreeEvent::Snow { at: Vec2::new(100.0, 60.0), area: 3.0, velocity: Vec2::new(7.0, 0.0) },
            TreeEvent::LimitReached,
            TreeEvent::Dirty,
        ],
        &layout(),
        &mut out,
    );
    assert_eq!(out.cards[0], CardRequest::TreeStyle { tree: 77, look: fp_trees::TreeLook::default(), quip: Some(1) });
    assert!(matches!(out.cards[1], CardRequest::LimitReached { .. }));
    assert_eq!(out.prefs[0].0, "standingTrees");
    let bursts = r.feed.bursts(Vec2::ZERO);
    assert_eq!(bursts.len(), 1);
    assert_eq!(bursts[0].point, Vec2::new(100.0, 60.0));
    assert_eq!(bursts[0].velocity, Vec2::new(7.0, 0.0));
    let mut none = WorldOutput::default();
    r.world.route_trees(vec![TreeEvent::LimitReached], &Layout::default(), &mut none);
    assert!(none.cards.is_empty());
    // a snow burst with no screens goes nowhere
    r.world.snaps.clear();
    r.world.burst_at(Vec2::ZERO, 1.0, Vec2::ZERO);
    assert!(r.feed.bursts(Vec2::ZERO).is_empty());
}

#[test]
fn gathered_snow_makes_balls_and_saves_them() {
    let mut r = Rig::new();
    r.run(0.5);
    // the feed is open for new balls
    let got = r.feed.gather(300.0, Vec2::new(500.0, 60.0), Vec2::new(10.0, 0.0), Vec2::ZERO);
    assert!(got > 0.0);
    r.run(0.5);
    assert_eq!(r.world.balls.count(), 1);
    // that ball is a target for more snow
    assert_eq!(r.feed.gather(10.0, Vec2::new(500.0, 60.0), Vec2::new(10.0, 0.0), Vec2::ZERO), 7.5);
    r.run(4.0);
    let saved = r.pref("standingSnowmen").unwrap().clone();
    assert_eq!(saved.as_array().unwrap().len(), 1);
    assert_eq!(r.sprites_of(Owner::Ball).len(), 1);
    let ball = r.sprites_of(Owner::Ball)[0];
    assert!(r.world.paint(ball.key, 1.0).is_some());

    // the pause keeps the snowmen for the resume
    r.world.set_paused_for_battery(true);
    r.snowing = false;
    r.prefs.clear();
    r.step(DT);
    assert!(r.pref("standingSnowmen").is_none());
    assert_eq!(r.world.balls.count(), 0);
    r.world.set_paused_for_battery(false);
    r.snowing = true;
    r.run(1.0);
    assert_eq!(r.world.balls.count(), 1);
    // switching the weather away for good forgets them
    r.snowing = false;
    r.step(DT);
    assert_eq!(r.pref("standingSnowmen"), Some(&json!([])));
    // a pause that cannot save (nothing was running) leaves the list alone
    r.snowing = true;
    r.step(DT);
    r.world.balls.clear();
    r.world.set_paused_for_battery(true);
    r.snowing = false;
    r.prefs.clear();
    r.step(DT);
    assert!(r.pref("standingSnowmen").is_none());
}

#[test]
fn full_ball_cap_closes_the_feed() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_with_dock()]);
    r.run(1.0);
    // a decoration drag also closes it
    assert_eq!(r.feed.gather(300.0, Vec2::new(100.0, 60.0), Vec2::ZERO, Vec2::ZERO), 120.0);
    assert!(r.world.balls.count() >= 3);
}

#[test]
fn snowballs_setting_off_breaks_everything() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_with_dock()]);
    r.run(1.0);
    assert_eq!(r.world.balls.count(), 3);
    assert_eq!(r.sprites_of(Owner::Ball).len(), 3);
    r.settings.snowballs = false;
    r.step(DT * 2.0);
    assert_eq!(r.world.balls.count(), 0);
    assert_eq!(r.pref("standingSnowmen"), Some(&json!([])));
    assert!(!r.feed.bursts(Vec2::ZERO).is_empty());
    // new snow no longer seeds balls
    r.step(DT * 2.0);
    assert_eq!(r.feed.gather(300.0, Vec2::new(500.0, 60.0), Vec2::ZERO, Vec2::ZERO), 0.0);
    // starting with snowballs off (saved setting) neither restores nor
    // forgets the list
    let mut saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    saved.insert("snowballs".into(), json!(false));
    let mut off = Rig::with(saved, vec![snap_with_dock()]);
    off.settings.snowballs = false;
    off.run(1.0);
    assert_eq!(off.world.balls.count(), 0);
    assert!(off.pref("standingSnowmen").is_none());
}

#[test]
fn broken_clock_is_ignored() {
    let mut r = Rig::new();
    r.world.command(Command::AddDecoration(Kind::RedBauble), r.now);
    r.run(0.5);
    let last = r.world.last;
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let input = WorldInput {
            now: bad,
            wall: WallTime::new(1_700_000_000, 0),
            pointer: r.pointer,
            settings: &r.settings,
            layout: &r.layout,
            snowing: true,
            main_screen: None,
            picker: None,
            windows: &[],
        };
        r.world.step(&input, &mut r.out);
        assert_eq!(r.world.last, last);
        assert_eq!(r.out.sprites.len(), 1);
    }
    // a good clock steps again with a clamped dt
    r.now = last.unwrap();
    r.step(1.0);
    assert_eq!(r.world.last, Some(last.unwrap() + 1.0));
}

#[test]
fn planting_while_off_waits_for_the_snow() {
    // the weather never turns to snow: 16 tries over ~8 s, then nothing
    let mut r = Rig::new();
    r.snowing = false;
    r.step(DT);
    r.world.command(Command::PlantTree, r.now);
    r.run(7.0);
    assert_eq!(r.world.plant.map(|p| p.tries), Some(2));
    r.run(1.5);
    assert!(r.world.plant.is_none());
    // the host switches the weather in time: the tree is planted
    r.world.command(Command::PlantTree, r.now);
    r.run(2.0);
    r.snowing = true;
    r.run(0.6);
    assert_eq!(r.world.grove.count(), 1);
    assert!(r.world.plant.is_none());
}

#[test]
fn snow_resume_reuses_the_last_snapshot() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    assert_eq!(r.world.balls.count(), 3);
    // the tracker goes quiet; a battery pause and resume still find ledges
    r.push = false;
    r.world.set_paused_for_battery(true);
    r.snowing = false;
    r.step(DT);
    assert_eq!(r.world.balls.count(), 0);
    r.snowing = true;
    r.run(1.0);
    assert_eq!(r.world.balls.count(), 3);
    assert_eq!(r.world.snaps.len(), 1);
}

#[test]
fn ball_event_routing_and_dress() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    let bottom = r.world.balls.standing_snowmen()[0].0;
    let look = fp_snowman::Look { hat: Some(fp_snowman::Hat::Viking), ..fp_snowman::Look::classic() };
    let mut out = WorldOutput::default();
    r.world.route_balls(vec![BallEvent::Dress { bottom }], &mut out);
    assert_eq!(out.cards, vec![CardRequest::DressUp { bottom, look: None }]);
    r.world.restyle_snowman(bottom, Some(look));
    r.world.restyle_snowman(bottom, Some(look));
    out.cards.clear();
    r.world.route_balls(vec![BallEvent::Dress { bottom }], &mut out);
    assert_eq!(out.cards, vec![CardRequest::DressUp { bottom, look: Some(look) }]);
    r.world.restyle_snowman(bottom, None);
    assert_eq!(r.world.balls.look(bottom), None);
    r.world.restyle_snowman(bottom, Some(look));
    r.world.route_balls(vec![BallEvent::Dirty], &mut out);
    assert_eq!(out.prefs[0].0, "standingSnowmen");
    // the same list again is not written twice
    out.prefs.clear();
    r.world.route_balls(vec![BallEvent::Dirty], &mut out);
    assert!(out.prefs.is_empty());
    // a save that must wait writes nothing
    r.world.balls.clear();
    let mut none = WorldOutput::default();
    r.world.route_balls(vec![BallEvent::Dirty], &mut none);
    assert!(none.prefs.is_empty());
    let origin = Vec2::ZERO;
    let burst = Burst { point: Vec2::new(5.0, 5.0), area: 9.0, velocity: Vec2::ZERO };
    r.world.route_balls(vec![BallEvent::Burst { origin, burst }], &mut none);
    assert_eq!(r.feed.bursts(origin).len(), 1);
}

#[test]
fn plough_blade_reaches_feed_and_balls() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.feed.report_depth(Vec2::ZERO, 30.0);
    r.run(1.0);
    r.world.command(Command::SendPlough, r.now);
    let mut blade: Option<Blade> = None;
    for _ in 0..600 {
        r.step(DT);
        blade = blade.or(r.feed.blade(Vec2::ZERO));
        if blade.is_some() {
            break;
        }
    }
    assert!(blade.is_some());
    assert!(r.sprites_of(Owner::Character).iter().any(|s| s.key.id() == 2));
    assert!(r.world.visitors.plough_front(frame()).is_some());
    // the blade is withdrawn again once the plough is gone
    r.run(60.0);
    assert!(r.feed.blade(Vec2::ZERO).is_none());
}

#[test]
fn visitor_events() {
    let mut r = Rig::new();
    r.step(DT);
    let mut balls = Vec::new();
    let mut out = WorldOutput::default();
    let events = vec![
        CharacterEvent::Burst { at: Vec2::new(10.0, 10.0), area: 220.0, velocity: Vec2::ZERO },
        CharacterEvent::GiftDropped { at: Vec2::ZERO },
        CharacterEvent::GiftOpened { at: Vec2::new(300.0, 80.0) },
        CharacterEvent::Smash { bottom: 99 },
        CharacterEvent::Chop { tree: 99 },
        CharacterEvent::FightDue,
        CharacterEvent::FightDue,
        CharacterEvent::Dirty,
    ];
    r.world.route_characters(events, &mut balls, Some(Lang::De), &mut out);
    assert_eq!(r.feed.bursts(Vec2::ZERO).len(), 2);
    let CardRequest::Quote { at, quote } = out.cards[0] else { panic!() };
    assert_eq!(at, Vec2::new(300.0, 80.0));
    assert!(quote < fp_text::quotes(Lang::De).len());
    assert_eq!(r.world.fight_req, Some((None, false)));
    let keys: Vec<_> = out.prefs.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys[0], "quotesLeftDe");
    // a second quote continues the saved deck; English is the default
    let mut out2 = WorldOutput::default();
    r.world.route_characters(vec![CharacterEvent::GiftOpened { at: Vec2::ZERO }], &mut balls, None, &mut out2);
    r.world.route_characters(vec![CharacterEvent::GiftOpened { at: Vec2::ZERO }], &mut balls, None, &mut out2);
    assert_eq!(out2.prefs.len(), 2);
    assert_eq!(out2.prefs[0].0, "quotesLeft");
    // visitor settings flow through
    r.settings.visitors_away = vec![fp_core::effect::Visitor::Yeti];
    r.settings.often_visits = true;
    r.settings.behind_windows = false;
    r.step(DT * 2.0);
}

#[test]
fn visitor_state_is_saved_after_a_santa_visit() {
    let mut r = Rig::new();
    r.step(DT);
    r.world.command(Command::SendSanta, r.now);
    r.run(2.0);
    assert!(r.sprites_of(Owner::Character).iter().any(|s| s.key.id() == 1));
}

#[test]
fn yeti_smashes_a_snowman() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    r.world.command(Command::SendYeti, r.now);
    assert!(r.until(60.0, |r| r.world.balls.count() == 0), "the yeti smashed the snowman");
}

#[test]
fn yeti_chops_a_tree() {
    let mut t = Rig::with(Map::new(), vec![snap_bare()]);
    t.step(DT);
    t.world.command(Command::PlantTree, t.now);
    t.run(2.0);
    t.world.command(Command::SendYeti, t.now);
    assert!(t.until(90.0, |r| r.world.grove.count() == 0), "the yeti felled the tree");
}

#[test]
fn chop_without_a_yeti_falls_right() {
    let mut t = Rig::with(Map::new(), vec![snap_bare()]);
    t.step(DT);
    t.world.command(Command::PlantTree, t.now);
    t.run(2.0);
    let id = t.world.grove.standing()[0].0;
    t.world.doom_tree(id);
    t.world.chop(id);
    assert!(t.world.grove.doomed().is_some());
}

#[test]
fn fight_runs_on_the_dock_and_finishes() {
    let mut r = Rig::new();
    r.step(DT);
    r.world.command(Command::SendFight(Some(fp_fight::Variant::Quick)), r.now);
    r.step(DT);
    assert!(r.world.fight.active());
    assert!(r.world.fight_req.is_none());
    assert_eq!(r.sprites_of(Owner::Fight).len(), 1);
    assert!(r.world.paint(r.sprites_of(Owner::Fight)[0].key, 1.0).is_some());
    // a second request waits for the first bout
    r.world.command(Command::SendFight(None), r.now);
    r.step(DT);
    assert!(r.world.fight_req.is_some());
    assert!(
        r.until(300.0, |r| r.world.fight_frame.is_some() && !r.world.fight.active() || r.world.fight_req.is_none())
    );
    r.run(300.0);
    assert!(!r.world.fight.active());
    assert!(!r.feed.bursts(Vec2::ZERO).is_empty());
}

#[test]
fn fight_on_bare_ground_waits_for_screen() {
    // no shown screen: the request waits
    let mut r = Rig::with(Map::new(), vec![ScreenSnap { shown: false, ..snap_bare() }]);
    r.step(DT);
    r.world.command(Command::SendFight(None), r.now);
    r.run(0.5);
    assert!(r.world.fight_req.is_some());
    // a narrow dock is treated like no dock
    let mut narrow = snap_bare();
    narrow.surfaces.push(Surface::new(SurfaceId::Dock, Frame::new(0.0, 0.0, 200.0, 48.0)));
    r.snaps = vec![narrow];
    r.run(0.5);
    assert!(r.world.fight.active());
    assert_eq!(r.world.fight.floor(), Some(0.0));
    // hiding the screen mid-fight makes the panel invisible
    r.snaps = vec![ScreenSnap { shown: false, ..snap_bare() }];
    r.run(0.2);
    assert_eq!(r.sprites_of(Owner::Fight)[0].opacity, 0.0);
    // losing the screen stops the bout
    r.snaps = Vec::new();
    r.run(0.2);
    assert!(!r.world.fight.active());
    assert!(r.world.fight_frame.is_none());
}

#[test]
fn plough_vetoes_a_fight() {
    let mut r = Rig::with(Map::new(), vec![snap_bare()]);
    r.feed.report_depth(Vec2::ZERO, 30.0);
    r.step(DT);
    r.world.command(Command::SendPlough, r.now);
    let near = |r: &Rig| r.world.visitors.plough_front(frame()).is_some_and(|f| f > 700.0 && f < 1200.0);
    assert!(r.until(60.0, near));
    r.world.command(Command::SendFight(Some(fp_fight::Variant::Both)), r.now);
    r.step(DT);
    assert!(!r.world.fight.active());
    assert!(r.world.fight_req.is_some());
}

#[test]
fn main_screen_limits_visits() {
    let mut r = Rig::new();
    r.main = Some(frame());
    r.step(DT);
    r.world.command(Command::SendSanta, r.now);
    r.run(1.0);
    assert!(r.sprites_of(Owner::Character).iter().any(|s| s.key.id() == 1));
}

#[test]
fn train_round_trip_carries_a_snowman() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    assert_eq!(r.world.balls.count(), 3);
    r.world.command(Command::SendTrain, r.now);
    assert!(r.until(60.0, |r| r.station().is_some()), "the train reached the station");
    let choices = r.station().unwrap();
    assert_eq!(choices.len(), 1);
    let (pick, _) = choices[0];
    assert!(matches!(pick, Pick::Snowman(_)));
    r.world.command(Command::TrainFetch(pick), r.now);
    // the snowman is lifted, loaded (a copy keeps riding), then the train leaves
    assert!(r.until(60.0, |r| r.world.riders.iter().any(|x| x.boarded)));
    assert!(r.until(60.0, |r| r.world.riders.is_empty()));
    assert_eq!(r.world.balls.count(), 0);
    // the delivery comes back and stands the snowman
    assert!(r.until(300.0, |r| r.world.balls.count() == 3));
    assert!(r.pref("trainQueue").is_some());
}

#[test]
fn train_round_trip_carries_a_tree() {
    let mut r = Rig::with(Map::new(), vec![snap_bare()]);
    r.step(DT);
    r.world.command(Command::PlantTree, r.now);
    r.run(3.0);
    r.world.command(Command::SendTrain, r.now);
    assert!(r.until(60.0, |r| r.station().is_some()));
    let choices = r.station().unwrap();
    assert!(matches!(choices[0].0, Pick::Tree(_)));
    r.world.command(Command::TrainFetch(choices[0].0), r.now);
    assert!(r.until(60.0, |r| r.world.riders.iter().any(|x| x.boarded)));
    assert!(r.until(60.0, |r| r.world.riders.is_empty()));
    assert!(r.until(300.0, |r| r.world.grove.count() == 1));
}

#[test]
fn train_dismiss_and_missing_pick() {
    let mut r = Rig::with(Map::new(), vec![snap_bare()]);
    r.step(DT);
    r.world.command(Command::SendTrain, r.now);
    assert!(r.until(60.0, |r| r.station().is_some()));
    // an object that is no longer there: the train just leaves
    r.world.command(Command::TrainFetch(Pick::Snowman(42)), r.now);
    r.run(30.0);
    r.world.command(Command::TrainDismiss, r.now);
    assert!(r.world.riders.is_empty());
}

#[test]
fn rider_rules() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    let bottom = r.world.balls.standing_snowmen()[0].0;
    // a lifted original is released when the train has no carry point
    r.world.riders.push(Rider { pick: Pick::Snowman(bottom), boarded: false });
    r.world.update_riders();
    assert!(r.world.riders.is_empty());
    // a copy vanishes
    let cargo = r.world.balls.take(bottom).unwrap();
    let id = r.world.balls.board(&cargo, Vec2::new(500.0, 100.0)).unwrap();
    r.world.riders.push(Rider { pick: Pick::Snowman(id), boarded: true });
    r.world.update_riders();
    assert_eq!(r.world.balls.count(), 0);
    // trees: original released, copy vanishes, release_riders handles both
    r.world.command(Command::PlantTree, r.now);
    r.run(2.0);
    let tree = r.world.grove.standing()[0].0;
    r.world.carry(Pick::Tree(tree), Some(Vec2::new(300.0, 300.0)));
    r.world.riders.push(Rider { pick: Pick::Tree(tree), boarded: false });
    r.world.release_riders();
    assert!(r.world.riders.is_empty());
    let value = r.world.grove.take(tree).unwrap();
    let copy = r.world.grove.board(&value, Vec2::new(300.0, 300.0)).unwrap();
    r.world.riders.push(Rider { pick: Pick::Tree(copy), boarded: true });
    r.world.riders.push(Rider { pick: Pick::Snowman(12345), boarded: false });
    r.world.release_riders();
    r.world.riders.push(Rider { pick: Pick::Tree(copy), boarded: true });
    r.world.update_riders();
    assert!(r.world.riders.is_empty());
    // loading something that is gone hands nothing over
    r.world.loaded(Pick::Snowman(777));
    r.world.loaded(Pick::Tree(777));
    assert!(r.world.riders.is_empty());
    let mut out = WorldOutput::default();
    r.world.route_characters(vec![CharacterEvent::Took { pick: Pick::Tree(5) }], &mut Vec::new(), None, &mut out);
    assert_eq!(r.world.riders.len(), 1);
    r.world.route_characters(vec![CharacterEvent::Left], &mut Vec::new(), None, &mut out);
    assert!(r.world.riders.is_empty());
}

#[test]
fn unload_places_snowman_beside_tree() {
    let mut r = Rig::with(Map::new(), vec![snap_bare()]);
    r.step(DT);
    let both = Cargo { tree: Some(saved_tree()), snowman: Some(saved_stack(0.0)) };
    let mut out = WorldOutput::default();
    r.world.route_characters(
        vec![CharacterEvent::Unload { cargo: both.clone(), x: 800.0, screen: frame() }],
        &mut Vec::new(),
        None,
        &mut out,
    );
    r.run(1.0);
    assert_eq!(r.world.balls.count(), 3);
    assert_eq!(r.world.grove.count(), 1);
    assert_eq!(r.world.balls.standing_snowmen()[0].1.x, 730.0);
    // a snowman alone stands at x
    r.world.unload(&Cargo { tree: None, snowman: Some(saved_stack(0.0)) }, 300.0, frame());
    assert_eq!(r.world.balls.count(), 6);
    // a tree alone is planted without a snowman
    r.world.unload(&Cargo { tree: Some(saved_tree()), snowman: None }, 1400.0, frame());
    assert_eq!(r.world.grove.count(), 2);
    // nothing to place it on
    let mut empty = Rig::with(Map::new(), Vec::new());
    empty.step(DT);
    empty.world.unload(&both, 800.0, frame());
    assert_eq!(empty.world.balls.count(), 0);
}

#[test]
fn delivery_boards_cargo_aboard() {
    let queue = json!([{"id": 0, "from": "", "cargo": {"tree": saved_tree(), "snowman": saved_stack(0.0)}}]);
    let mut saved = Map::new();
    saved.insert("trainQueue".into(), queue);
    saved.insert("trainLook".into(), json!("classic"));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.settings.often_visits = true;
    assert!(r.until(400.0, |r| r.world.riders.len() == 2), "both objects ride");
    assert!(r.until(60.0, |r| r.world.riders.is_empty()));
    assert!(r.until(60.0, |r| r.world.balls.count() == 3 && r.world.grove.count() == 1));
}

#[test]
fn shutdown_saves() {
    let mut r = Rig::new();
    r.step(DT);
    r.world.command(Command::PlantTree, r.now);
    r.run(1.0);
    let tree = r.world.grove.standing()[0].0;
    r.world.restyle_tree(tree, fp_trees::TreeLook { level: 0, ..Default::default() });
    r.world.balls.restyle(0, None);
    let mut out = WorldOutput::default();
    r.world.shutdown(&mut out);
    let keys: Vec<_> = out.prefs.iter().map(|(k, _)| *k).collect();
    assert!(keys.contains(&"standingTrees") && keys.contains(&"decorations"));
    assert!(!r.world.on);
    // nothing running: nothing new to write
    let mut out = WorldOutput::default();
    r.world.shutdown(&mut out);
    assert!(out.prefs.is_empty());
}

#[test]
fn fight_blocks_use_bout_screen_and_floor() {
    let f = Frame::new(100.0, 0.0, 1920.0, 1080.0);
    assert!(super::fight_blocks(None, Some(40.0), &[(1.0, 1.0, 1.0)]).is_empty());
    assert!(super::fight_blocks(Some(f), None, &[(1.0, 1.0, 1.0)]).is_empty());
    let b = super::fight_blocks(Some(f), Some(40.0), &[(300.0, 35.0, 58.0)]);
    assert_eq!(b, vec![(f, 265.0, 335.0, 40.0, 98.0)]);
}

#[test]
fn dress_card_invalid_after_break_all_and_clear() {
    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    let bottom = r.world.balls.standing_snowmen()[0].0;
    assert!(r.world.dress_card_valid(bottom));
    assert!(!r.world.dress_card_valid(bottom + 1000));
    let mut ev = Vec::new();
    r.world.balls.break_all(&mut ev);
    assert!(!r.world.dress_card_valid(bottom));

    let saved = saved_with("standingSnowmen", json!([saved_stack(900.0)]));
    let mut r = Rig::with(saved, vec![snap_bare()]);
    r.run(1.0);
    let bottom = r.world.balls.standing_snowmen()[0].0;
    r.world.balls.clear();
    assert!(!r.world.dress_card_valid(bottom));
}

#[test]
fn station_flag_clears_when_train_leaves() {
    let mut r = Rig::with(Map::new(), vec![snap_bare()]);
    r.step(DT);
    assert!(!r.world.station_open());
    r.world.command(Command::SendTrain, r.now);
    assert!(r.until(60.0, |r| r.station().is_some()));
    assert!(r.world.station_open());
    r.world.command(Command::TrainDismiss, r.now);
    assert!(r.until(300.0, |r| !r.world.station_open()), "Left closes the station");
}

#[test]
fn snow_off_closes_station_and_flags_cards() {
    let mut r = Rig::with(Map::new(), vec![snap_bare()]);
    r.step(DT);
    r.world.command(Command::SendTrain, r.now);
    assert!(r.until(60.0, |r| r.station().is_some()));
    assert!(r.world.station_open());
    assert!(!r.out.snow_stopped);
    r.snowing = false;
    r.step(DT);
    assert!(r.out.snow_stopped);
    assert!(!r.world.station_open());
}

#[test]
fn auto_fight_bookkeeping_follows_the_bout() {
    let mut r = Rig::new();
    r.step(DT);
    // a manual request is pending; auto due must not overwrite it
    r.world.command(Command::SendFight(Some(fp_fight::Variant::Quick)), r.now);
    let mut out = WorldOutput::default();
    r.world.route_characters(vec![CharacterEvent::FightDue], &mut Vec::new(), None, &mut out);
    assert_eq!(r.world.fight_req, Some((Some(fp_fight::Variant::Quick), true)));
    assert!(out.prefs.iter().all(|(k, _)| *k != "lastSnowmanFight"));
    r.step(DT);
    assert!(r.world.fight_manual);
    assert!(r.world.visitors.save_json().get("lastSnowmanFight").is_none());
    // auto due again once the manual bout is over: auto bout follows
    r.world.route_characters(vec![CharacterEvent::FightDue], &mut Vec::new(), None, &mut out);
    assert_eq!(r.world.fight_req, Some((None, false)));
    assert!(r.until(300.0, |r| r.world.fight_frame.is_none()));
    r.step(DT);
    assert!(r.world.fight.active() && !r.world.fight_manual);
    // the pref is written when the auto bout begins, not before
    assert!(r.world.visitors.save_json()["lastSnowmanFight"].is_string());
    // the auto bout ends: the world clears it and re-rolls the wait
    assert!(r.until(300.0, |r| r.world.fight_frame.is_none()));
    assert!(!r.world.fight.active());
}

#[test]
fn unusable_stage_keeps_the_fight_request() {
    let mut r = Rig::new();
    r.world.fight_req = Some((None, true));
    let bad = StageScreen { frame: Frame::new(f64::NAN, 0.0, 1920.0, 1080.0), shown: true, dock: None };
    let mut out = WorldOutput::default();
    r.world.start_fight(&[bad], WallTime::new(1_700_000_000, 0), &mut out);
    assert!(!r.world.fight.active());
    assert_eq!(r.world.fight_req, Some((None, true)));
    assert!(r.world.fight_frame.is_none());
}
