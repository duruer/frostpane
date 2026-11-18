//! Small math helpers shared by every simulation (EffectMath, snow-sim 5;
//! easing conventions from characters 0.1 and snowman-fight).

use std::f64::consts::PI;

/// `t` clamped to `[0, 1]`.
pub fn clamp01(t: f64) -> f64 {
    t.clamp(0.0, 1.0)
}

/// Smoothstep of `x` clamped to `[0, 1]`: `3t^2 - 2t^3` ("Ease").
pub fn smooth(x: f64) -> f64 {
    let t = clamp01(x);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL-style smoothstep between edges.
pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    smooth((x - e0) / (e1 - e0))
}

/// Arc `sin(pi * clamp01(x))` (hops).
pub fn arc(x: f64) -> f64 {
    (PI * clamp01(x)).sin()
}

/// `a + (b - a) * clamp01(p)`.
pub fn mix(a: f64, b: f64, p: f64) -> f64 {
    a + (b - a) * clamp01(p)
}

/// Unclamped linear interpolation.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Exponential smoothing step: `x += (target - x) * (1 - exp(-k dt))`.
pub fn approach(x: f64, target: f64, k: f64, dt: f64) -> f64 {
    x + (target - x) * (1.0 - (-k * dt).exp())
}

/// Quintic ease-out `1 - (1 - t)^5` ("Snappy").
pub fn quintic_out(t: f64) -> f64 {
    1.0 - (1.0 - clamp01(t)).powi(5)
}

/// `sin(pi * (t mod every) / lasting)` while `(t mod every) < lasting`, else 0.
pub fn pulse(t: f64, every: f64, lasting: f64) -> f64 {
    let m = t.rem_euclid(every);
    if m < lasting { (PI * m / lasting).sin() } else { 0.0 }
}

/// Swap-remove (`Take`): order is not preserved.
pub fn take<T>(v: &mut Vec<T>, i: usize) -> T {
    v.swap_remove(i)
}

/// .NET-style colour byte: round-half-to-even of `v * 255`, clamped.
pub fn color_byte(v: f64) -> u8 {
    (clamp01(v) * 255.0).round_ties_even() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(clamp01(2.0), 1.0);
        assert_eq!(smooth(-1.0), 0.0);
        assert_eq!(smooth(0.5), 0.5);
        assert_eq!(smoothstep(0.0, 2.0, 1.0), 0.5);
        assert!(arc(0.5) > 0.999);
        assert_eq!(mix(0.0, 10.0, 2.0), 10.0);
        assert_eq!(lerp(0.0, 10.0, 2.0), 20.0);
        assert!((approach(0.0, 1.0, 1e9, 1.0) - 1.0).abs() < 1e-12);
        assert_eq!(quintic_out(1.0), 1.0);
        assert!(pulse(0.3, 5.0, 0.6) > 0.99);
        assert_eq!(pulse(1.0, 5.0, 0.6), 0.0);
        let mut v = vec![1, 2, 3];
        assert_eq!(take(&mut v, 0), 1);
        assert_eq!(v, vec![3, 2]);
        assert_eq!(color_byte(0.7), 0xB2);
        assert_eq!(color_byte(0.3), 0x4C);
        assert_eq!(color_byte(0.5), 0x80);
    }
}
