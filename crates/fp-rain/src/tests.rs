use super::*;
use fp_core::geom::Rect4;

fn field(w: f64, h: f64) -> RainField {
    RainField::new(w, h, false, 7)
}

fn manual(x: f64, y: f64, vx: f64, vy: f64) -> Drop {
    Drop {
        pos: Vec2::new(x, y),
        vel: Vec2::new(vx, vy),
        width: 1.5,
        alpha: 0.4,
        drift: 1.0,
        lands: true,
        dripping: false,
    }
}

fn win(id: i64, x: f64, y: f64, w: f64, h: f64) -> Surface {
    Surface::new(SurfaceId::Window(id), Frame::new(x, y, w, h))
}

fn droplet(y: f64, vy: f64, floor: f64, age: f64) -> Droplet {
    Droplet { pos: Vec2::new(0.0, y), vel: Vec2::new(0.0, vy), width: 1.0, alpha: 0.5, floor, glow: 0.0, age }
}

#[test]
fn construction_and_accessors() {
    let mut f = field(1000.0, 1000.0);
    assert_eq!(f.drops.len(), 300);
    assert!(f.drops.iter().all(|d| d.pos.y >= 0.0 && d.pos.y < 1200.0));
    assert!((0.0..1000.0).contains(&(f.clock)));
    assert!(f.next_strike >= f.clock + 4.0 && f.next_strike < f.clock + 10.0);
    assert!(f.lightning);
    assert_eq!(f.share(), 1.0);
    f.set_share(0.5);
    assert_eq!(f.share(), 0.5);
    assert_eq!(f.target(), 150);
    f.set_wind(Wind::StrongLeft);
    f.set_lightning(false);
    assert!(!f.lightning);
    assert_eq!(f.slow, 1.0);
    let r = RainField::new(100.0, 100.0, true, 1);
    assert_eq!(r.slow, 0.5);
    assert!(!f.draws_above());
    assert!(!f.has_above());
}

#[test]
fn spawn_far_and_near() {
    let mut f = field(500.0, 500.0);
    let (mut far, mut near) = (0, 0);
    for _ in 0..3000 {
        let d = f.spawn_drop(10.0);
        assert!(d.vel.y < 0.0 && d.pos.x >= -60.0 && d.pos.x < 560.0);
        if d.lands {
            near += 1;
            assert!(d.width >= 0.7 && d.width <= 1.6 && d.alpha >= 0.16 && d.alpha <= 0.52);
            assert!(d.vel.y >= -1420.0 && d.vel.y <= -650.0);
        } else {
            far += 1;
            assert!(d.width >= 2.2 && d.width < 3.0 && d.alpha == 0.16 && d.drift == 1.2);
            assert!(d.vel.y <= -1500.0 && d.vel.y > -1800.0);
        }
    }
    assert!(far > 20 && near > 2800);
    let mut slow = RainField::new(500.0, 500.0, true, 3);
    for _ in 0..200 {
        let d = slow.spawn_drop(0.0);
        assert!(d.vel.y > -900.0);
    }
}

#[test]
fn step_ignores_non_positive_dt() {
    let mut f = field(400.0, 400.0);
    let c = f.clock;
    f.step(0.0, None);
    f.step(-1.0, None);
    f.step(f64::NAN, None);
    assert_eq!(f.clock, c);
    assert!(f.drops.iter().all(|d| d.pos.x.is_finite() && d.pos.y.is_finite()));
}

#[test]
fn long_run_keeps_population_and_is_deterministic() {
    let run = || {
        let mut f = field(800.0, 600.0);
        f.set_wind(Wind::StrongRight);
        for i in 0..1500 {
            let cur = if i % 3 == 0 { None } else { Some(Vec2::new(i as f64 % 800.0, 300.0)) };
            f.step(1.0 / 60.0, cur);
        }
        let mut out = DrawList::new(800.0, 600.0, 0.0);
        f.draw(Layer::Main, &mut out);
        (f.drops.len(), f.droplets.len(), f.ripples.len(), out)
    };
    let a = run();
    let b = run();
    assert_eq!(a, b);
    assert!(a.0 >= 100 && a.0 <= 180);
    assert!(a.3.passes.iter().any(|p| p.kind == PassKind::RainStrokes));
}

