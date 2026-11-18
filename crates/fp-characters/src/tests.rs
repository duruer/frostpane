//! Behaviour tests driven through the public `Visitors` API.

use super::*;
use fp_core::scene::HitMode;
use serde_json::json;

fn scr(x: f64, dock: bool, shown: bool) -> StageScreen {
    StageScreen { frame: Frame::new(x, 0.0, 1920.0, 1080.0), shown, dock: dock.then_some((x, x + 1920.0, 40.0)) }
}

struct Rig {
    v: Visitors,
    now: f64,
    wall: WallTime,
    screens: Vec<StageScreen>,
    ptr: PointerInput,
}

impl Rig {
    fn new(seed: u64) -> Self {
        let mut v = Visitors::new(seed);
        v.start(0.0);
        Self {
            v,
            now: 0.0,
            wall: WallTime::new(20_000 * 86_400 + 12 * 3600, 0),
            screens: vec![scr(0.0, true, true)],
            ptr: PointerInput::default(),
        }
    }

    fn tick(
        &mut self,
        dt: f64,
        prey: &[(u64, Vec<(Vec2, f64)>)],
        doomed: Option<(u64, Vec2, f64)>,
        obst: &[crate::Block],
    ) -> Vec<CharacterEvent> {
        self.now += dt;
        let ctx = StepCtx {
            now: self.now,
            dt,
            wall: self.wall,
            visit_screens: &self.screens,
            plough_screens: &self.screens,
            shown_screens: &self.screens,
            pointer: &self.ptr,
        };
        let mut out = Vec::new();
        self.v.step(&ctx, prey, doomed, obst, &mut out);
        out
    }

    fn step(&mut self) -> Vec<CharacterEvent> {
        self.tick(1.0 / 60.0, &[], None, &[])
    }

    fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::new();
        self.v.sprites(&mut out);
        out
    }

    fn sprite(&self, owner: Owner, id: u64) -> Option<Sprite> {
        self.sprites().into_iter().find(|s| s.key == SpriteKey::new(owner, id))
    }

    /// Steps until `done` says so (or `limit` seconds pass); returns all events.
    fn until(&mut self, limit: f64, mut done: impl FnMut(&mut Rig, &[CharacterEvent]) -> bool) -> Vec<CharacterEvent> {
        let end = self.now + limit;
        let mut all = Vec::new();
        while self.now < end {
            let ev = self.step();
            let stop = done(self, &ev);
            all.extend(ev);
            if stop {
                return all;
            }
        }
        panic!("condition not reached in {limit} s");
    }

    fn run(&mut self, secs: f64) -> Vec<CharacterEvent> {
        let end = self.now + secs;
        let mut all = Vec::new();
        while self.now < end {
            all.extend(self.step());
        }
        all
    }

    /// Every sprite paints at its own size.
    fn paint_all(&self, scale: f32) {
        for s in self.sprites() {
            let pm = self.v.paint(s.key, scale).unwrap_or_else(|| panic!("no art for {:?}", s.key));
            assert_eq!(pm.width(), (s.frame.w * f64::from(scale)).ceil() as u32, "{:?}", s.key);
            assert_eq!(pm.height(), (s.frame.h * f64::from(scale)).ceil() as u32, "{:?}", s.key);
        }
    }
}

fn has(ev: &[CharacterEvent], f: impl Fn(&CharacterEvent) -> bool) -> bool {
    ev.iter().any(f)
}

#[test]
fn defaults_and_stopped_world() {
    let mut v = Visitors::default();
    let mut out = Vec::new();
    let ctx = StepCtx {
        now: 1.0,
        dt: 0.1,
        wall: WallTime::default(),
        visit_screens: &[],
        plough_screens: &[],
        shown_screens: &[],
        pointer: &PointerInput::default(),
    };
    v.step(&ctx, &[], None, &[], &mut out);
    assert!(out.is_empty());
    let mut sprites = Vec::new();
    v.sprites(&mut sprites);
    assert!(sprites.is_empty());
    assert!(v.paint(SpriteKey::new(Owner::Character, 1), 1.0).is_none());
    assert!(v.paint(SpriteKey::new(Owner::Gift, 1), 1.0).is_none());
    assert!(v.paint(SpriteKey::new(Owner::Ball, 1), 1.0).is_none());
    assert!(v.blade().is_none());
    assert!(v.carry_point().is_none());
    assert!(v.cargo_aboard().is_none());
    assert!(v.plough_front(Frame::default()).is_none());
}

