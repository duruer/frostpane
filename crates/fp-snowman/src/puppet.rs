//! Walking snowman puppet used by the limit snowman and fights (snowman F9,
//! snowman-fight 9): a small three-ball rig that walks, hops and waves.

use crate::art::{draw_arm_stick, puppet_body, puppet_head, snow_ball_plain};
use crate::{Face, Look};
use fp_paint::Sketch;
use fp_paint::tiny_skia::Pixmap;
use std::f64::consts::PI;

/// Walking speed in points per second.
pub const WALK_SPEED: f64 = 100.0;
/// Height of a hop in points.
pub const HOP_HEIGHT: f64 = 12.0;
/// Duration of a hop in seconds.
pub const HOP_TIME: f64 = 0.3;
/// The puppet waves this long ...
pub const WAVE_FOR: f64 = 1.6;
/// ... out of every period (seconds, while standing).
pub const WAVE_EVERY: f64 = 4.5;
/// Frame time clamp.
pub const MAX_DT: f64 = 0.05;
/// The eyes blink shut for `BLINK_FOR` seconds ...
pub const BLINK_FOR: f64 = 0.11;
/// ... out of every `BLINK_EVERY` seconds.
pub const BLINK_EVERY: f64 = 2.7;
/// Puppet height in the popups (the fighter rig scaled by 44 / 58).
pub const POPUP_SIZE: f64 = 44.0;
/// Height of the unscaled rig in its own units.
const RIG_HEIGHT: f64 = 58.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puppet {
    pub x: f64,
    /// Current hop height in points (0 when on the ground).
    pub hop: f64,
    pub waving: bool,
    /// Seconds since creation (drives wave cycle and breathing).
    pub time: f64,
    /// Odometer: total distance walked.
    pub walked: f64,
    /// True while the puppet is still moving towards its goal.
    pub walking: bool,
    hop_age: f64,
}

impl Default for Puppet {
    fn default() -> Self {
        Puppet { x: 0.0, hop: 0.0, waving: false, time: 0.0, walked: 0.0, walking: false, hop_age: HOP_TIME }
    }
}

impl Puppet {
    /// A puppet standing at `x`.
    pub fn at(x: f64) -> Puppet {
        Puppet { x, ..Puppet::default() }
    }

    /// Starts a 0.3 s sine hop of 12 points (the bubble pop-in).
    pub fn start_hop(&mut self) {
        self.hop_age = 0.0;
    }

    /// Advance by `dt` seconds (clamped to 0..0.05, NaN counts as 0) towards
    /// `target_x`, moving at most `100 * dt`. The puppet counts as walking
    /// while it is more than 0.5 away from the target. A non-finite target
    /// (or position) stops it where it is.
    pub fn step(&mut self, dt: f64, target_x: f64) {
        let dt = if dt.is_nan() { 0.0 } else { dt.clamp(0.0, MAX_DT) };
        self.time += dt;
        let gap = target_x - self.x;
        let gap = if gap.is_finite() { gap } else { 0.0 };
        self.walking = gap.abs() > 0.5;
        let step = gap.signum() * (WALK_SPEED * dt).min(gap.abs());
        self.x += step;
        self.walked += step.abs();
        if self.hop_age < HOP_TIME {
            self.hop_age += dt;
        }
        self.hop = if self.hop_age < HOP_TIME { HOP_HEIGHT * (PI * self.hop_age / HOP_TIME).sin() } else { 0.0 };
        self.waving = !self.walking && self.time % WAVE_EVERY < WAVE_FOR;
    }

    /// Eyes shut: `time mod 2.7 < 0.11`.
    pub fn blinking(&self) -> bool {
        self.time % BLINK_EVERY < BLINK_FOR
    }
}

/// Paint the puppet (dressed in `look`) into `pixmap`, feet at the
/// bottom centre. `size` is the puppet height in points ([`POPUP_SIZE`] in
/// popups); walk bounce scales with it. The eyes blink per
/// [`Puppet::blinking`]; while waving the face is happy.
/// Used by the limit snowman window in `fp-ui` and by fights.
pub fn paint(pixmap: &mut Pixmap, scale: f32, size: f64, puppet: &Puppet, look: &Look, facing_left: bool) {
    pose(pixmap, scale, size, puppet, look, facing_left, puppet.blinking());
}