#[test]
fn blow_follows_wind_setting() {
    let mut f = field(100.0, 100.0);
    f.set_wind(Wind::StrongRight);
    for _ in 0..600 {
        f.step(1.0 / 60.0, None);
    }
    assert!(f.blow.speed > 100.0);
    // steady approaches 120 * strength 3
    assert!((f.blow.steady - 360.0).abs() < 1.0, "{}", f.blow.steady);
}

#[test]
fn wrap_both_sides() {
    let mut f = field(200.0, 100.0);
    f.set_share(0.0);
    f.drops = vec![manual(-59.0, 50.0, -3000.0, -1.0), manual(259.0, 50.0, 3000.0, -1.0)];
    f.drops.iter_mut().for_each(|d| d.lands = false);
    f.step(0.1, None);
    assert!(f.drops[0].pos.x > 0.0 && f.drops[0].pos.x < 100.0);
    assert!(f.drops[1].pos.x > 100.0 && f.drops[1].pos.x < 200.0);
}

#[test]
fn respawn_only_for_non_dripping() {
    let mut f = field(1000.0, 1000.0);
    let mut d = manual(10.0, -39.0, 0.0, -1000.0);
    d.lands = false;
    f.drops = vec![d.clone()];
    f.step(0.1, None);
    assert_eq!(f.drops.len(), 31);
    d.dripping = true;
    f.drops = vec![d];
    f.step(0.1, None);
    assert_eq!(f.drops.len(), 30);
    assert_eq!(f.drops.iter().filter(|d| d.dripping).count(), 0);
}

#[test]
fn dripping_motion_clamps_speed() {
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    let mut d = manual(500.0, 500.0, 0.0, -1290.0);
    d.dripping = true;
    f.drops = vec![d];
    f.step(0.1, None);
    assert_eq!(f.drops[0].vel.y, -1300.0);
    let mut r = RainField::new(1000.0, 1000.0, true, 1);
    r.set_share(0.0);
    let mut d = manual(500.0, 500.0, 0.0, -1290.0);
    d.dripping = true;
    r.drops = vec![d];
    r.step(0.1, None);
    assert_eq!(r.drops[0].vel.y, -650.0);
}

#[test]
fn ground_landing_splashes() {
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    f.drops = vec![manual(500.0, 20.0, 0.0, -3000.0)];
    f.step(0.01, None);
    assert!(f.drops.is_empty());
    assert!(!f.droplets.is_empty());
    assert!(f.droplets.iter().all(|d| d.floor == 1.0 && d.glow == 0.0 && d.alpha <= 0.7));
    assert_eq!(f.ripples.len(), 1);
    // a thin drop only rarely makes a ripple; the caps stop everything
    f.ripples = vec![Ripple { center: Vec2::ZERO, reach: 1.0, alpha: 1.0, life: 9.0, age: 0.0 }; MAX_RIPPLES];
    f.droplets = vec![droplet(0.0, 0.0, -5.0, 0.0); MAX_DROPLETS];
    f.drops = vec![manual(500.0, 20.0, 0.0, -1000.0)];
    f.step(0.1, None);
    assert_eq!(f.ripples.len(), MAX_RIPPLES);
    assert!(f.droplets.len() <= MAX_DROPLETS);
    let mut thin = manual(500.0, 20.0, 0.0, -1000.0);
    thin.width = 0.8;
    let mut with_ripple = 0;
    for _ in 0..200 {
        f.ripples.clear();
        f.drops = vec![thin.clone()];
        f.step(0.1, None);
        with_ripple += f.ripples.len();
    }
    // Unit() < 0.4 for thin drops
    assert!((40..120).contains(&with_ripple), "{with_ripple}");
}