#[test]
fn santa_manual_dock_run_hops_and_gives_no_gift() {
    let mut r = Rig::new(3);
    r.v.send_santa(0.0);
    assert!(!r.v.santa.active() && !r.v.plough.active());
    let mut seen = false;
    let mut max_bottom = 0.0f64;
    let mut ev_all = Vec::new();
    for i in 0..4000 {
        let obst = [(Frame::new(0.0, 0.0, 1920.0, 1080.0), 920.0, 1000.0, 39.0, 97.0)];
        let ev = r.tick(1.0 / 60.0, &[], None, &obst);
        ev_all.extend(ev);
        match r.sprite(Owner::Character, 1) {
            Some(s) => {
                seen = true;
                assert!(r.v.santa.active());
                let scale = s.frame.w / 266.0;
                assert!((scale - 0.8).abs() < 1e-9, "dock run uses dock scale");
                max_bottom = max_bottom.max(s.frame.y + 36.0 * scale);
                if i % 300 == 0 {
                    r.paint_all(1.0);
                }
            }
            None if seen => break,
            None => {}
        }
    }
    assert!(seen);
    assert!(max_bottom > 39.0 + 50.0, "sleigh hopped the obstacle: {max_bottom}");
    assert!(!has(&ev_all, |e| matches!(e, CharacterEvent::GiftDropped { .. })));
}

#[test]
fn santa_on_bare_ground_hops_plough_and_tilts() {
    let mut hopped = false;
    for seed in 0..4 {
        let mut r = Rig::new(seed);
        r.screens = vec![scr(0.0, false, true)];
        r.v.send_santa(0.0);
        r.v.send_plough(0.0);
        let mut seen = false;
        for _ in 0..4000 {
            r.step();
            match r.sprite(Owner::Character, 1) {
                Some(s) => {
                    seen = true;
                    assert!((s.frame.w - 266.0).abs() < 1e-9);
                    // on the flat touch segment the runners only leave the
                    // ground to clear the plough (66 high)
                    let center = s.frame.x + 133.0;
                    if (576.0..=1344.0).contains(&center) && s.frame.y + 36.0 > 66.0 {
                        hopped = true;
                    }
                }
                None if seen => break,
                None => {}
            }
        }
        assert!(seen);
    }
    assert!(hopped, "the sleigh lifted over the plough");
}

#[test]
fn santa_automatic_visit_drops_one_gift_that_opens() {
    let mut r = Rig::new(9);
    r.now = 11_000.0;
    let mut ev = r.step();
    assert!(has(&ev, |e| *e == CharacterEvent::Dirty));
    ev.extend(r.until(60.0, |r, _| r.sprite(Owner::Gift, 1).is_some()));
    assert_eq!(ev.iter().filter(|e| matches!(e, CharacterEvent::GiftDropped { .. })).count(), 1);
    let s = r.until(60.0, |r, _| r.sprite(Owner::Character, 1).is_none());
    assert!(!has(&s, |e| matches!(e, CharacterEvent::GiftDropped { .. })));
    r.run(5.0);
    r.paint_all(1.0);
    let g = r.sprite(Owner::Gift, 1).expect("gift on screen");
    assert_eq!(g.hit, HitMode::Always);
    let at = r.v.gift.as_ref().unwrap().position();
    assert!(at.y == 38.0 || at.y == 2.0, "gift rests on the dock ledge or ground: {at:?}");
    // clicking it opens it
    let key = SpriteKey::new(Owner::Gift, 1);
    r.ptr = PointerInput { pos: Some(at + Vec2::new(5.0, 5.0)), left_down: true, right_down: false, hot: Some(key) };
    r.step();
    r.ptr.left_down = false;
    let ev = r.step();
    let open = Vec2::new(at.x, at.y + 24.0);
    assert!(has(&ev, |e| *e == CharacterEvent::GiftOpened { at: open }));
    assert!(r.sprite(Owner::Gift, 1).is_none());
    // no second gift today
    r.now += 20_000.0;
    r.run(1.0);
    assert!(r.v.last_gift.is_some());
    let saved = r.v.save_json();
    assert!(saved["lastSantaVisit"].is_string());
    assert!(saved["lastGift"].is_string());
}

#[test]
fn santa_hidden_screen_drops_nothing_and_vanishing_screen_ends_run() {
    let mut r = Rig::new(2);
    r.now = 11_000.0;
    r.step();
    assert!(r.sprite(Owner::Character, 1).is_some());
    r.screens[0].shown = false;
    let ev = r.run(40.0);
    assert!(!has(&ev, |e| matches!(e, CharacterEvent::GiftDropped { .. })));
    if let Some(s) = r.sprite(Owner::Character, 1) {
        assert_eq!(s.opacity, 0.0);
        r.paint_all(1.0);
    }
    r.screens.clear();
    r.step();
    assert!(r.sprite(Owner::Character, 1).is_none());
    r.screens = vec![scr(0.0, true, true)];
    // an automatic run that cannot start (no shown screen) retries later
    r.screens[0].shown = false;
    r.now += 20_000.0;
    r.step();
    assert!(r.sprite(Owner::Character, 1).is_none());
    r.screens[0].shown = true;
    r.step();
    assert!(r.sprite(Owner::Character, 1).is_some());
    r.v.stop();
    r.step();
    assert!(r.sprite(Owner::Character, 1).is_none());
}

