//! Deterministic tests: Lcg64 seeds, fixed `dt`, no OS calls.

use super::*;
use crate::drift::{Drift, columns_for};
use fp_core::feed::{Blade, Burst};
use fp_core::geom::{Frame, Rect4};

const DT: f64 = 1.0 / 60.0;

fn cfg(seed: u64) -> SnowConfig {
    SnowConfig { width: 800.0, height: 600.0, reduce_motion: false, origin: Vec2::new(100.0, 50.0), seed }
}

fn field(seed: u64) -> SnowField {
    SnowField::new(cfg(seed), None)
}

fn feed_field(seed: u64) -> (SnowField, Arc<SnowFeed>) {
    let feed = Arc::new(SnowFeed::new());
    (SnowField::new(cfg(seed), Some(feed.clone())), feed)
}

fn surf(id: i64, x: f64, y: f64, w: f64, h: f64) -> Surface {
    Surface::new(SurfaceId::from_raw(id), Frame::new(x, y, w, h))
}

fn run(f: &mut SnowField, secs: f64) {
    for _ in 0..(secs / DT) as usize {
        f.step(DT, None);
    }
}

fn pile(f: &mut SnowField, id: SurfaceId, h: f64) {
    let d = f.drift_mut(id).unwrap();
    d.heights.fill(h);
    d.refresh_top();
}

fn total(d: &Drift) -> f64 {
    d.heights.iter().sum::<f64>() * 2.0
}

fn rng(seed: u64) -> Lcg64 {
    Lcg64::new(seed)
}

fn lone(width: f64, corner: f64, opens: bool) -> Drift {
    Drift::new(Vec2::new(10.0, 100.0), width, 14.0, corner, opens, &mut rng(7))
}

fn grain(x: f64, y: f64, vx: f64, vy: f64, area: f64, size: f64, lump: f64) -> Grain {
    Grain { pos: Vec2::new(x, y), vel: Vec2::new(vx, vy), area, size, lump }
}

fn old_puff(life: f64) -> Puff {
    Puff { pos: Vec2::ZERO, vel: Vec2::ZERO, size: 1.0, alpha: 0.1, life, age: 0.0 }
}

// ---- field basics ----

#[test]
fn flake_target_matches_spec() {
    let f = SnowField::new(SnowConfig { width: 1920.0, height: 1080.0, ..cfg(1) }, None);
    assert_eq!(f.flakes.len(), 984);
    assert_eq!(f.share(), 1.0);
    assert!(f.draws_above());
}

#[test]
fn share_and_flags() {
    let mut f = field(2);
    f.set_share(-3.0);
    assert_eq!(f.share(), 0.0);
    f.set_share(0.5);
    assert_eq!(f.share(), 0.5);
    f.set_wind(Wind::StrongLeft);
    f.show_ground(false);
    f.keep_clear(true);
    f.set_split(true);
    assert!(!f.ground_shown && f.keep_clear && f.split);
    assert!(!f.has_above());
}

#[test]
fn spawn_ranges_and_reduce_motion() {
    for reduce in [false, true] {
        let mut r = rng(3);
        let mut bigs = 0;
        for _ in 0..2000 {
            let fl = Flake::spawn(&mut r, 800.0, 600.0, 100.0, 5.0, 0.0, reduce);
            assert!(fl.x >= -30.0 && fl.x <= 830.0);
            assert!(fl.top >= 600.0 && fl.top <= 700.0);
            if fl.settles {
                assert!(fl.size >= 1.4 && fl.size <= 6.8);
            } else {
                bigs += 1;
                assert!(fl.size >= 8.0 && fl.size <= 13.0 && fl.alpha == 0.28);
            }
            if reduce {
                assert!(fl.fall <= 100.0 * 0.6 + 1e-9);
            }
        }
        assert!(bigs > 5 && bigs < 100);
    }
}

#[test]
fn flake_position_wraps_and_falls() {
    let mut r = rng(4);
    let fl = Flake::spawn(&mut r, 800.0, 600.0, 0.0, 0.0, 0.0, false);
    let span = 860.0;
    assert!(fl.position(2.0, 0.0, span).y < fl.position(1.0, 0.0, span).y);
    let far = fl.position(0.0, -1e5, span);
    assert!(far.x >= -30.0 && far.x < 830.0);
    let g = fl.to_gpu();
    assert_eq!(g.gone, f32::MAX);
    assert_eq!(g.top, fl.top as f32);
}

#[test]
fn rebase_keeps_wind_continuous() {
    let mut f = field(5);
    f.set_wind(Wind::StrongRight);
    f.flakes[3].landing = Some((SurfaceId::Ground, 5.0));
    f.flakes[3].gone = 10.0;
    f.clock += 1300.0;
    f.blow.carried = 77.0;
    let w_before = f.wind_now();
    f.rebase();
    assert_eq!(f.blow.carried, 0.0);
    assert!(f.now() < 600.0);
    assert!(f.flakes[3].gone < 0.0);
    let d_after = f.flakes[0].drift * (f.wind_now() - f.flakes[0].wind_at_birth);
    let d_before = f.flakes[0].drift * (w_before - (f.flakes[0].wind_at_birth + 77.0));
    assert!((d_after - d_before).abs() < 1e-6);
}

#[test]
fn step_rebases_after_600_seconds() {
    let mut f = field(6);
    let e = f.epoch;
    f.step(601.0, None);
    assert!(f.epoch > e);
}

#[test]
fn snow_accumulates_on_ground_and_is_reported() {
    let (mut f, feed) = feed_field(7);
    run(&mut f, 40.0);
    assert!(f.ground.top > 0.1, "ground collects snow");
    assert!(f.ground_depth() > 0.0);
    assert_eq!(feed.depth(f.cfg.origin), Some(f.ground_depth()));
    assert!(f.flakes.len() as f64 > f.target() as f64 * 0.8);
}

#[test]
fn ground_depth_prefers_dock_and_zero_when_empty() {
    let mut f = field(8);
    assert_eq!(f.ground_depth(), 0.0);
    f.update(&[surf(0, 0.0, 0.0, 800.0, 40.0)], 0.016, false);
    pile(&mut f, SurfaceId::Dock, 3.0);
    pile(&mut f, SurfaceId::Ground, 9.0);
    assert!((f.ground_depth() - 3.0).abs() < 1e-9);
}

#[test]
fn hidden_ground_lets_flakes_fall_through() {
    let mut f = field(9);
    f.show_ground(false);
    run(&mut f, 20.0);
    assert!(f.ground.top < 0.05);
}

#[test]
fn keep_clear_only_thin_ground_and_melts() {
    let mut f = field(10);
    f.keep_clear(true);
    run(&mut f, 30.0);
    assert!(f.ground.top < 8.0);
    pile(&mut f, SurfaceId::Ground, 20.0);
    run(&mut f, 60.0);
    assert!(f.ground.top < 10.0);
}