#[test]
fn splash_droplet_shape() {
    let mut f = field(1000.0, 1000.0);
    let d = Drop {
        pos: Vec2::new(500.0, 2.0),
        vel: Vec2::new(-200.0, -1300.0),
        width: 1.2,
        alpha: 0.6,
        drift: 1.0,
        lands: true,
        dripping: false,
    };
    let power = (d.vel.len() / 1300.0).min(1.3) * (0.4 + 1.2 * 0.45);
    for pointer in [false, true] {
        let per = if pointer { 3.5 } else { 2.5 };
        let hit = Hit { pos: Vec2::new(500.0, 3.0), ledge: None, pointer };
        f.droplets.clear();
        f.ripples.clear();
        f.splash(&hit, &d);
        let n = f.droplets.len() as f64;
        assert!(n >= (power * per).floor() && n <= (power * per).floor() + 1.0);
        for p in &f.droplets {
            assert_eq!(p.pos, Vec2::new(500.0, 4.0));
            assert!((p.alpha - 0.7).abs() < 1e-12 && p.glow == 0.0 && p.age == 0.0);
            assert!(p.width >= 0.84 - 1e-9 && p.width <= 1.32 + 1e-9);
            assert_eq!(p.floor, if pointer { -27.0 } else { 1.0 });
            // upward fan, speed rand(70,200)*power, x carries 15% of the drop
            let vx = p.vel.x - d.vel.x * 0.15;
            let speed = (vx * vx + (p.vel.y / 1.1).powi(2)).sqrt();
            assert!(p.vel.y > 0.0 && speed >= 70.0 * power - 1e-6 && speed <= 200.0 * power + 1e-6);
            let a = (p.vel.y / 1.1).atan2(vx);
            let (lo, hi) = if pointer { (0.2, 2.9416) } else { (0.35, 2.7916) };
            assert!(a >= lo - 1e-9 && a <= hi + 1e-9);
        }
        if pointer {
            assert!(f.ripples.is_empty());
        } else {
            let r = &f.ripples[0];
            assert_eq!(r.center, hit.pos);
            assert!(r.reach >= 3.0 + 1.2 * 3.5 && r.reach < 3.0 + 1.2 * 3.5 + 2.0);
            assert!((r.alpha - 0.66).abs() < 1e-12 && r.life >= 0.25 && r.life < 0.45);
        }
    }
}

#[test]
fn ambient_wind_and_drop_steering() {
    let t = 12.5f64;
    let want = -70.0
        + 45.0 * (0.05 * t).sin()
        + 25.0 * (0.17 * t + 1.3).sin()
        + 12.0 * (0.9 * t + 0.4).sin() * (0.11 * t + 2.2).sin().max(0.0);
    assert_eq!(ambient_wind(t), want);
    assert_eq!(ambient_wind(0.0), -70.0 + 25.0 * 1.3f64.sin() + 12.0 * 0.4f64.sin() * 2.2f64.sin().max(0.0));
    // no user wind: blow stays 0, so wind = ambient * slow
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    f.drops = vec![manual(500.0, 500.0, 0.0, -100.0)];
    f.step(0.1, None);
    let w = ambient_wind(f.clock);
    let d = &f.drops[0];
    assert!((d.vel.x - w * 0.15).abs() < 1e-9);
    assert_eq!(d.vel.y, -100.0);
    assert!((d.pos.x - (500.0 + w * 0.15 * 0.1)).abs() < 1e-9);
    let mut r = RainField::new(1000.0, 1000.0, true, 2);
    r.set_share(0.0);
    let mut drip = manual(500.0, 500.0, 0.0, 0.0);
    drip.dripping = true;
    drip.drift = 0.6;
    r.drops = vec![drip];
    r.step(0.1, None);
    let w = ambient_wind(r.clock) * 0.5;
    assert!((r.drops[0].vel.x - w * 0.6 * 0.3 * 0.2).abs() < 1e-9);
    assert!((r.drops[0].vel.y + 220.0).abs() < 1e-9);
}

#[test]
fn top_up_and_respawn_heights() {
    let mut f = field(1000.0, 1000.0);
    f.drops.clear();
    f.top_up();
    assert_eq!(f.drops.len(), SPAWN_BURST);
    assert!(f.drops.iter().all(|d| d.pos.y >= 1000.0 && d.pos.y < 1200.0));
    f.drops.clear();
    let mut d = manual(10.0, -39.0, 0.0, -1000.0);
    d.lands = false;
    f.drops = vec![d];
    f.advance_drops(0.1, None);
    assert_eq!(f.drops.len(), 1);
    assert!(f.drops[0].pos.y >= 1000.0 && f.drops[0].pos.y < 1060.0);
    // full population: no respawn
    f.set_share(0.0);
    let mut d = manual(10.0, -39.0, 0.0, -1000.0);
    d.lands = false;
    f.drops = vec![d];
    f.advance_drops(0.1, None);
    assert!(f.drops.is_empty());
}

