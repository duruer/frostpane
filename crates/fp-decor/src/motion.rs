//! Ambient animation curves (decorations 3.6a) and the pop in/out easing.

use crate::art::Motion;
use std::f64::consts::PI;

/// Visual state of one animated part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartState {
    pub opacity: f32,
    pub sx: f32,
    pub sy: f32,
    /// Radians, counter-clockwise in Y-up.
    pub rot: f32,
    /// Y-up translation in points.
    pub ty: f32,
}

impl PartState {
    pub const REST: PartState = PartState { opacity: 1.0, sx: 1.0, sy: 1.0, rot: 0.0, ty: 0.0 };
}

/// Linear keyframes at evenly spaced times; `u` in `[0, 1]`.
pub fn evenly(vals: &[f64], u: f64) -> f64 {
    let span = (vals.len() - 1) as f64;
    let x = u.clamp(0.0, 1.0) * span;
    let i = (x.floor() as usize).min(vals.len() - 2);
    vals[i] + (vals[i + 1] - vals[i]) * (x - i as f64)
}

/// Linear keyframes at explicit times.
pub fn keyed(times: &[f64], vals: &[f64], u: f64) -> f64 {
    let i = (1..times.len()).find(|&i| u <= times[i]).unwrap_or(times.len() - 1);
    let f = ((u - times[i - 1]) / (times[i] - times[i - 1])).clamp(0.0, 1.0);
    vals[i - 1] + (vals[i] - vals[i - 1]) * f
}

fn wrap(t: f64, period: f64) -> f64 {
    t.rem_euclid(period) / period
}

/// Sway rotation in radians (Y-up, counter-clockwise positive) for the
/// loop position `u` in `[0, 1)` and the kind's sway `angle` (radians).
pub fn sway(angle: f64, u: f64) -> f64 {
    const TIMES: [f64; 7] = [0.0, 0.08, 0.17, 0.26, 0.34, 0.42, 1.0];
    const VALS: [f64; 7] = [0.0, 1.0, -0.75, 0.45, -0.2, 0.0, 0.0];
    let i = (1..TIMES.len()).find(|&i| u <= TIMES[i]).unwrap_or(TIMES.len() - 1);
    let f = ((u - TIMES[i - 1]) / (TIMES[i] - TIMES[i - 1])).clamp(0.0, 1.0);
    let eased = (1.0 - (PI * f).cos()) / 2.0;
    angle * (VALS[i - 1] + (VALS[i] - VALS[i - 1]) * eased)
}

/// State of a part loop at time `now`; `phase` desyncs items, `value` is the
/// blink delay or the glow period.
/// Each motion runs at its own low frame rate (blink 20, flicker 24, glow 12,
/// steam 15 fps).
pub fn part_state(motion: Motion, value: f64, now: f64, phase: f64) -> PartState {
    let fps = match motion {
        Motion::Blink => 20.0,
        Motion::Flicker => 24.0,
        Motion::Glow => 12.0,
        Motion::Steam => 15.0,
    };
    let now = (now * fps).floor() / fps;
    let at = |period: f64, delay: f64| wrap(now + phase * period + delay, period);
    let mut s = PartState::REST;
    match motion {
        Motion::Blink => {
            s.opacity = keyed(&[0.0, 0.42, 0.5, 0.92, 1.0], &[1.0, 1.0, 0.12, 0.12, 1.0], at(1.8, value)) as f32;
        }
        Motion::Flicker => {
            s.sy = evenly(&[1.0, 1.1, 0.93, 1.06, 0.96, 1.0], at(1.1, 0.0)) as f32;
            s.sx = evenly(&[1.0, 0.93, 1.04, 0.97, 1.0], at(0.9, 0.0)) as f32;
            s.rot = evenly(&[0.0, -0.05, 0.04, -0.03, 0.02, 0.0], at(1.7, 0.0)) as f32;
        }
        Motion::Glow => {
            s.opacity = evenly(&[0.65, 1.0, 0.75, 0.95, 0.65], at(value, 0.0)) as f32;
        }
        Motion::Steam => {
            let u = at(2.8, 0.0);
            s.opacity = evenly(&[0.0, 0.9, 0.0], u) as f32;
            s.ty = evenly(&[-2.0, 5.0], u) as f32;
        }
    }
    s
}

/// WPF-style elastic ease-out (1 oscillation, springiness 4).
pub fn elastic_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let u = 1.0 - t;
    let spring = 4.0f64;
    let expo = ((spring * u).exp() - 1.0) / (spring.exp() - 1.0);
    1.0 - expo * ((2.0 * PI + PI / 2.0) * u).sin()
}

/// Pop-in scale factor over `t` seconds (0.55 s, 0.2 -> 1).
pub fn pop_in(t: f64) -> f64 {
    0.2 + 0.8 * elastic_out(t / 0.55)
}

/// Pop-out scale factor over `t` seconds (0.2 s, 1 -> 0.1).
pub fn pop_out(t: f64) -> f64 {
    1.0 - 0.9 * (t / 0.2).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves() {
        assert_eq!(evenly(&[0.0, 1.0, 0.0], 0.25), 0.5);
        assert_eq!(evenly(&[0.0, 1.0], 1.0), 1.0);
        assert_eq!(keyed(&[0.0, 1.0], &[0.0, 2.0], 0.5), 1.0);
        assert_eq!(keyed(&[0.0, 1.0], &[0.0, 2.0], 5.0), 2.0);
        assert_eq!(sway(0.1, 0.0), 0.0);
        assert!((sway(0.1, 0.08) - 0.1).abs() < 1e-12);
        assert!((sway(0.1, 0.17) + 0.075).abs() < 1e-12);
        assert_eq!(sway(0.1, 0.9), 0.0);
        assert_eq!(sway(0.1, 2.0), 0.0);
    }

    #[test]
    fn parts() {
        // blink: lit through 0.42 of the 1.8 s period, dim near 0.5..0.92
        assert_eq!(part_state(Motion::Blink, 0.0, 0.0, 0.0).opacity, 1.0);
        assert!((part_state(Motion::Blink, 0.0, 1.8 * 0.7, 0.0).opacity - 0.12).abs() < 1e-6);
        assert!((part_state(Motion::Blink, 0.9, 0.9 * 0.0, 0.0).opacity - 0.12).abs() < 0.9);
        let f = part_state(Motion::Flicker, 0.0, 0.3, 0.5);
        assert!(f.sy > 0.8 && f.sx > 0.8 && f.rot.abs() < 0.1);
        let g = part_state(Motion::Glow, 1.7, 0.0, 0.0);
        assert!((g.opacity - 0.65).abs() < 1e-6);
        let s = part_state(Motion::Steam, 0.0, 1.4, 0.0);
        assert!((s.opacity - 0.9).abs() < 1e-6);
        assert!((s.ty - 1.5).abs() < 1e-6);
    }

    #[test]
    fn pops() {
        assert!((pop_in(0.0) - 0.2).abs() < 1e-9);
        assert!((pop_in(0.55) - 1.0).abs() < 1e-9);
        assert!((pop_out(0.0) - 1.0).abs() < 1e-9);
        assert!((pop_out(1.0) - 0.1).abs() < 1e-9);
        assert!(elastic_out(0.9) > 0.9);
    }
}