#[test]
fn santa_away_and_often_toggles() {
    let mut r = Rig::new(2);
    r.v.set_away(&[Visitor::Santa, Visitor::Snowmen, Visitor::Yeti, Visitor::Plough]);
    r.now = 20_000.0;
    let ev = r.step();
    assert!(r.sprite(Owner::Character, 1).is_none());
    assert!(!has(&ev, |e| *e == CharacterEvent::FightDue));
    r.v.set_away(&[]);
    r.v.set_often(true);
    r.v.start(r.now);
    // often: the wait is 3600..10800 s divided by 6
    let next = r.v.santa_visits.next_at();
    assert!(next >= r.now + 600.0 && next <= r.now + 1800.0, "{next}");
    r.run(2000.0 / 10.0);
    assert!(r.v.santa_visits.next_at().is_finite());
}

#[test]
fn snowmen_fights_fire_when_due() {
    let mut r = Rig::new(6);
    r.now = 20_000.0;
    let ev = r.step();
    assert!(has(&ev, |e| *e == CharacterEvent::FightDue));
    // queueing writes nothing; the scheduler stays due until the world reports
    assert!(r.v.save_json().get("lastSnowmanFight").is_none());
    assert!(has(&r.run(2.0), |e| *e == CharacterEvent::FightDue));
    // a running bout gates due (snowman-fight 2.3)
    r.v.set_fight_active(true);
    assert!(!has(&r.step(), |e| *e == CharacterEvent::FightDue));
    r.v.set_fight_active(false);
    // bout begins: the visit is noted
    r.v.fight_came(r.wall);
    assert!(r.v.save_json()["lastSnowmanFight"].is_string());
    // bout ends: a fresh 7200..14400 wait counts from the finish
    let end = r.now;
    r.v.fight_ended(end);
    let next = r.v.fights.next_at();
    assert!(next >= end + 7200.0 && next <= end + 14_400.0, "{next}");
    assert!(!has(&r.run(2.0), |e| *e == CharacterEvent::FightDue));
    // no shown screen: the fight waits
    let mut r = Rig::new(6);
    r.screens = vec![scr(0.0, true, false)];
    r.now = 20_000.0;
    assert!(!has(&r.step(), |e| *e == CharacterEvent::FightDue));
}

#[test]
fn plough_manual_runs_on_dock_and_ground() {
    for dock in [true, false] {
        let mut r = Rig::new(4);
        r.screens = vec![scr(0.0, dock, true)];
        r.v.send_plough(0.0);
        r.step();
        assert!(r.v.plough.active());
        let frame = r.screens[0].frame;
        let (origin, blade) = r.v.blade().expect("blade while running");
        assert_eq!(origin, frame.min());
        assert_eq!(blade.on_dock, dock);
        assert_eq!(blade.speed, 120.0);
        assert!(r.v.plough_front(frame).is_some());
        assert!(r.v.plough_front(Frame::new(5.0, 5.0, 5.0, 5.0)).is_none());
        let mut painted = false;
        let mut opacity = Vec::new();
        for i in 0..3000 {
            r.step();
            match r.sprite(Owner::Character, 2) {
                Some(s) => {
                    opacity.push(s.opacity);
                    if i % 400 == 0 {
                        r.paint_all(1.0);
                        painted = true;
                    }
                }
                None => break,
            }
        }
        assert!(painted);
        assert!(r.v.blade().is_none());
        if dock {
            assert!(opacity.iter().any(|o| *o < 1.0 && *o > 0.0), "dock runs fade at the ends");
        }
    }
}