#[test]
fn flakes_land_on_a_window_and_ledge_fills() {
    let mut f = field(11);
    let w = surf(5, 200.0, 200.0, 300.0, 150.0);
    f.update(std::slice::from_ref(&w), 0.016, false);
    for _ in 0..4 {
        run(&mut f, 15.0);
        f.update(std::slice::from_ref(&w), 0.016, false);
    }
    assert!(f.drift(SurfaceId::Window(5)).unwrap().top > 0.05);
    assert!(f.has_above());
}

#[test]
fn keep_clear_deposit_rejects_ledges() {
    let mut f = field(12);
    f.keep_clear(true);
    f.update(&[surf(5, 200.0, 200.0, 300.0, 150.0)], 0.016, false);
    run(&mut f, 30.0);
    assert!(f.drift(SurfaceId::Window(5)).unwrap().top < 0.05);
}

#[test]
fn scan_tops_up_missing_flakes() {
    let mut f = field(16);
    f.flakes.truncate(10);
    f.last_check = f.clock - 1.0;
    f.step(DT, None);
    assert!(f.flakes.len() >= 30);
}

#[test]
fn stale_landing_on_vanished_drift_is_dropped() {
    let mut f = field(13);
    f.flakes[0].landing = Some((SurfaceId::Window(99), 10.0));
    f.flakes[0].gone = 0.0;
    f.last_check = f.clock - 1.0;
    let n = f.flakes.len();
    f.step(DT, None);
    assert!(f.flakes.len() <= n);
}

#[test]
fn big_flakes_never_land_and_are_renewed() {
    let mut f = field(14);
    f.flakes.clear();
    let mut r = rng(14);
    let mut b = Flake::spawn(&mut r, 800.0, 600.0, 0.0, f.now(), 0.0, false);
    b.settles = false;
    b.top = -500.0;
    b.size = 10.0;
    f.flakes.push(b);
    let mut keep = b;
    keep.top = 5000.0;
    f.flakes.push(keep);
    f.share = 0.0;
    f.last_check = f.clock - 1.0;
    f.step(DT, None);
    assert_eq!(f.flakes.len(), 1);
}

#[test]
fn settling_flake_below_screen_without_ground_is_renewed() {
    let mut f = field(15);
    f.show_ground(false);
    f.flakes.truncate(1);
    f.flakes[0].settles = true;
    f.flakes[0].top = -500.0;
    f.flakes[0].size = 3.0;
    f.share = 0.0;
    f.last_check = f.clock - 1.0;
    f.step(DT, None);
    assert!(f.flakes.is_empty());
}

// ---- drift ----

#[test]
fn columns_and_index() {
    assert_eq!(columns_for(1.0), 2);
    assert_eq!(columns_for(81.0), 41);
    let d = lone(40.0, 0.0, false);
    assert_eq!(d.count(), 20);
    assert_eq!(d.index(9.0), None);
    assert_eq!(d.index(10.0), Some(0));
    assert_eq!(d.index(49.9), Some(19));
    assert_eq!(d.index(50.0), None);
    assert_eq!(d.clamped_index(-1e9), 0);
    assert_eq!(d.clamped_index(1e9), 19);
    assert_eq!(d.center_x(0), 11.0);
}

#[test]
fn bed_wraps_rounded_corners() {
    let d = lone(100.0, 8.0, true);
    assert!(d.bed_depth > 0.0 && d.bed_depth <= 8.0);
    assert!(d.floor(0) < d.floor(25));
    let sq = lone(100.0, 0.0, true);
    assert_eq!(sq.bed_depth, 0.0);
    let mut d2 = lone(100.0, 8.0, true);
    d2.set_corner(8.0);
    d2.set_corner(0.0);
    assert_eq!(d2.bed_depth, 0.0);
}

#[test]
fn deposit_conserves_area_and_reports_overflow() {
    let mut d = lone(100.0, 0.0, true);
    assert_eq!(d.deposit(50.0, 60.0, true), 0.0);
    assert!((total(&d) - 50.0).abs() < 1e-9);
    assert_eq!(d.deposit(0.0, 60.0, true), 0.0);
    assert_eq!(d.deposit(-1.0, 60.0, false), 0.0);
    let before = total(&d);
    let o = d.deposit(20.0, 10.0, false);
    assert!((total(&d) - before - (20.0 - o)).abs() < 1e-9);
    assert!(o > 0.0);
    assert!((d.deposit(10.0, -500.0, true) - 10.0).abs() < 1e-9);
    d.deposit(5000.0, 60.0, true);
}

#[test]
fn resting_column_slides_downhill() {
    let mut d = lone(100.0, 0.0, false);
    d.heights[10] = 9.0;
    d.heights[11] = 5.0;
    assert!(d.resting_column(10) < 10);
    let mut e = lone(100.0, 0.0, false);
    e.heights[10] = 9.0;
    e.heights[9] = 12.0;
    assert!(e.resting_column(10) > 10);
    let mut g = lone(100.0, 0.0, false);
    g.heights[0] = 10.0;
    assert_eq!(g.resting_column(0), 1);
    let last = g.count() - 1;
    g.heights[last] = 10.0;
    assert_eq!(g.resting_column(last), last - 1);
}

#[test]
fn relax_flattens_a_spike_and_spills_at_open_ends() {
    let mut d = lone(100.0, 0.0, true);
    d.heights[25] = 20.0;
    d.heights[0] = 6.0;
    d.heights[49] = 6.0;
    d.refresh_top();
    let before = total(&d);
    for _ in 0..40 {
        d.relax();
    }
    d.refresh_top();
    assert!(d.top < 20.0);
    assert!(total(&d) + d.spill[0] + d.spill[1] <= before + 1e-6);
    assert!(d.spill[0] > 0.0 && d.spill[1] > 0.0);
    let mut c = lone(100.0, 0.0, false);
    c.heights[0] = 6.0;
    c.heights[1] = 6.0;
    c.refresh_top();
    c.relax();
    assert_eq!(c.spill, [0.0, 0.0]);
    let mut s = lone(6.0, 0.0, true);
    s.heights[1] = 9.0;
    s.refresh_top();
    s.relax();
    assert_eq!(s.heights[1], 9.0);
    let mut e = lone(100.0, 0.0, true);
    e.relax();
    assert_eq!(e.top, 0.0);
}

#[test]
fn relax_curvature_pass_moves_snow() {
    for odd_high in [false, true] {
        let mut d = lone(100.0, 0.0, false);
        for (i, h) in d.heights.iter_mut().enumerate() {
            *h = if (i % 2 == 0) != odd_high { 3.0 } else { 2.2 };
        }
        d.refresh_top();
        let before = d.heights.clone();
        d.relax();
        assert_ne!(before, d.heights);
    }
}

