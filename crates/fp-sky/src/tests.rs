use super::*;
use fp_core::geom::{Frame, Rect4};

fn ledge(id: i64, x: f64, y: f64, w: f64, h: f64) -> Surface {
    Surface::new(SurfaceId::Window(id), Frame::new(x, y, w, h))
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn star(pos: Vec2, vel: Vec2) -> Star {
    Star {
        pos,
        v: vel,
        fall: 30.0,
        size: 4.0,
        angle: 0.0,
        spin: 0.0,
        twinkle: 0.0,
        rate: 2.0,
        alpha: 0.8,
        hue: 0.0,
        rest: None,
    }
}

fn meteor(head: Vec2, dir: Vec2, bright: bool) -> Meteor {
    Meteor {
        head,
        v: dir * 1000.0,
        dir,
        age: 0.0,
        life: 0.8,
        length: 300.0,
        width: 2.0,
        brightness: 0.9,
        hue: 0.2,
        bright,
        hit_at: None,
        last_spark_age: 0.0,
    }
}

fn quiet(f: &mut StarfallField) {
    f.next_meteor = 1e9;
}

/// Alpha (5th f32) of the first star sprite of a Stars pass.
fn first_alpha(d: &DrawList) -> f32 {
    let b = &d.passes[0].buffers[0];
    f32::from_le_bytes(b[16..20].try_into().unwrap())
}

#[test]
fn hit_test_cases() {
    let mut a = ledge(1, 100.0, 0.0, 200.0, 300.0);
    a.corner_radius = 20.0;
    let b = ledge(2, 100.0, 0.0, 200.0, 400.0);
    let mut blk = ledge(3, 100.0, 0.0, 200.0, 500.0);
    blk.blockers.push(Rect4::new(0.0, 450.0, 1000.0, 600.0));
    let ground = Surface::new(SurfaceId::Ground, Frame::new(0.0, 0.0, 1000.0, 50.0));

    let h = hit_test(&[a.clone()], v(150.0, 310.0), v(170.0, 290.0)).unwrap();
    assert_eq!((h.x, h.y), (160.0, 300.0));
    let h = hit_test(&[b.clone(), a.clone()], v(150.0, 450.0), v(150.0, 250.0)).unwrap();
    assert_eq!(h.y, 400.0);
    let h = hit_test(&[a.clone(), b.clone()], v(150.0, 450.0), v(150.0, 250.0)).unwrap();
    assert_eq!(h.y, 400.0);
    // inset by corner radius * 0.5 = 10 (x = 105 is inside the frame but in the inset)
    assert!(hit_test(&[a.clone()], v(105.0, 310.0), v(105.0, 290.0)).is_none());
    assert!(hit_test(&[a.clone()], v(500.0, 310.0), v(500.0, 290.0)).is_none());
    // blocker removes the ledge; the ground still catches
    let h = hit_test(&[blk], v(150.0, 520.0), v(150.0, -5.0));
    assert_eq!(h.unwrap().y, 3.0);
    // ground surface entries are ignored
    let h = hit_test(&[ground], v(10.0, 10.0), v(20.0, -4.0)).unwrap();
    assert_eq!(h.y, 3.0);
    assert!(hit_test(&[], v(10.0, 100.0), v(10.0, 50.0)).is_none());
    // degenerate horizontal segment exactly on a top
    let h = hit_test(&[a], v(150.0, 300.0), v(160.0, 300.0)).unwrap();
    assert_eq!(h.x, 150.0);
}

#[test]
fn starfall_population_and_basics() {
    let mut f = StarfallField::new(1920.0, 1080.0, false, 7);
    assert_eq!(f.stars.len(), 58);
    assert!(f.stars.iter().all(|s| s.pos.y >= 0.0 && s.pos.y <= 1080.0));
    assert_eq!(f.share(), 1.0);
    f.set_share(2.0);
    assert_eq!(f.share(), 2.0);
    assert_eq!(f.target(), 116);
    assert!(f.next_meteor >= 0.5 && f.next_meteor < 2.0);
    assert!(f.radiant == -2.55 || f.radiant == -0.6);
    let slow = StarfallField::new(100.0, 100.0, true, 1);
    assert_eq!(slow.slow, 0.5);
    let radiants: Vec<f64> = (0..20).map(|s| StarfallField::new(100.0, 100.0, false, s).radiant).collect();
    assert!(radiants.contains(&-2.55) && radiants.contains(&-0.6));
}

#[test]
fn step_noop_for_zero_dt_and_topup() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 3);
    let n = f.stars.len();
    f.step(0.0, None);
    f.step(-1.0, None);
    assert_eq!(f.clock, 0.0);
    quiet(&mut f);
    f.stars.truncate(n - 10);
    f.step(0.016, None);
    assert_eq!(f.stars.len(), n - 10 + 3);
    f.set_wind(Wind::StrongRight);
    f.step(0.016, None);
    assert!(f.blow.speed > 0.0);
    assert!(f.stars.iter().any(|s| s.pos.y > 1000.0));
}