#[test]
fn plough_automatic_needs_depth_and_reschedules() {
    let mut r = Rig::new(8);
    r.screens = vec![scr(0.0, false, true), scr(2000.0, true, true)];
    let frames = [r.screens[0].frame, r.screens[1].frame];
    r.now = 1000.0;
    r.v.report_depth(frames[0], 10.0);
    r.v.report_depth(frames[1], 3.0);
    r.step();
    assert!(r.v.blade().is_none(), "too shallow on both screens");
    r.v.report_depth(frames[0], 25.0);
    r.step();
    assert!(r.v.blade().is_some());
    r.v.report_depth(frames[0], 0.0);
    r.until(60.0, |r, _| r.v.blade().is_none());
    // next run is at least 600 s away
    r.v.report_depth(frames[1], 30.0);
    r.run(10.0);
    assert!(r.v.blade().is_none());
    // a dock screen qualifies at depth 4
    r.now += 2000.0;
    r.step();
    assert!(r.v.blade().is_some());
    // run is dropped when its screen vanishes (and rescheduled)
    r.screens.clear();
    r.step();
    assert!(r.v.blade().is_none());
    // hidden screen: alpha 0
    r.screens = vec![scr(0.0, false, true)];
    r.v.send_plough(0.0);
    r.step();
    r.screens[0].shown = false;
    r.step();
    assert_eq!(r.sprite(Owner::Character, 2).unwrap().opacity, 0.0);
    // manual send with nothing shown is lost
    r.v.stop();
    r.screens[0].shown = true;
    r.v.start(r.now);
    r.screens[0].shown = false;
    r.v.send_plough(0.0);
    r.step();
    r.screens[0].shown = true;
    r.step();
    assert!(r.v.blade().is_none());
}

/// A snowman stack standing on `base`, bottom to top.
fn stack_of(base: Vec2, radii: &[f64]) -> Vec<(Vec2, f64)> {
    let mut y = base.y;
    radii
        .iter()
        .map(|&r| {
            let part = (Vec2::new(base.x, y + r), r);
            y += 2.0 * r;
            part
        })
        .collect()
}

fn prey_at(x: f64, y: f64) -> [(u64, Vec<(Vec2, f64)>); 1] {
    [(11, stack_of(Vec2::new(x, y), &[26.0, 19.0, 13.0]))]
}

fn hunt(r: &mut Rig, prey: &[(u64, Vec<(Vec2, f64)>)], limit: f64) -> Vec<CharacterEvent> {
    let end = r.now + limit;
    let mut all = Vec::new();
    let mut started = false;
    while r.now < end {
        let ev = r.tick(1.0 / 60.0, prey, None, &[]);
        all.extend(ev);
        let on = r.sprite(Owner::Character, 3).is_some();
        started |= on;
        if started && !on && r.sprites().iter().all(|s| s.key.id() < 16) {
            return all;
        }
    }
    panic!("yeti hunt did not finish");
}

#[test]
fn yeti_dashes_to_a_floor_snowman_and_smashes_it() {
    for (seed, x) in [(1u64, 500.0), (2, 1500.0)] {
        let mut r = Rig::new(seed);
        r.v.send_yeti(0.0);
        let ev = hunt(&mut r, &prey_at(x, 39.0), 40.0);
        assert!(ev.contains(&CharacterEvent::Smash { bottom: 11 }));
    }
}

#[test]
fn yeti_throws_at_a_ledge_snowman() {
    let mut painted = 0;
    for (seed, x) in [(1u64, 500.0), (2, 1500.0), (3, 700.0)] {
        let mut r = Rig::new(seed);
        r.v.send_yeti(0.0);
        let prey = prey_at(x, 400.0);
        let end = r.now + 60.0;
        let mut all = Vec::new();
        let mut started = false;
        while r.now < end {
            all.extend(r.tick(1.0 / 60.0, &prey, None, &[]));
            let on = r.sprite(Owner::Character, 3).is_some();
            started |= on;
            if on && (r.now * 60.0) as i64 % 20 == 0 {
                r.paint_all(1.0);
                painted += 1;
            }
            if started && !on && r.sprites().is_empty() {
                break;
            }
        }
        assert!(started);
        assert!(has(&all, |e| matches!(e, CharacterEvent::Burst { area, .. } if *area == 160.0)));
    }
    assert!(painted > 10);
}

#[test]
fn yeti_gives_up_when_the_snowman_vanishes() {
    let mut r = Rig::new(5);
    r.v.send_yeti(0.0);
    let prey = prey_at(500.0, 400.0);
    let mut thrown = false;
    for _ in 0..4000 {
        let live: &[(u64, Vec<(Vec2, f64)>)] = if thrown { &[] } else { &prey };
        r.tick(1.0 / 60.0, live, None, &[]);
        thrown |= r.sprites().iter().any(|s| s.key.id() >= 16);
        if thrown && r.sprites().is_empty() {
            return;
        }
    }
    panic!("never finished");
}

#[test]
fn yeti_chops_a_doomed_tree_three_times() {
    let mut r = Rig::new(7);
    let tree = Some((4u64, Vec2::new(900.0, 150.0), 30.0));
    let end = r.now + 60.0;
    let mut chops = 0;
    let mut started = false;
    while r.now < end {
        let ev = r.tick(1.0 / 60.0, &[], tree, &[]);
        chops += ev.iter().filter(|e| matches!(e, CharacterEvent::Chop { tree: 4 })).count();
        let on = r.sprite(Owner::Character, 3).is_some();
        if on && (r.now * 60.0) as i64 % 15 == 0 {
            r.paint_all(1.0);
        }
        started |= on;
        if chops >= 3 && started && !on {
            break;
        }
    }
    assert_eq!(chops, 3);
    // a vanished tree sends the yeti home sad
    let mut r = Rig::new(7);
    let mut seen = false;
    for i in 0..3000 {
        let t = if i < 120 { tree } else { None };
        r.tick(1.0 / 60.0, &[], t, &[]);
        let on = r.sprite(Owner::Character, 3).is_some();
        if seen && !on {
            return;
        }
        seen |= on;
    }
    panic!("yeti never left");
}

