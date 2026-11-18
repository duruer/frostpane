//! Small shared helpers: easing, pixmap sizing, sprite construction.

use crate::StageScreen;
use fp_core::geom::Frame;
use fp_core::scene::{HitMode, Owner, Sprite, SpriteKey, ZBand};
use fp_paint::tiny_skia::Pixmap;

/// Smoothstep with the input clamped to `0..=1`.
pub(crate) fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Eased interpolation from `a` to `b`.
pub(crate) fn ease(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * smooth(t)
}

/// A hop arc: `height * sin(pi * u)` for `u` in `0..=1`.
pub(crate) fn arc(height: f64, u: f64) -> f64 {
    height * (std::f64::consts::PI * u.clamp(0.0, 1.0)).sin()
}

/// Transparent pixmap for a `w` x `h` point box at monitor `scale`.
pub(crate) fn canvas(w: f64, h: f64, scale: f32) -> Option<Pixmap> {
    let px = |v: f64| (v * f64::from(scale)).ceil().max(1.0) as u32;
    Pixmap::new(px(w), px(h))
}

/// The screen with exactly this frame, if it is still listed.
pub(crate) fn find_screen(list: &[StageScreen], frame: Frame) -> Option<&StageScreen> {
    list.iter().find(|s| s.frame == frame)
}

/// One sprite record.
pub(crate) fn sprite(
    owner: Owner,
    id: u64,
    frame: Frame,
    (z, hit): (ZBand, HitMode),
    rev: u64,
    opacity: f64,
) -> Sprite {
    Sprite { key: SpriteKey::new(owner, id), frame, z, hit, rev, opacity: opacity as f32 }
}

/// `sin(pi * (t mod every) / lasting)` while inside the pulse, else 0.
pub(crate) fn pulse(t: f64, every: f64, lasting: f64) -> f64 {
    let m = t.rem_euclid(every);
    if m < lasting { (std::f64::consts::PI * m / lasting).sin() } else { 0.0 }
}