#[test]
fn star_lands_on_ledge_rests_and_is_removed() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 3);
    quiet(&mut f);
    f.stars.clear();
    f.set_share(0.0);
    f.update(&[ledge(5, 100.0, 100.0, 300.0, 200.0)], 0.1, false);
    f.stars.push(star(v(200.0, 300.5), v(0.0, -30.0)));
    f.step(0.1, None);
    let s = &f.stars[0];
    let r = s.rest.unwrap();
    assert_eq!(r.id, SurfaceId::Window(5));
    assert!((r.along - 100.0).abs() < 1.0);
    assert!((s.pos.y - (300.0 + 4.0 * 0.35)).abs() < 1e-9);
    assert!(r.until >= f.clock + 3.0 + FADE_TIME && r.until < f.clock + 8.0 + FADE_TIME);
    // moves with the ledge
    f.update(&[ledge(5, 150.0, 120.0, 300.0, 200.0)], 0.1, false);
    f.step(0.1, None);
    assert!((f.stars[0].pos.x - (150.0 + r.along)).abs() < 1e-9);
    assert!((f.stars[0].pos.y - 321.4).abs() < 1e-9);
    let mut d = DrawList::new(1000.0, 1000.0, 0.0);
    f.draw(Layer::Main, &mut d);
    assert_eq!(d.passes.len(), 1);
    assert!(first_alpha(&d) > 0.0);
    // a blocker hides it
    let mut l = ledge(5, 150.0, 120.0, 300.0, 200.0);
    l.blockers.push(Rect4::new(0.0, 0.0, 2000.0, 2000.0));
    f.update(&[l], 0.1, false);
    let mut d = DrawList::new(1000.0, 1000.0, 0.0);
    f.draw(Layer::Main, &mut d);
    assert_eq!(first_alpha(&d), 0.0);
    // ledge vanishes: rest cleared
    f.update(&[], 0.1, false);
    assert!(f.stars[0].rest.is_none());
    // ground resting: lands, stays put, never cleared by update
    f.stars.clear();
    f.stars.push(star(v(500.0, 5.0), v(0.0, -60.0)));
    f.step(0.1, None);
    assert_eq!(f.stars[0].rest.unwrap().id, SurfaceId::Ground);
    let at = f.stars[0].pos;
    f.update(&[], 0.1, false);
    assert!(f.stars[0].rest.is_some());
    f.step(0.1, None);
    assert_eq!(f.stars[0].pos, at);
    // a falling star is ignored by update
    f.stars.push(star(v(1.0, 500.0), v(0.0, -1.0)));
    f.update(&[], 0.1, true);
    // expiry
    for _ in 0..100 {
        f.step(0.1, None);
    }
    assert!(f.stars.iter().all(|s| s.rest.is_none_or(|r| r.until > f.clock)));
    assert!(f.stars.len() < 2);
}

#[test]
fn star_motion_wrap_wake_and_removal() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 9);
    quiet(&mut f);
    f.set_share(0.0);
    f.stars.clear();
    f.stars.push(star(v(-39.0, 500.0), v(-400.0, -1.0)));
    f.stars.push(star(v(1039.0, 500.0), v(400.0, -1.0)));
    f.stars.push(star(v(500.0, -9.5), v(0.0, -200.0)));
    f.stars.push(star(v(500.0, 500.0), v(0.0, -30.0)));
    f.stars.push(star(v(700.0, 500.0), v(0.0, 400.0)));
    f.step(0.1, Some(v(500.0, 500.0)));
    assert_eq!(f.stars.len(), 4);
    assert!(f.stars[0].pos.x > 900.0);
    assert!(f.stars[1].pos.x < 100.0);
    // wake: the cursor moves past a star
    let before = f.stars[2].spin;
    f.step(0.1, Some(v(540.0, 500.0)));
    assert!(f.pointer_vel.len() > 20.0);
    assert_ne!(f.stars[2].spin, before);
    // far star unaffected, no-cursor path, slow pointer
    f.step(0.1, None);
    f.step(0.1, Some(v(0.0, 0.0)));
    f.step(0.1, Some(v(0.5, 0.0)));
    assert!(f.stars.len() >= 2);
}