#[test]
fn yeti_automatic_hunt_respects_toggle_and_timeout() {
    // away: never hunts
    let mut r = Rig::new(3);
    r.v.set_away(&[Visitor::Yeti]);
    let prey = prey_at(500.0, 39.0);
    r.tick(5000.0, &prey, None, &[]);
    r.tick(0.01, &prey, None, &[]);
    assert!(r.sprite(Owner::Character, 3).is_none());
    // coming: the countdown runs while prey exists
    let mut r = Rig::new(3);
    r.tick(5000.0, &prey, None, &[]);
    r.tick(0.01, &prey, None, &[]);
    assert!(r.sprite(Owner::Character, 3).is_some());
    // a manual send with no target times out
    let mut r = Rig::new(3);
    r.v.send_yeti(0.0);
    r.tick(100.0, &[], None, &[]);
    r.tick(0.01, &prey, None, &[]);
    assert!(r.sprite(Owner::Character, 3).is_none());
    // a target outside every screen cannot be hunted
    r.v.send_yeti(r.now);
    r.tick(0.01, &prey_at(5000.0, 39.0), None, &[]);
    assert!(r.sprite(Owner::Character, 3).is_none());
}

#[test]
fn yeti_hidden_or_lost_screen() {
    let mut r = Rig::new(3);
    r.v.send_yeti(0.0);
    let prey = prey_at(500.0, 39.0);
    r.tick(0.1, &prey, None, &[]);
    r.screens[0].shown = false;
    r.tick(0.1, &prey, None, &[]);
    assert_eq!(r.sprite(Owner::Character, 3).unwrap().opacity, 0.0);
    r.screens.clear();
    r.tick(0.1, &prey, None, &[]);
    assert!(r.sprite(Owner::Character, 3).is_none());
}

fn cargo(snowman: bool) -> Cargo {
    if snowman {
        Cargo { snowman: Some(json!({"radii": [30.0, 20.0, 12.0]})), tree: None }
    } else {
        Cargo { tree: Some(json!({"seed": 4})), snowman: None }
    }
}

fn ride(look: TrainLook, pick: Pick, x: f64, snowman: bool) -> Rig {
    let mut r = Rig::new(5);
    r.v.set_train_look(look);
    r.v.summon_train(0.0);
    assert!(r.v.train.busy());
    let mut painted = 0;
    let ev = r.until(60.0, |r, ev| {
        if r.now.fract() < 0.02 && painted < 40 {
            r.paint_all(1.0);
            painted += 1;
        }
        has(ev, |e| matches!(e, CharacterEvent::TrainReady { .. }))
    });
    let ready =
        ev.iter().find_map(|e| if let CharacterEvent::TrainReady { at } = e { Some(*at) } else { None }).unwrap();
    // braking stops the nose at frame.MaxX - max(4% of width, 24); the elf
    // waits 44 behind the tail
    let stop_at = 1920.0 - 1920.0 * 0.04;
    assert!((ready.x - (stop_at - look.length() - 44.0)).abs() < 1e-6, "{ready:?}");
    assert_eq!(ready.y, 0.0);
    assert!(r.v.train.at_station());
    r.paint_all(2.0);
    r.v.train_fetch(pick, x, r.now);
    assert!(r.v.carry_point().is_none(), "nothing is carried before the pickup");
    let mut took = false;
    let mut loaded = false;
    let mut carried = 0;
    let mut cargo_given = false;
    let ev = r.until(60.0, |r, ev| {
        for e in ev {
            match e {
                CharacterEvent::Took { pick: p } => {
                    assert_eq!(*p, pick);
                    took = true;
                }
                CharacterEvent::Loaded { .. } => {
                    loaded = true;
                    r.v.load(cargo(snowman));
                    cargo_given = true;
                }
                _ => {}
            }
        }
        if took && r.v.carry_point().is_some() {
            carried += 1;
        }
        r.now.fract() < 0.02 && {
            r.paint_all(1.0);
            false
        } || has(ev, |e| *e == CharacterEvent::Left)
    });
    assert!(took && loaded && cargo_given && carried > 10);
    assert!(has(&ev, |e| matches!(e, CharacterEvent::Burst { area, .. } if *area == 220.0)));
    assert!(has(&ev, |e| *e == CharacterEvent::Dirty));
    assert_eq!(r.v.train.queue_len(), 1);
    r
}