#[test]
fn thaw_melts_and_skips_empty() {
    let mut d = lone(100.0, 0.0, false);
    d.thaw(1.0, 8.0, 0.034, 17.6);
    assert_eq!(d.top, 0.0);
    d.heights.fill(30.0);
    d.heights[3] = 0.0;
    d.refresh_top();
    d.thaw(1.0, 8.0, 0.034, 17.6);
    assert!(d.heights[0] < 30.0 && d.heights[3] == 0.0);
    d.thaw(1e6, 8.0, 0.034, 17.6);
    assert!(d.heights.iter().all(|h| *h == 0.0));
}

#[test]
fn spill_emits_fall_and_rerolls() {
    let mut d = lone(100.0, 0.0, true);
    d.spill = [100.0, 100.0];
    let mut out = Vec::new();
    d.settle(0.05, 4.0, 17.0, 0.034, &mut rng(1), &mut out);
    assert_eq!(out.len(), 2);
    assert_eq!(d.spill, [0.0, 0.0]);
    assert!(matches!(out[0], Emit::Fall { dir, .. } if dir == -1.0));
    assert!(matches!(out[1], Emit::Fall { dir, .. } if dir == 1.0));
}

#[test]
fn shift_scrolls_both_ways() {
    let mut d = lone(20.0, 0.0, false);
    for (i, h) in d.heights.iter_mut().enumerate() {
        *h = i as f64 + 1.0;
    }
    let (a, p) = d.shift(1.0);
    assert_eq!(a, 20.0);
    assert_eq!(p.x, d.center_x(9));
    assert_eq!(d.heights[0], 0.0);
    assert_eq!(d.heights[1], 1.0);
    let (a, _) = d.shift(-1.0);
    assert_eq!(a, 0.0);
    assert_eq!(d.heights[9], 0.0);
}

#[test]
fn resize_variants() {
    let mut d = lone(40.0, 0.0, true);
    d.heights.fill(1.0);
    d.refresh_top();
    assert_eq!(d.resize(20.0, true), 20.0);
    assert_eq!(d.count(), 10);
    assert_eq!(d.resize(10.0, false), 10.0);
    assert_eq!(d.count(), 5);
    assert_eq!(d.resize(20.0, true), 0.0);
    assert_eq!(d.heights[5], 0.0);
    assert_eq!(d.resize(30.0, false), 0.0);
    assert_eq!(d.heights[0], 0.0);
    assert_eq!(d.count(), 15);
    assert_eq!(d.resize(30.0, false), 0.0);
    assert_eq!(d.visible.len(), 15);
}

#[test]
fn visibility_blockers_and_screen_edges() {
    let mut d = lone(100.0, 0.0, true);
    d.recompute_visible(&[Rect4::new(30.0, 90.0, 60.0, 130.0)], 1000.0);
    assert!(d.visible[0] && !d.visible[20]);
    d.recompute_visible(&[], 50.0);
    assert!(d.visible[0] && !d.visible[49]);
    d.origin = Vec2::new(-200.0, 100.0);
    d.recompute_visible(&[], 1000.0);
    assert!(!d.visible[0]);
}

#[test]
fn shape_buffer_layout() {
    let mut d = lone(20.0, 0.0, false);
    d.heights[5] = 16.0;
    let s = d.shape();
    assert_eq!(s.len(), 20);
    assert_eq!(s[5], 6.0);
    assert_eq!(s[4], 4.0);
    assert_eq!(s[3], 1.0);
    assert_eq!(s[0], 0.0);
}

#[test]
fn cut_carves_a_notch() {
    let mut d = lone(100.0, 0.0, false);
    d.heights.fill(10.0);
    d.refresh_top();
    assert_eq!(d.cut(500.0, 500.0), 0.0);
    d.visible[25] = false;
    let removed = d.cut(60.0, 105.0);
    assert!(removed > 0.3);
    assert_eq!(d.heights[25], 10.0);
    assert!(d.heights[27] < 10.0);
    assert!(d.cut(40.0, 98.0) > 0.0);
}

#[test]
fn plough_cut_limits_height() {
    let mut d = lone(100.0, 0.0, false);
    d.heights.fill(10.0);
    d.refresh_top();
    assert_eq!(d.plough_cut(-50.0, -10.0, 2.0), 0.0);
    assert_eq!(d.plough_cut(200.0, 300.0, 2.0), 0.0);
    assert!(d.plough_cut(20.0, 40.0, 2.0) > 0.0);
    assert!(d.heights[6] < 6.0);
    assert_eq!(d.plough_cut(20.0, 40.0, 2.0), 0.0);
}

#[test]
fn break_up_conserves_area() {
    let mut d = lone(100.0, 0.0, false);
    for (i, h) in d.heights.iter_mut().enumerate() {
        *h = if i % 7 == 0 { 0.0 } else { 6.0 };
    }
    d.refresh_top();
    let before = total(&d);
    let mut out = Vec::new();
    d.break_up(1.0, Vec2::new(1.0, 2.0), &mut rng(2), &mut out);
    let shed: f64 = out
        .iter()
        .map(|e| match *e {
            Emit::Release { area, .. } | Emit::Lump { area, .. } | Emit::Fall { area, .. } => area,
        })
        .sum();
    assert!((shed - before).abs() < 1e-6);
    assert!(d.is_empty());
    assert!(out.iter().any(|e| matches!(e, Emit::Lump { .. })));
    let mut p = lone(100.0, 0.0, false);
    p.heights.fill(1.0);
    p.break_up(0.5, Vec2::ZERO, &mut rng(3), &mut Vec::new());
    assert!(p.heights.iter().all(|h| (*h - 0.5).abs() < 1e-9));
    let mut q = lone(100.0, 0.0, false);
    q.heights[3] = 1.0;
    let mut o = Vec::new();
    q.break_up(1.0, Vec2::ZERO, &mut rng(3), &mut o);
    assert!(o.iter().all(|e| matches!(e, Emit::Release { .. })));
}

// ---- carry ----

fn loaded(width: f64) -> Drift {
    let mut d = lone(width, 0.0, true);
    d.heights.fill(5.0);
    d.refresh_top();
    d
}

#[test]
fn carry_empty_pile_resets_offset() {
    let mut d = lone(100.0, 0.0, true);
    d.offset = Vec2::new(1.0, 1.0);
    d.origin = Vec2::new(300.0, 100.0);
    let mut out = Vec::new();
    d.carry(DT, &mut rng(1), &mut out);
    assert_eq!(d.offset, Vec2::ZERO);
    assert!(out.is_empty());
}

#[test]
fn carry_follows_origin_and_sheds_columns() {
    let mut d = loaded(100.0);
    let mut out = Vec::new();
    let mut r = rng(1);
    d.carry(DT, &mut r, &mut out);
    d.origin = Vec2::new(d.origin.x + 40.0, 100.0);
    d.velocity = Vec2::new(900.0, 0.0);
    d.track = d.origin - Vec2::new(54.0, 0.0);
    for _ in 0..60 {
        d.carry(DT, &mut r, &mut out);
    }
    assert!(d.track.dist(d.origin) < 1.0);
    let mut e = loaded(100.0);
    for k in 0..30 {
        let s = if k % 2 == 0 { 1.0 } else { -1.0 };
        e.velocity = Vec2::new(s * 3000.0, 0.0);
        e.track = e.origin - Vec2::new(s * 100.0, 0.0);
        e.carry(DT, &mut r, &mut out);
    }
    assert!(out.iter().any(|x| matches!(x, Emit::Release { .. })));
}