#[test]
fn meteors_free_and_strike_soak() {
    for seed in 0..40 {
        let mut f = StarfallField::new(1000.0, 800.0, seed % 3 == 0, seed);
        let mut b = ledge(3, 500.0, 100.0, 400.0, 100.0);
        b.blockers.push(Rect4::new(0.0, 0.0, 2000.0, 2000.0));
        f.update(&[ledge(1, 100.0, 200.0, 300.0, 100.0), ledge(2, 600.0, 300.0, 100.0, 50.0), b], 0.1, false);
        let mut d = DrawList::new(1000.0, 800.0, 0.0);
        let mut launched = 0;
        for i in 0..800 {
            let before = f.next_meteor;
            f.step(0.016, if i % 50 < 25 { Some(v(300.0, 400.0)) } else { None });
            if f.next_meteor != before {
                launched += 1;
                // gap / (slow * max(share, 0.5)), gap in [0.3, 4.5)
                let gap = (f.next_meteor - f.clock) * f.slow;
                assert!((0.3..4.5).contains(&gap), "gap {gap}");
            }
            // burst cap 500 plus one exempt flash; trail sparks stop at 400 (+1)
            assert!(f.sparks.len() <= 501);
            f.draw(Layer::Main, &mut d);
            for p in &d.passes {
                for c in p.buffers[0].chunks(4) {
                    assert!(f32::from_le_bytes(c.try_into().unwrap()).is_finite());
                }
            }
            for s in floats(&d, PassKind::Stars).chunks(6) {
                assert!((0.0..=1.0).contains(&s[4]), "alpha {}", s[4]);
            }
            d.clear();
        }
        assert!(launched >= 2, "seed {seed}: {launched}");
    }
}

/// All f32s of the first pass of `kind`.
fn floats(d: &DrawList, kind: PassKind) -> Vec<f32> {
    d.passes
        .iter()
        .find(|p| p.kind == kind)
        .map(|p| p.buffers[0].chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
        .unwrap_or_default()
}

#[test]
fn bad_input_is_harmless() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 1);
    f.step(f64::NAN, None);
    f.step(f64::INFINITY, None);
    assert_eq!(f.clock, 0.0);
    f.set_share(f64::INFINITY);
    assert_eq!(f.target(), 0);
    let mut g = FireflyField::new(1000.0, 1000.0, false, 1);
    g.step(f64::NAN, None);
    assert_eq!(g.clock, 0.0);
    g.set_share(f64::INFINITY);
    assert_eq!(g.flies.len(), 8);
    g.set_share(f64::NAN);
    assert_eq!(g.flies.len(), 8);
    // zero-size fields still work
    let mut z = StarfallField::new(0.0, 0.0, false, 2);
    assert!(z.stars.is_empty());
    for _ in 0..300 {
        z.step(0.016, None);
    }
    assert!(z.sparks.iter().all(|s| s.pos.x.is_finite() && s.pos.y.is_finite()));
}

#[test]
fn star_spawn_ranges() {
    let mut f = StarfallField::new(1000.0, 1000.0, true, 21);
    f.stars.clear();
    for _ in 0..500 {
        f.spawn_star(10.0);
    }
    for s in &f.stars {
        // q in [0, 1): fall = (14 + 26q) * slow, size = 3 + 5q, alpha = 0.45 + 0.5q
        let q = (s.size - 3.0) / 5.0;
        assert!((0.0..1.0).contains(&q));
        assert!((s.fall - (14.0 + 26.0 * q) * 0.5).abs() < 1e-9);
        assert!((s.alpha - (0.45 + 0.5 * q)).abs() < 1e-9);
        assert!((0.0..1000.0).contains(&s.pos.x));
        assert!((0.0..PI / 2.0).contains(&s.angle));
        assert!(s.spin.abs() <= 0.2);
        assert!((1.5..4.0).contains(&s.rate));
        assert!(HUES.contains(&s.hue));
        assert_eq!(s.v, v(0.0, -s.fall));
    }
}