#[test]
fn train_summon_fetch_and_delivery_round_trip() {
    for (look, pick, x, snowman) in [
        (TrainLook::Festive, Pick::Tree(3), 1200.0, false),
        (TrainLook::Classic, Pick::Snowman(9), 1500.0, true),
        (TrainLook::Festive, Pick::Snowman(9), 1263.0 - 44.0, false),
    ] {
        let mut r = ride(look, pick, x, snowman);
        let saved = r.v.save_json();
        assert_eq!(saved["trainQueue"].as_array().unwrap().len(), 1);
        assert_eq!(saved["trainLook"], json!(look.raw()));
        // the delivery arrives later, unloads once, then the queue is empty
        let ev = r.until(300.0, |r, ev| {
            if r.v.cargo_aboard().is_some() && r.v.carry_point().is_some() && r.now.fract() < 0.02 {
                r.paint_all(1.0);
            }
            has(ev, |e| matches!(e, CharacterEvent::Unload { .. }))
        });
        let (c, ux) = ev
            .iter()
            .find_map(
                |e| if let CharacterEvent::Unload { cargo, x, .. } = e { Some((cargo.clone(), *x)) } else { None },
            )
            .unwrap();
        assert_eq!(c, cargo(snowman));
        assert!(ux < 1920.0 - 96.0);
        assert!(r.v.cargo_aboard().is_none(), "placed");
        let ev = r.until(60.0, |_, ev| has(ev, |e| *e == CharacterEvent::Dirty));
        assert!(!has(&ev, |e| *e == CharacterEvent::Left));
        assert_eq!(r.v.train.queue_len(), 0);
        assert!(r.v.carry_point().is_none());
        assert!(r.sprites().iter().all(|s| s.key.owner_byte() != Owner::Character as u8));
    }
}

#[test]
fn train_snowman_chase_brings_the_yeti() {
    let mut r = Rig::new(5);
    r.v.summon_train(0.0);
    r.until(60.0, |_, ev| has(ev, |e| matches!(e, CharacterEvent::TrainReady { .. })));
    r.v.train_fetch(Pick::Snowman(2), 1200.0, r.now);
    let mut yeti_seen = false;
    r.until(60.0, |r, ev| {
        if has(ev, |e| matches!(e, CharacterEvent::Loaded { .. })) {
            r.v.load(cargo(true));
        }
        yeti_seen |= r.sprite(Owner::Character, 3).is_some();
        has(ev, |e| *e == CharacterEvent::Left)
    });
    assert!(yeti_seen, "the yeti chased the riding snowman");
}

#[test]
fn riding_stacks_real_radii_from_the_carry_point() {
    let mut r = Rig::new(5);
    assert!(r.v.train.riding().is_none(), "no run");
    r.v.summon_train(0.0);
    r.until(60.0, |_, ev| has(ev, |e| matches!(e, CharacterEvent::TrainReady { .. })));
    r.v.train_fetch(Pick::Snowman(2), 1200.0, r.now);
    r.until(60.0, |r, ev| {
        if has(ev, |e| matches!(e, CharacterEvent::Loaded { .. })) {
            r.v.load(cargo(true));
        }
        r.v.carry_point().is_some() && r.v.cargo_aboard().is_some()
    });
    let foot = r.v.carry_point().unwrap();
    r.v.train.test_set_cargo(Some(json!({"radii": [60.0, 45.0, 35.0, 25.0]})));
    let parts = r.v.train.riding().unwrap();
    let want = [(foot.y + 60.0, 60.0), (foot.y + 165.0, 45.0), (foot.y + 245.0, 35.0), (foot.y + 305.0, 25.0)];
    assert_eq!(parts.len(), 4);
    for ((c, rad), (y, wr)) in parts.iter().zip(want) {
        assert_eq!((c.x, c.y, *rad), (foot.x, y, wr));
    }
    for bad in [json!({"radii": []}), json!({"radii": [10.0, -1.0]}), json!({"radii": "x"}), json!({})] {
        r.v.train.test_set_cargo(Some(bad));
        assert!(r.v.train.riding().is_none());
    }
    r.v.train.test_set_cargo(None);
    assert!(r.v.train.riding().is_none());
}