#[test]
fn pointer_collider() {
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    let cur = Some(Vec2::new(100.0, 100.0));
    f.drops = vec![manual(105.0, 110.0, 0.0, -3000.0)];
    f.step(0.01, cur);
    assert!(f.drops.is_empty());
    assert!(!f.droplets.is_empty());
    assert!(f.droplets.iter().all(|d| (d.floor - 71.0).abs() < 0.5));
    assert!(f.ripples.is_empty());
    // inside the cap but not crossing it: no pointer hit, falls on
    f.droplets.clear();
    f.drops = vec![manual(105.0, 95.0, 0.0, -10.0)];
    f.step(0.01, cur);
    assert_eq!(f.drops.len(), 1);
}

#[test]
fn land_priorities_and_ledge_rules() {
    let mut f = field(1000.0, 1000.0);
    let a = win(1, 100.0, 0.0, 200.0, 50.0);
    let b = win(2, 100.0, 0.0, 200.0, 80.0);
    f.update(&[a.clone(), b.clone()], 0.1, false);
    let h = f.land(Vec2::new(150.0, 200.0), Vec2::new(150.0, -10.0), None).unwrap();
    assert_eq!((h.pos.y, h.ledge, h.pointer), (80.0, Some(SurfaceId::Window(2)), false));
    f.update(&[b.clone(), a.clone()], 0.1, false);
    let h = f.land(Vec2::new(150.0, 200.0), Vec2::new(150.0, -10.0), None).unwrap();
    assert_eq!(h.pos.y, 80.0);
    // crossing only the lower one
    let h = f.land(Vec2::new(150.0, 60.0), Vec2::new(150.0, -10.0), None).unwrap();
    assert_eq!(h.pos.y, 50.0);
    // slanted crossing interpolates x
    let h = f.land(Vec2::new(100.0, 90.0), Vec2::new(200.0, 70.0), None).unwrap();
    assert!((h.pos.x - 150.0).abs() < 1e-9 && h.pos.y == 80.0);
    // blocked top lets the drop fall to the lower ledge
    let mut blocked_b = b.clone();
    blocked_b.blockers.push(Rect4::new(0.0, 70.0, 1000.0, 90.0));
    f.update(&[blocked_b, a.clone()], 0.1, false);
    let h = f.land(Vec2::new(150.0, 200.0), Vec2::new(150.0, -10.0), None).unwrap();
    assert_eq!(h.pos.y, 50.0);
    // corner inset 0.4 * radius keeps the rim free
    let mut round = win(3, 100.0, 0.0, 200.0, 50.0);
    round.corner_radius = 25.0;
    f.update(&[round], 0.1, false);
    assert!(f.land(Vec2::new(105.0, 200.0), Vec2::new(105.0, 30.0), None).is_none());
    assert!(f.land(Vec2::new(150.0, 200.0), Vec2::new(150.0, 30.0), None).is_some());
    // above the ledge, nothing
    assert!(f.land(Vec2::new(150.0, 200.0), Vec2::new(150.0, 100.0), None).is_none());
    // ground
    let g = f.land(Vec2::new(600.0, 10.0), Vec2::new(601.0, 2.0), None).unwrap();
    assert_eq!((g.pos.x, g.pos.y, g.ledge), (601.0, 3.0, None));
}

#[test]
fn cross_x_flat_segment() {
    assert_eq!(cross_x(Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0), 5.0), 10.0);
    assert_eq!(cross_x(Vec2::new(0.0, 10.0), Vec2::new(10.0, 0.0), 5.0), 5.0);
}