fn pose(pixmap: &mut Pixmap, scale: f32, size: f64, puppet: &Puppet, look: &Look, facing_left: bool, blink: bool) {
    let width = pixmap.width() as f32 / scale;
    let mut s = Sketch::new(pixmap, scale);
    let t = puppet.time;
    let k = size.max(1.0) / RIG_HEIGHT;
    let (mut lift, mut lean) = (puppet.hop, 0.0);
    let (mut front, mut back) = (-2.15f64, 2.15f64);
    let mut dressed = *look;
    if puppet.walking {
        let a = puppet.walked / (8.0 * k);
        lift += a.sin().abs() * 3.5 * k;
        lean = 0.14 * a.sin();
        front += 0.35 * a.sin();
        back += 0.35 * a.sin();
    } else if puppet.waving {
        dressed.face = Face::Happy;
        front = -0.7 + 0.4 * (14.0 * t).sin();
    }
    let squash = 1.0 + 0.015 * (3.0 * t).sin();
    let k = k as f32;
    s.translate(width / 2.0, lift as f32);
    if facing_left {
        s.scale_by(-1.0, 1.0);
    }
    s.rotate(-lean as f32);
    s.scale_by(k, k * squash as f32);
    draw_arm_stick(&mut s, (-6.0, 28.0), back as f32);
    s.at(0.0, 11.5, 0.0, 1.0, |s| snow_ball_plain(s, 11.5, 11));
    s.at(0.0, 26.0, 0.0, 1.0, |s| puppet_body(s, 8.5, 22, &dressed));
    s.at(0.0, 38.0, 0.0, 1.0, |s| puppet_head(s, 9.0, 33, &dressed, blink));
    draw_arm_stick(&mut s, (6.0, 28.0), front as f32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Hat, Outfit};

    fn run(p: &mut Puppet, seconds: f64, target: f64) {
        for _ in 0..(seconds * 60.0) as usize {
            p.step(1.0 / 60.0, target);
        }
    }

    #[test]
    fn walks_at_100_per_second() {
        let mut p = Puppet::at(0.0);
        run(&mut p, 0.5, 1000.0);
        assert!((p.x - 50.0).abs() < 1e-6);
        assert!((p.walked - 50.0).abs() < 1e-6);
        assert!(p.walking && !p.waving);
    }

    #[test]
    fn walks_left_and_stops_on_target() {
        let mut p = Puppet::at(100.0);
        run(&mut p, 2.0, 34.0);
        assert!((p.x - 34.0).abs() < 1e-6);
        assert!(!p.walking);
        assert!((p.walked - 66.0).abs() < 1e-6);
    }

    #[test]
    fn dt_is_clamped() {
        let mut p = Puppet::at(0.0);
        p.step(10.0, 1000.0);
        assert!((p.x - 5.0).abs() < 1e-9);
        assert!((p.time - 0.05).abs() < 1e-12);
        p.step(-1.0, 1000.0);
        assert!((p.x - 5.0).abs() < 1e-9);
        p.step(f64::NAN, 1000.0);
        assert_eq!((p.x, p.time), (5.0, 0.05));
    }

    #[test]
    fn bad_targets_and_positions_stop_the_puppet() {
        let mut p = Puppet::at(10.0);
        for target in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            p.step(0.05, target);
            assert_eq!((p.x, p.walked, p.walking), (10.0, 0.0, false));
        }
        let mut q = Puppet::at(f64::NAN);
        q.step(0.05, 10.0);
        assert!(!q.walking && q.walked == 0.0 && q.time > 0.0);
    }

    #[test]
    fn small_gaps_close_without_walking() {
        // Within 0.5 of the goal the puppet still slides onto it but does not
        // count as walking (so it stands, waves and blinks normally).
        let mut p = Puppet::at(33.7);
        p.step(0.05, 34.0);
        assert!((p.x - 34.0).abs() < 1e-12);
        assert!(!p.walking && p.waving);
        assert!((p.walked - 0.3).abs() < 1e-9);
    }

    #[test]
    fn blinks_for_011_of_every_27_seconds() {
        let mut p = Puppet::at(0.0);
        let mut shut = 0;
        for _ in 0..540 {
            p.step(0.01, 0.0);
            if p.blinking() {
                shut += 1;
            }
        }
        // 5.4 s = two periods of 0.11 s.
        assert!((21..=23).contains(&shut), "{shut}");
        p.time = 2.75;
        assert!(p.blinking());
        p.time = 2.82;
        assert!(!p.blinking());
    }

    #[test]
    fn waves_16_of_every_45_seconds_while_standing() {
        let mut p = Puppet::at(10.0);
        let mut waved: f64 = 0.0;
        for _ in 0..(9.0 * 100.0) as usize {
            p.step(0.01, 10.0);
            if p.waving {
                waved += 0.01;
            }
        }
        // Two full periods: 2 * 1.6 s.
        assert!((waved - 3.2).abs() < 0.05, "{waved}");
        let mut q = Puppet::at(10.0);
        q.step(0.05, 10.0);
        assert!(q.waving);
        q.time = 2.0;
        q.step(0.01, 10.0);
        assert!(!q.waving);
    }

    #[test]
    fn hop_is_a_12_point_sine_over_a_third_of_a_second() {
        let mut p = Puppet::at(0.0);
        p.step(0.01, 0.0);
        assert_eq!(p.hop, 0.0);
        p.start_hop();
        let mut peak: f64 = 0.0;
        for _ in 0..15 {
            p.step(0.02, 0.0);
            peak = peak.max(p.hop);
        }
        assert!((peak - 12.0).abs() < 0.2, "{peak}");
        for _ in 0..5 {
            p.step(0.02, 0.0);
        }
        assert_eq!(p.hop, 0.0);
    }

    fn draw(p: &Puppet, look: &Look, left: bool) -> Pixmap {
        let mut pm = Pixmap::new(120, 120).unwrap();
        paint(&mut pm, 2.0, 44.0, p, look, left);
        pm
    }

    fn lit(pm: &Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.alpha() > 0).count()
    }

    #[test]
    fn blink_closes_dot_eyes_only() {
        let standing = Puppet { time: 2.0, ..Puppet::at(0.0) };
        assert!(!standing.waving);
        for face in Face::ALL {
            let look = Look { face, ..Look::classic() };
            let shot = |blink| {
                let mut pm = Pixmap::new(120, 120).unwrap();
                pose(&mut pm, 2.0, 44.0, &standing, &look, false, blink);
                pm
            };
            let changed = shot(true).data() != shot(false).data();
            let dot_eyes = matches!(face, Face::Classic | Face::Wink | Face::Surprised | Face::Grumpy);
            assert_eq!(changed, dot_eyes, "{face:?}");
        }
    }

    #[test]
    fn walk_bounce_scales_with_size() {
        // Same pose drawn at the popup size and at the full rig size: the
        // bigger one is the same picture scaled up (top rows, bounce included).
        let mut walker = Puppet::at(0.0);
        run(&mut walker, 0.13, 500.0);
        assert!(walker.walking);
        let top = |size: f64| {
            let mut pm = Pixmap::new(200, 200).unwrap();
            paint(&mut pm, 1.0, size, &walker, &Look::classic(), false);
            (0..200).find(|&y| (0..200).any(|x| pm.pixel(x, y).unwrap().alpha() > 0)).unwrap()
        };
        let (small, big) = (200 - top(44.0), 200 - top(116.0));
        assert!((f64::from(big) / f64::from(small) - 116.0 / 44.0).abs() < 0.1, "{small} {big}");
        // Size 0 or NaN is treated as 1 point and does not panic.
        let mut pm = Pixmap::new(10, 10).unwrap();
        paint(&mut pm, 1.0, f64::NAN, &walker, &Look::classic(), true);
        paint(&mut pm, 1.0, 0.0, &walker, &Look::classic(), true);
    }

    #[test]
    fn paints_standing_walking_and_waving_poses() {
        let look = Look { hat: Some(Hat::Santa), outfit: Some(Outfit::Puffer), ..Look::classic() };
        let standing = draw(&Puppet::at(0.0), &look, false);
        assert!(lit(&standing) > 500);
        // Feet at the bottom centre.
        assert!(standing.pixel(60, 118).unwrap().alpha() > 0);
        let mut walker = Puppet::at(0.0);
        run(&mut walker, 0.4, 500.0);
        let walking = draw(&walker, &look, false);
        assert_ne!(walking.data(), standing.data());
        let mut waver = Puppet::at(0.0);
        waver.step(0.05, 0.0);
        assert!(waver.waving);
        assert_ne!(draw(&waver, &look, false).data(), standing.data());
        // Mirrored when facing left.
        assert_ne!(draw(&Puppet::at(0.0), &look, true).data(), standing.data());
    }
}