#[test]
fn star_velocity_eases_toward_fall_and_wind() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 4);
    quiet(&mut f);
    f.set_share(0.0);
    f.stars.clear();
    f.stars.push(star(v(500.0, 900.0), v(0.0, 0.0)));
    f.step(0.1, None);
    // v += (target - v) * 0.15; twinkle = 0.2 -> x target = sin(0.06) * 6
    let s = &f.stars[0];
    assert!((s.v.y - (-30.0 * 0.15)).abs() < 1e-9);
    assert!((s.v.x - (0.06f64).sin() * 6.0 * 0.15).abs() < 1e-9);
    assert!((s.twinkle - 0.2).abs() < 1e-12);
    // a steady right wind pushes falling stars right
    f.set_wind(Wind::StrongRight);
    for _ in 0..200 {
        f.step(0.05, None);
    }
    assert!(f.blow.speed > 0.0);
    assert!(f.stars.iter().all(|s| s.rest.is_some() || s.v.x > 0.0));
}

#[test]
fn wake_pushes_along_pointer() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 4);
    quiet(&mut f);
    f.set_share(0.0);
    f.stars.clear();
    f.stars.push(star(v(500.0, 500.0), v(0.0, -30.0)));
    f.stars.push(star(v(800.0, 500.0), v(0.0, -30.0)));
    f.step(0.1, Some(v(480.0, 500.0)));
    let far_vx = f.stars[1].v.x;
    // pointer moves +60 px in 0.1 s = 600 px/s to the right
    f.step(0.1, Some(v(540.0, 500.0)));
    assert!((f.pointer_vel.x - 600.0).abs() < 1e-9);
    assert!(f.stars[0].v.x > 50.0);
    assert!((f.stars[1].v.x - far_vx).abs() < 1.0);
}

#[test]
fn strike_reaches_target_and_skips_narrow_or_blocked() {
    let mut f = StarfallField::new(1000.0, 800.0, false, 13);
    let mut blk = ledge(3, 500.0, 100.0, 400.0, 100.0);
    blk.blockers.push(Rect4::new(0.0, 0.0, 2000.0, 2000.0));
    // narrow (exactly 120) and blocked ledges never become targets
    f.update(&[ledge(2, 600.0, 300.0, 120.0, 50.0), blk], 0.1, false);
    for _ in 0..40 {
        f.meteors.clear();
        f.strike();
        let m = &f.meteors[0];
        let hit = m.head + m.v * (m.life * 0.65);
        assert!((hit.y - GROUND_Y).abs() < 1e-6);
        assert!((100.0..=900.0).contains(&hit.x));
    }
    // a wide clear ledge: targets land on its top, 30 px in from both ends
    f.update(&[ledge(1, 100.0, 200.0, 300.0, 100.0)], 0.1, false);
    let mut on_ledge = 0;
    for _ in 0..60 {
        f.meteors.clear();
        f.strike();
        let m = &f.meteors[0];
        let hit = m.head + m.v * (m.life * 0.65);
        if (hit.y - 300.0).abs() < 1e-6 {
            on_ledge += 1;
            assert!((130.0..=370.0).contains(&hit.x));
        }
    }
    assert!(on_ledge > 10 && on_ledge < 50);
}