#[test]
fn ledge_water_drips_and_caps() {
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    let mut s = win(1, 100.0, 0.0, 200.0, 50.0);
    s.corner_radius = 10.0;
    f.update(&[s], 0.1, false);
    f.ledges.get_mut(&SurfaceId::Window(1)).unwrap().drip_at = 0.1;
    f.drops = vec![manual(120.0, 60.0, 0.0, -1000.0)];
    f.step(0.1, None);
    let drip = f.drops.iter().find(|d| d.dripping).unwrap();
    assert_eq!(drip.pos.x, 99.5);
    assert_eq!(drip.pos.y, 50.0 - 6.0);
    assert!(drip.vel.x < 0.0 && drip.alpha == 0.5 && drip.lands);
    let l = &f.ledges[&SurfaceId::Window(1)];
    assert!(l.drip_at >= 3.0 && l.drip_at < 9.0);
    // right side
    f.ledges.get_mut(&SurfaceId::Window(1)).unwrap().drip_at = 0.1;
    f.drops = vec![manual(280.0, 60.0, 0.0, -1000.0)];
    f.step(0.1, None);
    let drip = f.drops.iter().find(|d| d.dripping).unwrap();
    assert_eq!(drip.pos.x, 300.5);
    assert!(drip.vel.x > 0.0);
    // cap
    let l = f.ledges.get_mut(&SurfaceId::Window(1)).unwrap();
    l.water = 80.0;
    l.drip_at = 1000.0;
    f.drops = vec![manual(280.0, 60.0, 0.0, -1000.0)];
    f.step(0.1, None);
    assert_eq!(f.ledges[&SurfaceId::Window(1)].water, 40.0);
    // water without a ledge record is ignored
    f.ledges.clear();
    f.drops = vec![manual(280.0, 60.0, 0.0, -1000.0)];
    f.step(0.1, None);
    assert!(f.ledges.is_empty());
}

#[test]
fn drip_amount_widens_drop() {
    let mut f = field(1000.0, 1000.0);
    f.update(&[win(1, 0.0, 0.0, 500.0, 50.0)], 0.1, false);
    let l = f.ledges.get_mut(&SurfaceId::Window(1)).unwrap();
    l.water = 30.0;
    l.drip_at = 1.0;
    let hit = Hit { pos: Vec2::new(100.0, 50.0), ledge: Some(SurfaceId::Window(1)), pointer: false };
    f.splash(&hit, &manual(100.0, 60.0, 0.0, -100.0));
    let d = f.drops.iter().find(|d| d.dripping).unwrap();
    assert_eq!(d.width, 2.6);
}

#[test]
fn stir_displaces_nearby_drops() {
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    f.drops = vec![manual(120.0, 100.0, 0.0, 0.0), manual(300.0, 100.0, 0.0, 0.0)];
    // no previous cursor: no velocity
    f.stir(0.1, Some(Vec2::new(100.0, 100.0)));
    assert_eq!(f.drops[0].pos.x, 120.0);
    f.stir(0.1, Some(Vec2::new(140.0, 100.0)));
    // vel 400, dist 20 -> 400 * 0.75 * 0.5 * 0.1
    assert!((f.drops[0].pos.x - 135.0).abs() < 1e-9);
    assert_eq!(f.drops[1].pos.x, 300.0);
    // slow pointer: nothing; absent pointer is stored
    f.stir(0.1, Some(Vec2::new(141.0, 100.0)));
    assert!((f.drops[0].pos.x - 135.0).abs() < 1e-9);
    f.stir(0.1, None);
    assert_eq!(f.last_cursor, None);
}

#[test]
fn particles_expire() {
    let mut f = field(1000.0, 1000.0);
    f.droplets = vec![
        droplet(100.0, 10.0, 0.0, 0.0),     // rising: keep
        droplet(100.0, -10.0, 0.0, 0.0),    // above floor: keep
        droplet(-5.0, -500.0, 0.0, 0.0),    // below floor falling: drop
        droplet(100.0, 10.0, 0.0, 0.95),    // too old
        droplet(-9.0, 5000.0, -500.0, 0.0), // y < -10 after step? rising fast keeps
        droplet(-9.9, -1.0, -500.0, 0.0),   // falls under -10
    ];
    f.ripples = vec![
        Ripple { center: Vec2::ZERO, reach: 5.0, alpha: 1.0, life: 0.3, age: 0.0 },
        Ripple { center: Vec2::ZERO, reach: 5.0, alpha: 1.0, life: 0.1, age: 0.0 },
    ];
    f.advance_particles(0.1);
    assert_eq!(f.droplets.len(), 3);
    assert_eq!(f.ripples.len(), 1);
}