#[test]
fn train_dismiss_waves_goodbye() {
    let mut r = Rig::new(5);
    // not started or busy: ignored
    r.v.stop();
    r.v.summon_train(0.0);
    assert!(!r.v.train.busy());
    r.v.start(r.now);
    r.v.summon_train(0.0);
    r.v.summon_train(0.0);
    r.v.train_dismiss(0.0);
    r.v.train_fetch(Pick::Tree(1), 1.0, 0.0);
    r.until(60.0, |_, ev| has(ev, |e| matches!(e, CharacterEvent::TrainReady { .. })));
    r.run(9.0);
    r.paint_all(1.0);
    r.v.train_dismiss(r.now);
    r.v.load(cargo(false));
    let ev = r.until(30.0, |r, ev| {
        if r.now.fract() < 0.02 {
            r.paint_all(1.0);
        }
        has(ev, |e| *e == CharacterEvent::Left)
    });
    assert!(!has(&ev, |e| *e == CharacterEvent::Dirty), "dismissed run carries nothing");
    assert_eq!(r.v.train.queue_len(), 0);
}

#[test]
fn train_screen_loss_and_stop() {
    let mut r = Rig::new(5);
    r.v.summon_train(0.0);
    r.run(2.0);
    r.screens.clear();
    let ev = r.step();
    assert!(has(&ev, |e| *e == CharacterEvent::Left));
    assert!(r.sprite(Owner::Character, 4).is_none());
    // stop mid run: Left is delivered on the next step
    r.screens = vec![scr(0.0, true, true)];
    r.v.summon_train(0.0);
    r.run(2.0);
    r.v.stop();
    r.v.stop();
    assert!(has(&r.step(), |e| *e == CharacterEvent::Left));
    // a delivery run that loses its screen stays queued
    r.v.start(r.now);
    r.v.load_json(&json!({"trainQueue": [{"id": 4, "from": "pal", "cargo": {"tree": {"a": 1}}}]}));
    r.until(120.0, |r, _| r.sprite(Owner::Character, 4).is_some());
    r.screens.clear();
    let ev = r.step();
    assert!(!has(&ev, |e| *e == CharacterEvent::Left));
    assert_eq!(r.v.train.queue_len(), 1);
    // hidden screen: transparent train
    r.screens = vec![scr(0.0, true, true)];
    r.until(120.0, |r, _| r.sprite(Owner::Character, 4).is_some());
    r.screens[0].shown = false;
    r.step();
    assert_eq!(r.sprite(Owner::Character, 4).unwrap().opacity, 0.0);
}

#[test]
fn deliveries_prefer_another_screen() {
    let mut r = Rig::new(5);
    r.screens = vec![scr(0.0, true, true), scr(2000.0, false, true)];
    r.v.summon_train(0.0);
    r.until(60.0, |_, ev| has(ev, |e| matches!(e, CharacterEvent::TrainReady { .. })));
    r.v.train_fetch(Pick::Tree(1), 1200.0, r.now);
    r.until(60.0, |r, ev| {
        if has(ev, |e| matches!(e, CharacterEvent::Loaded { .. })) {
            r.v.load(cargo(false));
        }
        has(ev, |e| *e == CharacterEvent::Left)
    });
    let ev = r.until(300.0, |_, ev| has(ev, |e| matches!(e, CharacterEvent::Unload { .. })));
    let screen = ev
        .iter()
        .find_map(|e| if let CharacterEvent::Unload { screen, .. } = e { Some(*screen) } else { None })
        .unwrap();
    assert_eq!(screen.x, 2000.0);
}

#[test]
fn persistence_round_trip_and_lenient_loading() {
    let mut r = Rig::new(1);
    r.v.load_json(&json!({
        "lastSantaVisit": "2026-11-16T10:00:00Z",
        "lastSnowmanFight": "bad",
        "lastGift": "2026-11-16",
        "trainLook": "classic",
        "trainQueue": [
            {"id": 2, "from": "a", "cargo": {"snowman": {"radii": [10]}, "note": "ignored"}},
            {"id": 3},
            {"from": "no id"},
            7
        ]
    }));
    assert_eq!(r.v.train.queue_len(), 2);
    assert_eq!(r.v.train.look(), TrainLook::Classic);
    let saved = r.v.save_json();
    assert_eq!(saved["lastSantaVisit"], json!("2026-11-16T10:00:00Z"));
    assert!(saved.get("lastSnowmanFight").is_none());
    assert_eq!(saved["lastGift"], json!("2026-11-16"));
    assert_eq!(saved["trainQueue"][1]["id"], json!(3));
    assert_eq!(saved["trainQueue"][0]["cargo"]["snowman"]["radii"][0], json!(10));
    // a delivery queued while the train is running starts soon
    let mut v2 = Visitors::new(1);
    v2.load_json(&saved);
    assert_eq!(v2.save_json(), saved);
    // garbage loads leave a clean state
    v2.load_json(&json!(17));
    assert_eq!(v2.train.queue_len(), 0);
    assert_eq!(v2.train.look(), TrainLook::Festive);
    assert!(v2.save_json().get("lastGift").is_none());
    // queue present at start schedules a first delivery
    let mut v3 = Visitors::new(1);
    v3.load_json(&saved);
    v3.start(100.0);
    assert!(v3.train.queue_len() == 2);
    r.v.load_json(&json!({"trainQueue": [{"id": 9, "cargo": {"tree": {}}}]}));
    assert_eq!(r.v.train.queue_len(), 1);
}