#[test]
fn carry_lift_breaks_pile() {
    let mut d = loaded(100.0);
    d.offset = Vec2::new(0.0, 12.0);
    d.offset_vel = Vec2::new(0.0, 100.0);
    let mut out = Vec::new();
    d.carry(DT, &mut rng(1), &mut out);
    assert!(d.is_empty());
    assert_eq!(d.offset, Vec2::ZERO);
    assert!(!out.is_empty());
}

#[test]
fn carry_airborne_lands_again() {
    let mut d = loaded(100.0);
    d.offset = Vec2::new(0.0, 1.0);
    d.offset_vel = Vec2::new(0.0, -500.0);
    d.carry(DT, &mut rng(1), &mut Vec::new());
    assert_eq!(d.offset.y, 0.0);
    assert_eq!(d.offset_vel.y, 0.0);
}

#[test]
fn carry_friction_branches() {
    let mut out = Vec::new();
    // slow slide stops: friction reverses it
    let mut d = loaded(100.0);
    d.offset_vel = Vec2::new(1.0, 0.0);
    d.carry(DT, &mut rng(1), &mut out);
    assert_eq!(d.offset_vel.x, 0.0);
    // negative slide keeps its sign
    let mut n = loaded(100.0);
    n.offset_vel = Vec2::new(-500.0, 0.0);
    n.carry(DT, &mut rng(1), &mut out);
    assert!(n.offset_vel.x < 0.0);
    // airborne, no push, no slide: nothing happens
    let mut a = loaded(100.0);
    a.offset = Vec2::new(0.0, 3.0);
    a.carry(DT, &mut rng(1), &mut out);
    assert_eq!(a.offset_vel.x, 0.0);
    // airborne and shoved: static grip is zero, so it slides (push sign)
    let mut b = loaded(100.0);
    b.offset = Vec2::new(0.0, 3.0);
    b.velocity = Vec2::new(-2000.0, 0.0);
    b.track = b.origin;
    b.carry(DT, &mut rng(1), &mut out);
    assert!(b.offset_vel.x != 0.0);
    // a hard shove on the ground beats static grip
    let mut c = loaded(100.0);
    c.track = c.origin - Vec2::new(300.0, 0.0);
    c.velocity = Vec2::new(2000.0, 0.0);
    c.carry(DT, &mut rng(1), &mut out);
    assert!(c.offset_vel.x != 0.0, "shove beats 3x grip");
    // a gentle push on the ground stays gripped
    let mut g = loaded(100.0);
    g.track = g.origin - Vec2::new(1.0, 0.0);
    g.carry(DT, &mut rng(1), &mut out);
    assert_eq!(g.offset_vel.x, 0.0);
}

#[test]
fn shaking_throws_snow_off() {
    let mut d = loaded(200.0);
    let mut out = Vec::new();
    let mut r = rng(9);
    d.agitation = 10.0;
    d.shake(Vec2::ZERO, 1.0 / 30.0, &mut r, &mut out);
    assert!(!out.is_empty());
    let mut e = loaded(200.0);
    e.note_swing(Vec2::new(20000.0, 0.0), DT);
    e.note_swing(Vec2::new(-20000.0, 0.0), DT);
    e.note_swing(Vec2::new(20000.0, 0.0), DT);
    assert!(e.agitation > 4.0);
    let mut o2 = Vec::new();
    for _ in 0..30 {
        e.agitation = 12.0;
        e.carry(DT, &mut r, &mut o2);
    }
    assert!(o2.iter().any(|x| matches!(x, Emit::Release { .. })));
}

// ---- release / lumps / falls ----

#[test]
fn release_makes_clamped_grains() {
    let mut f = field(20);
    f.release(0.01, Vec2::new(100.0, 100.0), Vec2::ZERO, Vec2::ZERO, 0.0);
    assert!(f.grains.is_empty());
    f.release(1.0, Vec2::new(100.0, 100.0), Vec2::ZERO, Vec2::ZERO, 0.0);
    assert_eq!(f.grains.len(), 1);
    f.release(100.0, Vec2::new(100.0, 100.0), Vec2::new(10.0, 0.0), Vec2::new(2.0, 2.0), 0.3);
    assert_eq!(f.grains.len(), 7);
    let g = f.grains.last().unwrap();
    assert!(g.size >= 1.1 && g.size <= 5.0 && g.lump == 0.0);
}

#[test]
fn full_grain_cap_deposits_on_ground() {
    let mut f = field(21);
    f.grains = vec![grain(-1000.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0); MAX_GRAINS];
    let before = total(&f.ground);
    f.release(10.0, Vec2::new(300.0, 100.0), Vec2::ZERO, Vec2::ZERO, 0.0);
    f.throw_lump(200.0, Vec2::new(300.0, 100.0), Vec2::ZERO);
    assert_eq!(f.grains.len(), MAX_GRAINS);
    assert!((total(&f.ground) - before - 210.0).abs() < 1e-6);
}

#[test]
fn throw_lump_small_and_large() {
    let mut f = field(22);
    f.throw_lump(4.0, Vec2::new(100.0, 100.0), Vec2::ZERO);
    assert!(f.grains.iter().all(|g| g.lump == 0.0));
    f.grains.clear();
    f.throw_lump(200.0, Vec2::new(100.0, 100.0), Vec2::ZERO);
    assert_eq!(f.grains.len(), 3);
    assert!(f.grains.iter().all(|g| g.lump >= 0.05 && g.size <= 11.0));
}

#[test]
fn fall_splits_into_lumps() {
    let mut f = field(23);
    f.fall(300.0, Vec2::new(100.0, 100.0), 1.0, Vec2::ZERO);
    assert!(!f.grains.is_empty());
}

// ---- grains landing ----

#[test]
fn crumb_lands_and_overflow_rereleased_from_ledges() {
    let mut f = field(24);
    f.update(&[surf(5, 200.0, 200.0, 40.0, 100.0)], 0.016, false);
    f.grains.push(grain(201.0, 300.5, 4.0, -50.0, 60.0, 3.0, 0.0));
    f.advance_grains(DT);
    assert!(f.drift(SurfaceId::Window(5)).unwrap().top > 0.0);
    assert!(!f.grains.is_empty());
    let mut g = field(25);
    g.grains.push(grain(1.0, 1.0, 0.0, -50.0, 60.0, 3.0, 0.0));
    g.advance_grains(DT);
    assert!(g.grains.is_empty());
    assert!(g.ground.top > 0.0);
}