#[test]
fn brightness_and_flash() {
    let mut s = Strike { pulses: vec![0.0, 0.1], age: 0.0, life: 0.6, strength: 0.11, segs: vec![] };
    assert_eq!(s.brightness(), 1.0);
    s.age = 0.05;
    let b = (-0.05f64 * 11.0).exp();
    assert!((s.brightness() - b).abs() < 1e-12);
    s.age = 0.2;
    let b = (-0.2f64 * 11.0).exp() + 0.75 * (-0.1f64 * 11.0).exp();
    assert!((s.brightness() - b).abs() < 1e-12);
    let mut f = field(100.0, 100.0);
    assert_eq!(f.flash(), 0.0);
    s.age = 0.0;
    f.strikes = vec![s.clone(), Strike { strength: 0.06, ..s }];
    assert!((f.flash() - 0.11).abs() < 1e-12);
}

#[test]
fn scheduler_and_lightning_flag() {
    let mut f = field(400.0, 400.0);
    f.set_lightning(false);
    f.clock = 100.0;
    f.next_strike = 0.0;
    f.advance_strikes(0.1);
    assert!(f.strikes.is_empty());
    assert_eq!(f.next_strike, 0.0);
    f.set_lightning(true);
    let mut quick = false;
    for _ in 0..60 {
        f.strikes.clear();
        f.next_strike = 0.0;
        f.advance_strikes(0.1);
        assert_eq!(f.strikes.len(), 1);
        let gap = f.next_strike - f.clock;
        assert!((0.8..26.0).contains(&gap));
        quick |= gap < 1.8;
    }
    assert!(quick);
    // aged strikes are removed
    f.next_strike = 1e9;
    f.strikes = vec![Strike { pulses: vec![0.0], age: 0.45, life: 0.5, strength: 0.1, segs: vec![] }];
    f.advance_strikes(0.1);
    assert!(f.strikes.is_empty());
}

#[test]
fn bolt_kinds_and_pulses() {
    for reduce in [false, true] {
        let mut f = RainField::new(1000.0, 800.0, reduce, 11);
        let (mut sheet, mut visible, mut three, mut two) = (false, false, false, false);
        for _ in 0..200 {
            f.droplets.clear();
            f.ripples.clear();
            f.bolt();
            let s = f.strikes.last().unwrap();
            let last = *s.pulses.last().unwrap();
            assert_eq!(s.pulses[0], 0.0);
            assert!((s.life - (last + 0.5)).abs() < 1e-12);
            if reduce {
                assert_eq!(s.pulses, vec![0.0]);
            }
            match (s.segs.is_empty(), reduce) {
                (true, false) => {
                    sheet = true;
                    assert_eq!(s.strength, 0.06);
                }
                (true, true) => assert_eq!(s.strength, 0.02),
                (false, false) => {
                    visible = true;
                    assert_eq!(s.strength, 0.11);
                }
                (false, true) => {
                    visible = true;
                    assert_eq!(s.strength, 0.03);
                }
            }
            three |= s.pulses.len() == 3;
            two |= s.pulses.len() == 2;
        }
        assert!(visible);
        if !reduce {
            assert!(sheet && three && two);
        }
    }
}

#[test]
fn bolt_geometry() {
    let mut f = field(1000.0, 800.0);
    f.surfaces = vec![win(1, -5000.0, 0.0, 10000.0, 100.0)];
    let segs = f.bolt_segments();
    assert!(segs.len() >= 64 + 2 * 16 && segs.len() <= 64 + 4 * 16);
    assert_eq!(segs[0].width, 2.4);
    assert!((segs[63].width - (2.4 - 63.0 / 65.0)).abs() < 1e-12);
    assert_eq!(segs[0].a.y, 810.0);
    assert_eq!(segs[63].b.y, 100.0);
    assert!(segs[..64].windows(2).all(|w| w[0].b == w[1].a));
    assert_eq!(segs[64].width, 1.2);
    assert_eq!(segs[64].alpha, 0.55);
    assert_eq!(f.droplets.len(), 18);
    assert!(f.droplets.iter().all(|d| d.glow == 1.0 && d.alpha == 0.9 && d.floor == 60.0));
    let r = f.ripples.last().unwrap();
    assert_eq!((r.center.y, r.reach, r.alpha, r.life), (100.0, 26.0, 0.6, 0.6));
    // caps
    f.droplets = vec![droplet(0.0, 0.0, -5.0, 0.0); MAX_DROPLETS - 3];
    f.ripples = vec![Ripple { center: Vec2::ZERO, reach: 1.0, alpha: 1.0, life: 9.0, age: 0.0 }; MAX_RIPPLES];
    f.bolt_segments();
    assert_eq!(f.droplets.len(), MAX_DROPLETS);
    assert_eq!(f.ripples.len(), MAX_RIPPLES);
}