#[test]
fn trail_and_spark_render_values() {
    let mut m = meteor(v(500.0, 700.0), v(1.0, 0.0), false);
    // t = 0.06 / 0.8 = 0.075 -> envelope 0.625, length = 300 * 0.36
    m.age = 0.06;
    let t = trail_of(&m);
    assert!((t.alpha - 0.9 * 0.625).abs() < 1e-6);
    assert!((t.tail[0] - (500.0 - 108.0)).abs() < 1e-3);
    assert_eq!(t.tail[1], 700.0);
    // t = 0.9 -> fade (1 - 0.3/0.4) = 0.25
    m.age = 0.72;
    assert!((trail_of(&m).alpha - 0.9 * 0.25).abs() < 1e-6);
    // hit half-way through the collapse: length halves, envelope 0.75
    m.hit_at = Some(0.64);
    let t = trail_of(&m);
    assert!((t.alpha - 0.9 * 0.75).abs() < 1e-6);
    assert!((t.tail[0] - 350.0).abs() < 1e-3);

    let mut f = StarfallField::new(1000.0, 800.0, false, 5);
    f.stars.clear();
    f.spark(v(10.0, 20.0), Vec2::ZERO, 1.0, 10.0, 0.0, 0.5, false);
    f.spark(v(10.0, 20.0), Vec2::ZERO, 1.0, 10.0, 0.0, 0.0, true);
    for s in &mut f.sparks {
        s.age = 0.5;
    }
    let mut d = DrawList::new(1000.0, 800.0, 0.0);
    f.draw(Layer::Main, &mut d);
    let p = floats(&d, PassKind::Stars);
    // plain spark: size as is, angle 0, alpha 0.85 * (1 - 0.5)
    assert_eq!(&p[0..6], &[10.0, 20.0, 10.0, 0.0, 0.425, 0.5]);
    // flash: size * (0.6 + 0.4), angle 0.4, alpha 0.25
    assert_eq!(&p[6..12], &[10.0, 20.0, 10.0, 0.4, 0.25, 0.0]);
}

#[test]
fn falling_star_fade_and_twinkle_size() {
    let mut f = StarfallField::new(1000.0, 1000.0, false, 5);
    f.stars.clear();
    let mut s = star(v(100.0, 30.0), v(0.0, -30.0));
    s.twinkle = PI / 2.0; // tf = 1
    f.stars.push(s);
    let mut d = DrawList::new(1000.0, 1000.0, 0.0);
    f.draw(Layer::Main, &mut d);
    let p = floats(&d, PassKind::Stars);
    assert!((p[2] - 4.0 * 1.1).abs() < 1e-6);
    // fade = 30 / 60
    assert!((p[4] - 0.8 * 0.5).abs() < 1e-6);
}

#[test]
fn firefly_spawn_ranges_and_blink_schedule() {
    let mut f = FireflyField::new(1000.0, 1000.0, true, 6);
    f.set_share(5.0);
    for x in &f.flies {
        assert!((0.0..1000.0).contains(&x.home.x));
        assert!((20.0..=870.0).contains(&x.home.y));
        assert!((x.pos.x - x.home.x).abs() <= 60.0 && (x.pos.y - x.home.y).abs() <= 40.0);
        assert!((0.125..0.35).contains(&x.rate.0) && (0.1..0.3).contains(&x.rate.1));
        assert!((6.0..13.0).contains(&x.speed));
        assert!((0.0..5.0).contains(&x.blink_at));
        assert!((2.5..5.0).contains(&x.period));
        assert!((1.0..1.6).contains(&x.size));
        assert!((28.0..80.0).contains(&x.orbit_radius));
    }
    // later flies are scheduled from the current clock
    f.set_share(0.0);
    for _ in 0..100 {
        f.step(0.1, None);
    }
    f.set_share(5.0);
    assert!(f.flies[8..].iter().all(|x| x.blink_at >= f.clock - 1e-9));
    // reschedule: lit is evaluated before blink_at moves, by period * [0.8, 1.25)
    let clock = f.clock;
    let fl = &mut f.flies[0];
    fl.blink_at = clock - 3.0;
    fl.period = 2.5;
    let at = fl.blink_at;
    f.step(0.1, None);
    let fl = &f.flies[0];
    assert_eq!(fl.lit, blink(clock + 0.1, at));
    assert!(fl.blink_at >= at + 2.0 - 1e-9 && fl.blink_at < at + 3.125);
}

#[test]
fn meteor_launch_branches() {
    let mut f = StarfallField::new(1000.0, 800.0, false, 11);
    f.update(&[ledge(1, 100.0, 200.0, 300.0, 100.0)], 0.1, false);
    for _ in 0..30 {
        f.launch();
    }
    assert_eq!(f.meteors.len(), 30);
    assert!(f.meteors.iter().any(|m| m.bright));
    assert!(f.meteors.iter().any(|m| !m.bright));
    for rad in [-2.55, -0.6] {
        f.radiant = rad;
        f.meteors.clear();
        for _ in 0..30 {
            f.free_meteor();
            f.strike();
        }
        assert_eq!(f.meteors.len(), 60);
        assert!(f.meteors.iter().all(|m| (m.dir.x < 0.0) == (rad < -1.57)));
    }
}