#[test]
fn lump_bursts_into_crumbs_and_puffs() {
    let mut f = field(26);
    f.grains.push(grain(300.0, 1.0, 10.0, -800.0, 80.0, 9.0, 0.5));
    f.advance_grains(DT);
    assert!(f.ground.top > 0.0);
    assert!(!f.puffs.is_empty());
    assert!(f.grains.iter().all(|g| g.lump == 0.0));
    let mut h = field(27);
    h.update(&[surf(5, 200.0, 200.0, 12.0, 100.0)], 0.016, false);
    h.grains.push(grain(205.0, 301.0, 0.0, -300.0, 120.0, 11.0, 0.5));
    h.advance_grains(DT);
    assert!(h.drift(SurfaceId::Window(5)).unwrap().top > 0.0);
    let mut p = field(28);
    p.puffs = vec![old_puff(100.0); MAX_PUFFS];
    p.grains.push(grain(300.0, 1.0, 0.0, -800.0, 80.0, 9.0, 0.5));
    p.advance_grains(DT);
    assert_eq!(p.puffs.len(), MAX_PUFFS);
}

#[test]
fn lump_outside_its_drift_uses_own_height() {
    let mut f = field(29);
    f.update(&[surf(5, 200.0, 200.0, 40.0, 100.0)], 0.016, false);
    f.land(grain(5000.0, 7.0, 0.0, -300.0, 50.0, 7.0, 0.3), SurfaceId::Window(5));
    assert!(!f.puffs.is_empty());
}

#[test]
fn landing_on_missing_drift_does_nothing() {
    let mut f = field(30);
    f.land(grain(5.0, 7.0, 0.0, -300.0, 50.0, 7.0, 0.3), SurfaceId::Window(404));
    assert!(f.grains.is_empty() && f.puffs.is_empty());
}

#[test]
fn grains_leave_the_screen_and_get_pushed_by_wind() {
    let mut f = field(31);
    f.grains.push(grain(-29.0, 500.0, -900.0, 0.0, 1.0, 1.0, 0.0));
    f.grains.push(grain(300.0, 500.0, 0.0, 100.0, 1.0, 1.0, 0.0));
    f.blow.speed = 100.0;
    f.advance_grains(DT);
    assert_eq!(f.grains.len(), 1);
    assert!(f.grains[0].vel.x > 0.0);
    let mut g = field(32);
    g.show_ground(false);
    g.grains.push(grain(300.0, -19.0, 0.0, -500.0, 1.0, 1.0, 0.0));
    g.advance_grains(DT);
    assert!(g.grains.is_empty());
}

#[test]
fn puffs_fade_out() {
    let mut f = field(33);
    f.puffs.push(Puff { pos: Vec2::ZERO, vel: Vec2::new(10.0, 10.0), size: 2.0, alpha: 0.2, life: 0.5, age: 0.0 });
    f.advance_puffs(0.3);
    assert_eq!(f.puffs.len(), 1);
    f.advance_puffs(0.3);
    assert!(f.puffs.is_empty());
}

// ---- landing helper ----

#[test]
fn landing_picks_highest_visible_surface() {
    let mut f = field(34);
    f.update(&[surf(1, 100.0, 100.0, 200.0, 100.0), surf(2, 120.0, 150.0, 100.0, 100.0)], 0.016, false);
    assert_eq!(f.landing(150.0, 400.0, 390.0), None);
    assert_eq!(f.landing(150.0, 400.0, 240.0), Some(SurfaceId::Window(2)));
    assert_eq!(f.landing(150.0, 220.0, 190.0), Some(SurfaceId::Window(1)));
    assert_eq!(f.landing(150.0, 400.0, 190.0), Some(SurfaceId::Window(2)));
    f.drift_mut(SurfaceId::Window(2)).unwrap().visible.iter_mut().for_each(|v| *v = false);
    assert_eq!(f.landing(150.0, 400.0, 195.0), Some(SurfaceId::Window(1)));
    assert_eq!(f.landing(500.0, 10.0, -1.0), Some(SurfaceId::Ground));
    f.show_ground(false);
    assert_eq!(f.landing(500.0, 10.0, -1.0), None);
}

// ---- update ----

#[test]
fn update_creates_follows_and_removes() {
    let mut f = field(40);
    let s = surf(5, 200.0, 200.0, 100.0, 100.0);
    f.update(std::slice::from_ref(&s), 0.016, false);
    assert_eq!(f.ledges.len(), 1);
    pile(&mut f, SurfaceId::Window(5), 6.0);
    f.update(std::slice::from_ref(&s), 0.016, false);
    assert_eq!(f.ledges.len(), 1);
    let ground = Surface::new(SurfaceId::Ground, Frame::new(0.0, 0.0, 800.0, 10.0));
    f.update(&[s.clone(), ground], 0.016, false);
    assert_eq!(f.ledges.len(), 1);
    f.update(&[], 0.016, false);
    assert!(f.ledges.is_empty());
    assert!(!f.grains.is_empty());
    let mut g = field(41);
    g.update(&[s], 0.016, false);
    g.update(&[], 0.016, false);
    assert!(g.grains.is_empty());
}

#[test]
fn update_drag_start_teleport_and_resize() {
    let mut f = field(42);
    let id = SurfaceId::Window(5);
    f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0)], 0.016, false);
    pile(&mut f, id, 6.0);
    f.clock += 5.0;
    f.update(&[surf(5, 210.0, 200.0, 100.0, 100.0)], 0.1, false);
    let d = f.drift(id).unwrap();
    assert!(d.velocity.x > 0.0 && d.track.x < d.origin.x);
    f.update(&[surf(5, 215.0, 200.0, 100.0, 100.0)], 0.016, false);
    f.update(&[surf(5, 600.0, 400.0, 100.0, 100.0)], 0.016, false);
    assert!(f.drift(id).unwrap().is_empty());
    assert!(!f.grains.is_empty());
    pile(&mut f, id, 6.0);
    f.update(&[surf(5, 610.0, 400.0, 100.0, 120.0)], 0.016, false);
    let d = f.drift(id).unwrap();
    assert_eq!(d.velocity, Vec2::ZERO);
    assert_eq!(d.track, d.origin);
}

#[test]
fn update_width_changes_throw_cut_snow() {
    let mut f = field(43);
    let id = SurfaceId::Window(5);
    f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0)], 0.016, false);
    pile(&mut f, id, 6.0);
    f.update(&[surf(5, 200.0, 200.0, 60.0, 100.0)], 0.016, false);
    assert_eq!(f.drift(id).unwrap().count(), 30);
    assert!(!f.grains.is_empty());
    f.grains.clear();
    f.update(&[surf(5, 220.0, 200.0, 40.0, 100.0)], 0.016, false);
    assert!(!f.grains.is_empty());
    f.grains.clear();
    f.update(&[surf(5, 220.0, 200.0, 80.0, 100.0)], 0.016, false);
    assert!(f.grains.is_empty());
}