#[test]
fn jagged_is_pinned_and_guarded() {
    let mut rng = Lcg64::new(5);
    let pts = jagged(&mut rng, Vec2::new(0.0, 100.0), Vec2::new(0.0, 0.0), 6, 0.2);
    assert_eq!(pts.len(), 65);
    assert_eq!((pts[0], pts[64]), (Vec2::new(0.0, 100.0), Vec2::ZERO));
    let flat = jagged(&mut rng, Vec2::new(3.0, 3.0), Vec2::new(3.0, 3.0), 4, 0.25);
    assert_eq!(flat.len(), 17);
    assert!(flat.iter().all(|p| p.x == 3.0 && p.y == 3.0));
}

#[test]
fn update_creates_pours_and_flings() {
    let mut f = field(1000.0, 1000.0);
    f.set_share(0.0);
    let s = win(1, 100.0, 0.0, 200.0, 50.0);
    f.update(std::slice::from_ref(&s), 0.1, false);
    assert!(f.ledges.contains_key(&SurfaceId::Window(1)));
    let drip_at = f.ledges[&SurfaceId::Window(1)].drip_at;
    assert!((3.0..9.0).contains(&drip_at));
    assert_eq!(f.surfaces.len(), 1);

    // too little water, wrong size, too slow, too fast: nothing flies
    let id = SurfaceId::Window(1);
    f.ledges.get_mut(&id).unwrap().water = 0.4;
    f.update(&[win(1, 120.0, 0.0, 200.0, 50.0)], 0.01, false);
    assert!(f.droplets.is_empty());
    f.ledges.get_mut(&id).unwrap().water = 5.0;
    f.update(&[win(1, 140.0, 0.0, 201.0, 50.0)], 0.01, false);
    assert!(f.droplets.is_empty());
    f.update(&[win(1, 140.5, 0.0, 201.0, 50.0)], 1.0, false);
    assert!(f.droplets.is_empty());
    f.update(&[win(1, 10140.0, 0.0, 201.0, 50.0)], 0.01, false);
    assert!(f.droplets.is_empty());

    // a real fling: speed 2000 -> k = 1, water 5 -> 8 droplets, water emptied
    f.update(&[win(1, 10140.0, 0.0, 201.0, 50.0)], 0.01, false);
    f.update(&[win(1, 10160.0, 0.0, 201.0, 50.0)], 0.01, false);
    assert_eq!(f.droplets.len(), 8);
    assert!(f.droplets.iter().all(|d| d.floor == -10.0 && d.alpha == 0.55 && d.pos.y == 51.0));
    assert!(f.ledges[&id].water.abs() < 1e-9);

    // partial k, and the 40 / room caps
    f.droplets.clear();
    f.ledges.get_mut(&id).unwrap().water = 100.0;
    f.update(&[win(1, 10160.0, 0.0, 201.0, 50.0)], 1.0, false);
    f.update(&[win(1, 10160.0 + 12.0, 0.0, 201.0, 50.0)], 0.01, false); // speed 1200
    assert_eq!(f.droplets.len(), 40);
    let k = (1200.0f64 - 450.0) / 1500.0 + 0.3;
    assert!((f.ledges[&id].water - 100.0 * (1.0 - k)).abs() < 1e-6);
    f.ledges.get_mut(&id).unwrap().water = 100.0;
    f.droplets = vec![droplet(0.0, 0.0, -5.0, 0.0); MAX_DROPLETS - 2];
    f.update(&[win(1, 10160.0 + 36.0, 0.0, 201.0, 50.0)], 0.01, false);
    assert_eq!(f.droplets.len(), MAX_DROPLETS);

    // pour on removal (not when the window went to another space)
    f.ledges.get_mut(&id).unwrap().water = 100.0;
    f.drops.clear();
    f.update(&[], 0.1, true);
    assert!(f.drops.is_empty() && f.ledges.is_empty());
    f.update(std::slice::from_ref(&s), 0.1, false);
    f.ledges.get_mut(&id).unwrap().water = 10.0;
    f.update(&[], 0.1, false);
    assert_eq!(f.drops.len(), 5);
    assert!(f.drops.iter().all(|d| d.dripping && d.alpha == 0.5 && d.pos.x >= 100.0 && d.pos.x < 300.0));
    f.drops.clear();
    f.update(&[s], 0.1, false);
    f.ledges.get_mut(&id).unwrap().water = 100.0;
    f.update(&[], 0.1, false);
    assert_eq!(f.drops.len(), 14);
}