#[test]
fn groundlife_scene_paints_at_all_phases() {
    for t in [
        0.0,
        0.5,
        2.0,
        4.25,
        4.2 + 2.4 * 0.2,
        4.2 + 2.4 * 0.4,
        4.2 + 2.4 * 0.7,
        4.2 + 2.4 * 1.4,
        4.2 + 2.4 * 1.7,
        100.0,
    ] {
        let mut pm = fp_paint::tiny_skia::Pixmap::new(980, 150).unwrap();
        groundlife::paint(&mut pm, 1.0, t);
        assert!(pm.pixels().iter().any(|p| p.alpha() > 0), "t={t}");
    }
    let mut pm = fp_paint::tiny_skia::Pixmap::new(1470, 225).unwrap();
    groundlife::paint(&mut pm, 1.5, 6.0);
    assert_eq!((groundlife::WIDTH, groundlife::HEIGHT), (980.0, 150.0));
}

#[test]
fn plough_order_follows_behind_windows() {
    for behind in [false, true] {
        let mut r = Rig::new(4);
        r.v.set_behind_windows(behind);
        r.v.send_plough(0.0);
        r.step();
        let s = r.sprite(Owner::Character, 2).expect("plough");
        assert_eq!(s.z, if behind { fp_core::scene::ZBand::UnderDock } else { fp_core::scene::ZBand::Front });
    }
}

#[test]
fn empty_delivery_passes_without_stopping() {
    let mut r = Rig::new(5);
    r.v.load_json(&json!({"trainQueue": [{"id": 1, "from": "", "cargo": {}}]}));
    r.until(120.0, |r, _| r.sprite(Owner::Character, 4).is_some());
    assert!(r.v.cargo_aboard().is_none() && r.v.carry_point().is_none());
    let mut slowest = f64::INFINITY;
    let mut last = r.sprite(Owner::Character, 4).unwrap().frame.x;
    let ev = r.until(60.0, |r, ev| {
        if let Some(s) = r.sprite(Owner::Character, 4) {
            slowest = slowest.min((s.frame.x - last) * 60.0);
            last = s.frame.x;
        }
        has(ev, |e| *e == CharacterEvent::Dirty)
    });
    assert!((slowest - 150.0).abs() < 1e-6, "constant cruise: {slowest}");
    assert!(!has(&ev, |e| matches!(e, CharacterEvent::Unload { .. } | CharacterEvent::Left)));
    assert_eq!(r.v.train.queue_len(), 0);
}

#[test]
fn elf_hop_arms_win_over_waving_but_not_carrying() {
    let hop = art::ElfLook { t: 0.3, walking: false, carrying: false, waving: true, hopping: true };
    assert_eq!(art::elf_angles(&hop).1, [0.9, -0.9]);
    assert_eq!(art::elf_angles(&hop).0, [0.8, 0.48]);
    let carry = art::ElfLook { carrying: true, ..hop };
    assert!(art::elf_angles(&carry).1[0] > 2.7);
    let walk = art::ElfLook { t: 0.1, walking: true, carrying: false, waving: false, hopping: false };
    let s = 0.6 * (1.1f64).sin();
    let (legs, arms) = art::elf_angles(&walk);
    assert!((f64::from(legs[0]) - s).abs() < 1e-6 && (f64::from(arms[1]) - 0.8 * s).abs() < 1e-6);
}

#[test]
fn quote_pick_is_reexported() {
    assert_eq!(quote::key(fp_core::effect::Lang::En), "quotesLeft");
}

fn santa_peak(obst: &[crate::Block]) -> f64 {
    let mut r = Rig::new(3);
    r.v.send_santa(0.0);
    let mut peak = 0.0f64;
    for _ in 0..4000 {
        r.tick(1.0 / 60.0, &[], None, obst);
        if let Some(s) = r.sprite(Owner::Character, 1) {
            peak = peak.max(s.frame.y);
        }
    }
    peak
}

#[test]
fn santa_ignores_obstacles_on_other_screens() {
    let near = Frame::new(0.0, 0.0, 1920.0, 1080.0);
    let far = Frame::new(5000.0, 0.0, 1920.0, 1080.0);
    let free = santa_peak(&[]);
    assert_eq!(santa_peak(&[(far, 0.0, 1920.0, 0.0, 900.0)]), free);
    assert!(santa_peak(&[(near, 0.0, 1920.0, 0.0, 900.0)]) > free + 100.0);
}