#[test]
fn meteor_advance_paths() {
    let mut f = StarfallField::new(1000.0, 800.0, false, 5);
    f.stars.clear();
    f.update(&[ledge(1, 100.0, 200.0, 300.0, 100.0)], 0.1, false);
    // bright hit: flash + 30 sparks, then the trail collapses and the meteor is removed
    let mut m = meteor(v(200.0, 310.0), v(0.0, -1.0), true);
    assert!(f.advance_meteor(&mut m, 0.02));
    assert_eq!(m.hit_at, Some(0.02));
    assert_eq!(f.sparks.len(), 31);
    assert_eq!(m.head, v(200.0, 300.0));
    assert!(trail_of(&m).alpha > 0.0);
    assert!(f.advance_meteor(&mut m, 0.1));
    assert!(trail_of(&m).alpha > 0.0);
    assert!(!f.advance_meteor(&mut m, 0.1));
    assert_eq!(trail_of(&m).head[0], 200.0);
    // non-bright hit: flash + 18 sparks
    f.sparks.clear();
    let mut m = meteor(v(200.0, 310.0), v(0.0, -1.0), false);
    assert!(f.advance_meteor(&mut m, 0.02));
    assert_eq!(f.sparks.len(), 19);
    // near the cap: the flash is exempt and the count is clamped
    f.sparks.clear();
    for _ in 0..495 {
        f.spark(v(0.0, 0.0), v(0.0, 0.0), 1.0, 1.0, 0.0, 0.0, false);
    }
    let mut m = meteor(v(200.0, 310.0), v(0.0, -1.0), false);
    f.advance_meteor(&mut m, 0.02);
    assert_eq!(f.sparks.len(), 500);
    let mut m = meteor(v(200.0, 310.0), v(0.0, -1.0), false);
    f.advance_meteor(&mut m, 0.02);
    assert_eq!(f.sparks.len(), 501);
    // free flight: bright trail sparks, throttle, cap, end-of-life flash
    f.sparks.clear();
    let mut m = meteor(v(500.0, 700.0), v(1.0, 0.0), true);
    assert!(f.advance_meteor(&mut m, 0.03));
    assert_eq!(f.sparks.len(), 2);
    assert!(f.advance_meteor(&mut m, 0.005));
    assert_eq!(f.sparks.len(), 2);
    for _ in 0..400 {
        f.spark(v(0.0, 0.0), v(0.0, 0.0), 1.0, 1.0, 0.0, 0.0, false);
    }
    m.age = 0.5;
    assert!(f.advance_meteor(&mut m, 0.01));
    m.age = 0.79;
    let n = f.sparks.len();
    assert!(!f.advance_meteor(&mut m, 0.02));
    assert_eq!(f.sparks.len(), n + 1);
    let mut m = meteor(v(500.0, 700.0), v(1.0, 0.0), false);
    m.age = 0.79;
    assert!(!f.advance_meteor(&mut m, 0.02));
    assert_eq!(f.sparks.len(), n + 1);
    // trail envelope: ramp-up and fade-out
    let mut m = meteor(v(500.0, 700.0), v(1.0, 0.0), false);
    m.age = 0.05;
    assert!(trail_of(&m).alpha > 0.0);
    m.age = 0.7;
    assert!(trail_of(&m).alpha > 0.0);
}

#[test]
fn sparks_physics_and_render() {
    let mut f = StarfallField::new(1000.0, 800.0, false, 5);
    f.stars.clear();
    f.spark(v(10.0, 100.0), v(10.0, 0.0), 0.5, 3.0, 700.0, 0.5, false);
    f.spark(v(10.0, 100.0), v(10.0, 0.0), 0.5, 3.0, 0.0, 0.0, true);
    f.step_sparks(0.1);
    assert!(f.sparks[0].v.y < 0.0);
    assert!(f.sparks[0].pos.x > 10.0);
    let mut d = DrawList::new(1000.0, 800.0, 0.0);
    f.meteors.push(meteor(v(5.0, 5.0), v(1.0, 0.0), false));
    f.draw(Layer::Main, &mut d);
    assert_eq!(d.passes.len(), 2);
    assert_eq!(d.passes[0].kind, PassKind::Stars);
    assert_eq!(d.passes[0].instances, 2);
    assert_eq!(d.passes[1].kind, PassKind::StarTrails);
    f.step_sparks(1.0);
    assert!(f.sparks.is_empty());
    let mut d = DrawList::new(1000.0, 800.0, 0.0);
    f.draw(Layer::Above, &mut d);
    assert!(d.is_empty());
}