#[test]
fn update_stashes_when_switching_spaces() {
    let mut f = field(44);
    let id = SurfaceId::Window(5);
    f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0), surf(0, 0.0, 0.0, 800.0, 40.0)], 0.016, false);
    pile(&mut f, id, 6.0);
    f.update(&[], 0.016, true);
    assert!(f.drift(id).is_none());
    assert_eq!(f.away.len(), 1);
    f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0)], 0.016, false);
    assert!(f.drift(id).unwrap().top > 5.0);
    assert!(f.away.is_empty());
}

#[test]
fn away_stash_is_bounded() {
    let mut f = field(45);
    let many: Vec<Surface> = (1..=130).map(|i| surf(i, 10.0, 200.0, 100.0, 100.0)).collect();
    f.update(&many, 0.016, false);
    f.update(&[], 0.016, true);
    assert_eq!(f.away.len(), 100);
    assert!(!f.away.contains_key(&SurfaceId::Window(1)));
}

#[test]
fn update_blockers_hide_columns_and_corner_changes() {
    let mut f = field(46);
    let mut s = surf(5, 200.0, 200.0, 100.0, 100.0);
    s.blockers = (0..20).map(|i| Rect4::new(200.0, 290.0 + f64::from(i), 220.0, 320.0)).collect();
    s.corner_radius = 8.0;
    f.update(std::slice::from_ref(&s), 0.016, false);
    let d = f.drift(SurfaceId::Window(5)).unwrap();
    assert_eq!(d.occluders.len(), 16);
    assert!(!d.visible[0] && d.visible[40]);
    assert!(d.bed_depth > 0.0);
    s.corner_radius = 0.0;
    f.update(&[s], 0.016, false);
    assert_eq!(f.drift(SurfaceId::Window(5)).unwrap().bed_depth, 0.0);
}

#[test]
fn moving_window_sheds_snow_in_steps() {
    let mut f = field(47);
    let id = SurfaceId::Window(5);
    f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0)], 0.016, false);
    pile(&mut f, id, 6.0);
    f.clock += 5.0;
    let mut x = 200.0;
    for k in 0..60 {
        x += if k < 30 { 25.0 } else { -25.0 };
        f.update(&[surf(5, x, 200.0, 100.0, 100.0)], 0.016, false);
        f.step(DT, None);
    }
    assert!(!f.grains.is_empty() || f.drift(id).unwrap().top < 6.0);
}

// ---- cursor ----

#[test]
fn cursor_sweep_carves_and_stirs() {
    let mut f = field(50);
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.update(&[surf(5, 200.0, 200.0, 300.0, 100.0)], 0.016, false);
    pile(&mut f, SurfaceId::Window(5), 8.0);
    f.flakes[0].landing = Some((SurfaceId::Ground, 1.0));
    let c = f.flakes[0].position(f.now(), f.wind_now(), f.span());
    f.step(DT, Some(Vec2::new(c.x - 30.0, c.y)));
    let before = total(&f.ground);
    f.step(DT, Some(Vec2::new(300.0, 8.0)));
    f.step(DT, Some(Vec2::new(340.0, 8.0)));
    assert!(total(&f.ground) < before);
    let ledge_before = total(f.drift(SurfaceId::Window(5)).unwrap());
    f.step(DT, Some(Vec2::new(300.0, 300.0)));
    f.step(DT, Some(Vec2::new(340.0, 300.0)));
    assert!(total(f.drift(SurfaceId::Window(5)).unwrap()) < ledge_before);
    f.step(DT, Some(Vec2::new(340.1, 300.0)));
    f.step(0.0, Some(Vec2::new(500.0, 300.0)));
    f.step(DT, None);
}

#[test]
fn cursor_directions_and_feed_gather() {
    let (mut f, feed) = feed_field(51);
    feed.publish_targets(Vec::new(), true);
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.cursor_prev = Some(Vec2::new(300.0, 6.0));
    f.step(0.1, Some(Vec2::new(320.0, 6.0)));
    assert_eq!(feed.take().1.len(), 1);
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.cursor_prev = Some(Vec2::new(300.0, 0.0));
    f.step(0.1, Some(Vec2::new(300.0, 10.0)));
    assert_eq!(feed.take().1.len(), 1);
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.cursor_prev = Some(Vec2::new(300.0, 6.0));
    f.step(DT, Some(Vec2::new(500.0, 6.0)));
    assert!(feed.take().1.is_empty());
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.cursor_prev = Some(Vec2::new(300.0, 6.0));
    f.step(0.1, Some(Vec2::new(280.0, 6.0)));
}

#[test]
fn hidden_ground_is_not_carved() {
    let mut f = field(52);
    f.show_ground(false);
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.cursor_prev = Some(Vec2::new(300.0, 6.0));
    f.step(0.1, Some(Vec2::new(340.0, 6.0)));
    assert!(f.ground.top > 11.0);
}

// ---- plough / bursts ----

fn blade(front: f64, way: f64, on_dock: bool) -> Blade {
    Blade { front, way, speed: 200.0, leave: 1.0, on_dock }
}

#[test]
fn plough_pushes_ground_snow() {
    let (mut f, feed) = feed_field(60);
    pile(&mut f, SurfaceId::Ground, 12.0);
    feed.publish_blade(Some((f.cfg.origin, blade(400.0, 1.0, false))));
    f.step(DT, None);
    assert!(f.ground.heights[200] < 12.0);
    assert!(!f.grains.is_empty());
    feed.publish_blade(Some((f.cfg.origin, blade(2.0, -1.0, false))));
    f.step(DT, None);
    f.step(DT, None);
    feed.publish_blade(Some((f.cfg.origin, blade(-500.0, 1.0, false))));
    f.step(DT, None);
    feed.publish_blade(Some((f.cfg.origin, blade(400.0, 1.0, false))));
    f.show_ground(false);
    pile(&mut f, SurfaceId::Ground, 12.0);
    f.step(DT, None);
    assert_eq!(f.ground.heights[200], 12.0);
    f.show_ground(true);
    feed.publish_blade(Some((f.cfg.origin, blade(400.0, 1.0, true))));
    f.step(DT, None);
    f.update(&[surf(0, 0.0, 0.0, 800.0, 40.0)], 0.016, false);
    pile(&mut f, SurfaceId::Dock, 12.0);
    f.step(DT, None);
    assert!(f.drift(SurfaceId::Dock).unwrap().heights[200] < 12.0);
}

#[test]
fn bursts_shatter_into_lumps_and_puffs() {
    let (mut f, feed) = feed_field(61);
    feed.publish_burst(
        f.cfg.origin,
        Burst { point: Vec2::new(300.0, 300.0), area: 900.0, velocity: Vec2::new(100.0, -50.0) },
    );
    f.step(DT, None);
    assert!(f.grains.len() >= 5);
    assert!(f.puffs.len() >= 20);
    f.grains = vec![grain(-1000.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0); MAX_GRAINS];
    let before = total(&f.ground);
    f.shatter(Vec2::new(300.0, 300.0), 450.0, Vec2::ZERO);
    assert!(total(&f.ground) > before);
    f.puffs = vec![old_puff(100.0); MAX_PUFFS];
    f.shatter(Vec2::new(300.0, 300.0), 450.0, Vec2::ZERO);
    assert_eq!(f.puffs.len(), MAX_PUFFS);
}