#[test]
fn draw_layers_and_passes() {
    let mut f = field(1000.0, 600.0);
    f.set_share(0.0);
    f.drops =
        vec![manual(100.0, 100.0, 0.0, 0.0), manual(200.0, 100.0, 0.0, -1000.0), manual(300.0, 100.0, 0.0, -10.0)];
    f.droplets = vec![
        Droplet {
            pos: Vec2::new(10.0, 10.0),
            vel: Vec2::new(100.0, 100.0),
            width: 1.0,
            alpha: 0.5,
            floor: 0.0,
            glow: 1.0,
            age: 0.45,
        },
        Droplet {
            pos: Vec2::new(10.0, 10.0),
            vel: Vec2::new(0.1, 0.1),
            width: 1.0,
            alpha: 0.5,
            floor: 0.0,
            glow: 0.0,
            age: 2.0,
        },
    ];
    f.ripples = vec![Ripple { center: Vec2::new(5.0, 6.0), reach: 10.0, alpha: 0.5, life: 0.4, age: 0.1 }];
    f.strikes = vec![Strike {
        pulses: vec![0.0],
        age: 0.0,
        life: 0.5,
        strength: 0.11,
        segs: vec![Seg { a: Vec2::new(0.0, 10.0), b: Vec2::new(0.0, 0.0), width: 2.0, alpha: 0.5 }],
    }];
    let mut above = DrawList::new(1.0, 1.0, 0.0);
    f.draw(Layer::Above, &mut above);
    assert!(above.is_empty());
    let mut out = DrawList::new(1.0, 1.0, 0.0);
    f.draw(Layer::Main, &mut out);
    assert_eq!(out.scene[0], 1000.0);
    assert_eq!(out.scene[1], 600.0);
    let kinds: Vec<_> = out.passes.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, vec![PassKind::RainFlash, PassKind::RainRipples, PassKind::RainStrokes]);
    assert_eq!(out.passes[0].uniforms, vec![0.11, 600.0, 0.0, 0.0]);
    assert_eq!(out.passes[0].vertices, 4);
    let ripple: &[RainRipple] = bytemuck::cast_slice(&out.passes[1].buffers[0]);
    assert!((ripple[0].radius - 10.0 * (0.25 + 0.75 * 0.25f32.sqrt())).abs() < 1e-5);
    assert!((ripple[0].alpha - 0.5 * 0.75 * 0.75).abs() < 1e-6);
    let strokes: &[RainStroke] = bytemuck::cast_slice(&out.passes[2].buffers[0]);
    assert_eq!(strokes.len(), 3 + 2 + 2);
    // stationary drop points straight down with the minimum streak
    assert_eq!(strokes[0].tail, [100.0, 100.0 + 2.25]);
    assert_eq!(strokes[1].tail, [200.0, 100.0 + 22.0]);
    assert_eq!(strokes[2].tail[1], 100.0 + 2.25);
    assert!((strokes[3].alpha - 0.25).abs() < 1e-6 && strokes[3].glow == 1.0);
    assert_eq!(strokes[4].alpha, 0.0);
    assert_eq!(strokes[4].tail, [10.0, 10.0 - 0.01]);
    assert_eq!((strokes[5].width, strokes[5].glow), (14.0, 2.0));
    assert!((strokes[5].alpha - 0.2 * 0.5 * 0.11 / 0.11).abs() < 1e-6);
    assert_eq!((strokes[6].width, strokes[6].glow, strokes[6].alpha), (2.0, 1.0, 0.5));
    // nothing to draw: empty list
    let mut idle = field(100.0, 100.0);
    idle.set_share(0.0);
    idle.drops.clear();
    let mut e = DrawList::new(1.0, 1.0, 0.0);
    idle.draw(Layer::Main, &mut e);
    assert!(e.is_empty());
}

#[test]
fn changing_wind_follows_the_wall_clock() {
    use fp_core::falling::Falling;
    let run = |t: Option<f64>| {
        let mut f = RainField::new(800.0, 600.0, false, 7);
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