#[test]
fn fireflies_population_and_draw() {
    let mut f = FireflyField::new(1920.0, 1080.0, false, 4);
    assert_eq!(f.flies.len(), 87);
    assert_eq!(f.share(), 1.0);
    f.set_share(0.1);
    assert_eq!(f.flies.len(), 8);
    f.set_share(0.5);
    assert_eq!(f.flies.len(), 43);
    f.set_wind(Wind::StrongLeft);
    f.update(&[], 0.1, false);
    let mut d = DrawList::new(1920.0, 1080.0, 0.0);
    f.draw(Layer::Above, &mut d);
    assert!(d.is_empty());
    f.draw(Layer::Main, &mut d);
    assert_eq!(d.passes[0].kind, PassKind::Fireflies);
    assert_eq!(d.passes[0].instances, 43);
    let slow = FireflyField::new(100.0, 100.0, true, 1);
    assert_eq!(slow.slow, 0.5);
    assert_eq!(slow.flies.len(), 8);
    assert!(slow.flies.iter().all(|x| x.lit == 0.12 && x.v == Vec2::ZERO));
}

#[test]
fn blink_curve() {
    assert_eq!(blink(0.0, 1.0), 0.12);
    assert_eq!(blink(5.0, 1.0), 0.12);
    assert!((blink(1.0, 1.0) - 0.12).abs() < 1e-12);
    assert!((blink(1.0 + 0.66, 1.0) - 1.0).abs() < 1e-9);
    assert!(blink(1.0 + 1.5, 1.0) < 1.0);
    assert!(blink(1.0 + 0.3, 1.0) < blink(1.0 + 0.6, 1.0));
}

#[test]
fn firefly_motion_modes() {
    let mut f = FireflyField::new(800.0, 600.0, false, 2);
    f.step(0.0, None);
    assert_eq!(f.clock, 0.0);
    for _ in 0..600 {
        f.step(0.016, None);
    }
    assert!(f.flies.iter().all(|x| x.pos.x >= -20.0 && x.pos.x <= 820.0 && x.pos.y >= -20.0 && x.pos.y <= 620.0));
    assert!(f.flies.iter().any(|x| x.blink_at > 5.0));
    // gather: a still cursor for more than 1.2 s pulls flies into orbit
    let c = v(400.0, 300.0);
    for _ in 0..400 {
        f.step(0.016, Some(c));
    }
    let near = f.flies.iter().filter(|x| x.pos.dist(c) < 120.0).count();
    assert!(near > f.flies.len() / 2);
    // a fast sweep scatters nearby flies
    f.still_since = f.clock;
    for fl in &mut f.flies {
        fl.pos = c + v(50.0, 0.0);
        fl.v = Vec2::ZERO;
    }
    f.last_cursor = Some(c + v(-100.0, 0.0));
    f.step(0.1, Some(c));
    assert!(f.flies.iter().all(|x| x.v.x > 0.0));
    // a fly under the pointer is skipped (0.1 guard), a far one is out of reach
    f.flies[0].pos = c;
    f.flies[1].pos = c + v(300.0, 0.0);
    f.last_cursor = Some(c + v(-100.0, 0.0));
    f.step(0.1, Some(c));
    // in gather mode flies beyond the gather reach are not pulled
    let mut g = FireflyField::new(2000.0, 2000.0, false, 8);
    for _ in 0..200 {
        g.step(0.016, Some(v(0.0, 0.0)));
    }
    assert!(g.flies.iter().any(|x| x.pos.dist(v(0.0, 0.0)) > GATHER_REACH));
}

#[test]
fn changing_wind_follows_the_wall_clock() {
    use fp_core::falling::Falling;
    let run = |t: Option<f64>| {
        let mut f = StarfallField::new(800.0, 600.0, false, 7);
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