// ---- drawing ----

#[test]
fn draw_emits_passes_per_layer() {
    let mut f = field(70);
    let mut hidden_one = surf(7, 100.0, 100.0, 50.0, 50.0);
    hidden_one.blockers = vec![Rect4::new(0.0, 0.0, 10.0, 10.0)];
    f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0), surf(6, 400.0, 200.0, 100.0, 100.0), hidden_one], 0.016, false);
    pile(&mut f, SurfaceId::Ground, 6.0);
    pile(&mut f, SurfaceId::Window(5), 6.0);
    f.release(30.0, Vec2::new(300.0, 300.0), Vec2::ZERO, Vec2::ZERO, 0.0);
    f.puffs.push(Puff { pos: Vec2::ZERO, vel: Vec2::ZERO, size: 2.0, alpha: 0.2, life: 1.0, age: 0.5 });
    f.shatter(Vec2::new(300.0, 300.0), 450.0, Vec2::ZERO);
    let kinds = |l: &DrawList| l.passes.iter().map(|p| p.kind).collect::<Vec<_>>();

    let mut all = DrawList::default();
    f.draw(Layer::Main, &mut all);
    assert_eq!(all.scene[0], 800.0);
    assert_eq!(all.scene[3], 1.0);
    let k = kinds(&all);
    assert_eq!(k.iter().filter(|x| **x == PassKind::SnowDrift).count(), 2);
    assert!(k.contains(&PassKind::SnowFlakes) && k.contains(&PassKind::SnowLoose));
    let mut none = DrawList::default();
    f.draw(Layer::Above, &mut none);
    assert!(none.is_empty());

    f.set_split(true);
    let mut main = DrawList::default();
    f.draw(Layer::Main, &mut main);
    assert_eq!(kinds(&main).iter().filter(|x| **x == PassKind::SnowDrift).count(), 1);
    let mut above = DrawList::default();
    f.draw(Layer::Above, &mut above);
    assert_eq!(kinds(&above), vec![PassKind::SnowDrift]);
    assert!(f.has_above());

    let p = &above.passes[0];
    assert_eq!(p.vertices, 4);
    assert_eq!(p.uniforms.len(), 8);
    assert_eq!(p.uniforms[3], 50.0);
    assert_eq!(p.buffers[0].len(), 50 * 2 * 4);
    assert_eq!(p.buffers[1].len(), 16);

    f.show_ground(false);
    f.set_split(false);
    f.ledges[0].1.occluders = vec![Rect4::new(1.0, 2.0, 3.0, 4.0)];
    let mut hidden = DrawList::default();
    f.draw(Layer::Main, &mut hidden);
    let drift = hidden.passes.iter().find(|p| p.kind == PassKind::SnowDrift).unwrap();
    assert_eq!(drift.buffers[1].len(), 16);
    assert_eq!(drift.uniforms[6], 1.0);
    let flakes = hidden.passes.iter().find(|p| p.kind == PassKind::SnowFlakes).unwrap();
    assert_eq!(flakes.instances as usize, f.flakes.len());
    assert_eq!(flakes.uniforms[3], 860.0);
}

#[test]
fn draw_with_nothing_to_draw() {
    let mut f = field(71);
    f.flakes.clear();
    let mut out = DrawList::default();
    f.draw(Layer::Main, &mut out);
    assert!(out.is_empty());
}

// ---- long mixed run ----

#[test]
fn long_run_with_wind_cursor_and_windows_stays_sane() {
    for (seed, wind) in [(80, Wind::Changing), (81, Wind::StrongLeft), (82, Wind::GentleRight)] {
        let (mut f, feed) = feed_field(seed);
        f.set_wind(wind);
        f.set_split(seed % 2 == 0);
        let wins =
            [surf(1, 100.0, 150.0, 250.0, 120.0), surf(2, 300.0, 300.0, 300.0, 100.0), surf(0, 0.0, 0.0, 800.0, 40.0)];
        f.update(&wins, 0.016, false);
        for k in 0..2400 {
            let t = f64::from(k) * DT;
            let cursor = (k % 400 > 200).then(|| Vec2::new(100.0 + (t * 200.0) % 600.0, 30.0 + (t * 40.0) % 300.0));
            f.step(DT, cursor);
            if k % 30 == 0 {
                let mut w = wins.clone();
                w[0].frame.x += (f64::from(k) / 30.0).sin() * 40.0;
                f.update(&w, 0.05, k % 700 == 0);
            }
            if k % 500 == 0 {
                feed.publish_burst(
                    f.cfg.origin,
                    Burst { point: Vec2::new(200.0, 200.0), area: 300.0, velocity: Vec2::ZERO },
                );
                feed.publish_blade(Some((f.cfg.origin, blade(100.0 + f64::from(k) % 500.0, 1.0, k % 1000 == 0))));
            }
        }
        assert!(f.grains.len() <= MAX_GRAINS && f.puffs.len() <= MAX_PUFFS);
        assert!(f.ground.heights.iter().all(|h| h.is_finite() && *h >= 0.0));
        let mut out = DrawList::default();
        f.draw(Layer::Main, &mut out);
        f.draw(Layer::Above, &mut out);
    }
}

// ---- decor ----

#[test]
fn decor_dots_are_deterministic_and_in_range() {
    let a = decor::dots(40, 300.0, 200.0, 12.5);
    assert_eq!(a, decor::dots(40, 300.0, 200.0, 12.5));
    assert_eq!(a.len(), 40);
    for d in &a {
        assert!(d.y >= -5.0 && d.y < 205.0);
        assert!(d.radius >= 1.0 && d.radius <= 3.2);
        assert!(d.alpha >= 0.55 && d.alpha <= 1.0);
        assert!(d.x > -12.0 && d.x < 312.0);
    }
    let b = decor::dots(40, 300.0, 200.0, 13.5);
    assert_ne!(a[0].y, b[0].y);
    assert!(decor::dots(0, 1.0, 1.0, 0.0).is_empty());
}

// ---- verifier: exact spec formulas and edge input ----

#[test]
fn closed_form_matches_integrated_velocity() {
    // snow-sim 3.3: position is the exact integral of the streak velocity.
    let mut r = rng(90);
    let fl = Flake::spawn(&mut r, 800.0, 600.0, 50.0, 2.0, 0.0, false);
    let span = 1e9;
    let steps = 200_000;
    let t_end = 4.0;
    let h = t_end / f64::from(steps);
    let (mut x, mut y) = (fl.x, fl.top);
    for k in 0..steps {
        let a = (f64::from(k) + 0.5) * h;
        let p = fl.phase + fl.sway_rate * a;
        x += fl.sway * (p.cos() + 0.3 * (2.3 * p).cos()) * h;
        y -= fl.fall * (1.0 + 0.12 * (1.3 * p).sin()) * h;
    }
    let pos = fl.position(2.0 + t_end, 0.0, span);
    assert!((pos.x - x).abs() < 1e-6, "{} vs {x}", pos.x);
    assert!((pos.y - y).abs() < 1e-6, "{} vs {y}", pos.y);
    // wind shifts by drift * (wind - wind_at_birth)
    let shifted = fl.position(6.0, 10.0, span);
    assert!((shifted.x - pos.x - fl.drift * 10.0).abs() < 1e-9);
    // wrap keeps x in [-30, W+30)
    let w = fl.position(6.0, 5000.0, 860.0);
    assert!(w.x >= -30.0 && w.x < 830.0);
}

#[test]
fn deposit_kernel_is_exact_triangle() {
    let mut d = lone(100.0, 0.0, false);
    // area 18 -> k = floor(sqrt(9)) = 3, weights (4-|i|)/16
    assert_eq!(d.deposit(18.0, 60.0, false), 0.0);
    let col = 25;
    assert!((d.heights[col] - 18.0 * 4.0 / 16.0 / 2.0).abs() < 1e-12);
    assert!((d.heights[col + 3] - 18.0 / 16.0 / 2.0).abs() < 1e-12);
    assert_eq!(d.heights[col + 4], 0.0);
    // tiny wide deposit still spreads over k = 2
    let mut e = lone(100.0, 0.0, false);
    e.deposit(1.0, 60.0, true);
    assert!(e.heights[col + 2] > 0.0 && e.heights[col + 3] == 0.0);
    // far away and non-finite x: everything overflows, no panic
    let mut f = lone(100.0, 0.0, false);
    assert_eq!(f.deposit(7.0, f64::INFINITY, true), 7.0);
    assert_eq!(f.deposit(7.0, f64::NEG_INFINITY, true), 7.0);
    assert_eq!(f.deposit(f64::NAN, 60.0, true), 0.0);
    assert_eq!(total(&f), 0.0);
}

#[test]
fn settle_uses_spec_thaw_floors() {
    let expect = |h: f64, floor: f64, settle: f64| h - (0.034 + (h - floor).max(0.0) / settle) * 0.05;
    for keep in [false, true] {
        let mut f = field(91);
        f.keep_clear(keep);
        f.update(&[surf(5, 200.0, 200.0, 100.0, 100.0)], 0.016, false);
        pile(&mut f, SurfaceId::Ground, 30.0);
        pile(&mut f, SurfaceId::Window(5), 0.5);
        f.settle(0.05);
        let (gf, lf, s) = if keep { (3.0, 0.0, 8.0) } else { (8.0, 4.0, THAW_SETTLE) };
        assert!((f.ground.heights[100] - expect(30.0, gf, s)).abs() < 1e-12);
        let ledge = f.drift(SurfaceId::Window(5)).unwrap();
        assert!((ledge.heights[20] - expect(0.5, lf, s)).abs() < 1e-12);
    }
}

#[test]
fn landed_flake_deposits_size_squared_times_fill() {
    let mut f = field(92);
    f.flakes.truncate(1);
    f.share = 0.0;
    pile(&mut f, SurfaceId::Ground, 10.0);
    let col = 150;
    let cap_fill = f.ground.fill(col);
    let x = f.ground.center_x(col);
    f.flakes[0].landing = Some((SurfaceId::Ground, x));
    f.flakes[0].gone = 0.0;
    f.flakes[0].size = 4.0;
    let before = total(&f.ground);
    f.check_flakes();
    let area = 16.0 * 1.1 * cap_fill;
    assert!((total(&f.ground) - before - area).abs() < 1e-9);
    assert!(f.flakes.is_empty());
}

#[test]
fn smooth_damp_matches_spec() {
    let mut d = lone(100.0, 0.0, true);
    d.track = d.origin - Vec2::new(10.0, 0.0);
    d.velocity = Vec2::new(5.0, 0.0);
    let dt = 0.02;
    let omega = 2.0 / 0.06;
    let x = omega * dt;
    let k = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let c = -10.0;
    let t = (5.0 + omega * c) * dt;
    d.carry(dt, &mut rng(1), &mut Vec::new());
    assert!((d.velocity.x - (5.0 - omega * t) * k).abs() < 1e-12);
    assert!((d.track.x - (d.origin.x + (c + t) * k)).abs() < 1e-12);
}

#[test]
fn carry_huge_or_bad_offset_is_bounded() {
    let mut d = loaded(20.0);
    d.offset = Vec2::new(1e12, 0.0);
    let mut out = Vec::new();
    d.carry(DT, &mut rng(1), &mut out);
    assert!(d.offset.x.abs() < 2.0);
    assert_eq!(out.len(), 10);
    assert!(d.is_empty());
    let mut n = loaded(20.0);
    n.offset = Vec2::new(f64::NAN, 0.0);
    let mut out = Vec::new();
    n.carry(DT, &mut rng(1), &mut out);
    assert_eq!(n.offset.x, 0.0);
    assert!(out.is_empty());
    // offset -4.5 sheds two columns to the left, keeps -0.5
    let mut l = loaded(20.0);
    l.offset = Vec2::new(-4.5, 0.0);
    let mut out = Vec::new();
    l.carry(1e-9, &mut rng(1), &mut out);
    assert!((l.offset.x + 0.5).abs() < 1e-6);
    assert_eq!(out.len(), 2);
    assert_eq!(l.heights[9], 0.0);
}

#[test]
fn bad_inputs_do_not_poison_the_field() {
    let mut f = field(93);
    f.release(f64::NAN, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, 0.0);
    f.throw_lump(f64::NAN, Vec2::ZERO, Vec2::ZERO);
    f.shatter(Vec2::new(10.0, 10.0), -5.0, Vec2::ZERO);
    f.shatter(Vec2::new(10.0, 10.0), f64::NAN, Vec2::ZERO);
    assert!(f.grains.is_empty() && f.puffs.is_empty());
    f.set_share(f64::INFINITY);
    assert_eq!(f.share(), 0.0);
    f.set_share(f64::NAN);
    assert_eq!(f.share(), 0.0);
    let clock = f.clock;
    f.step(f64::NAN, None);
    f.step(-1.0, None);
    assert_eq!(f.clock, clock);
}

#[test]
fn scene_time_is_elapsed_and_survives_rebase() {
    let mut f = field(94);
    let mut out = DrawList::default();
    f.draw(Layer::Main, &mut out);
    assert_eq!(out.scene[2], 0.0);
    for _ in 0..13 {
        f.step(50.0, None);
    }
    f.draw(Layer::Main, &mut out);
    assert_eq!(out.scene[2], 650.0);
    assert!(f.now() < 600.0);
}

#[test]
fn changing_wind_follows_the_wall_clock() {
    use fp_core::falling::Falling;
    let run = |t: Option<f64>| {
        let mut f = SnowField::new(cfg(1), None);
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
