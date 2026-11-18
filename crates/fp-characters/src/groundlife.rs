//! GroundLife (other-weather 2.5): decorative 980 x 150 vignette on the
//! settings page (sleigh, three deer, two mascots playing catch). Pure:
//! `fp-ui` calls [`paint`] at 30 fps (or once at t = 100 s when the OS has
//! animations off) and shows the pixmap.

use crate::art::{self, DEER_AT, c};
use crate::util::{ease, pulse};
use fp_paint::Sketch;
use fp_paint::tiny_skia::{Pixmap, PixmapPaint, Transform};

pub const WIDTH: f64 = 980.0;
pub const HEIGHT: f64 = 150.0;

const SLEIGH_X: f32 = 495.0;
const SIZE: f64 = 64.0;
const ENTRANCE: f64 = 1030.0;
const SPOTS: [f64; 2] = [790.0, 915.0];
const STARTS: [f64; 2] = [0.4, 1.2];
const DURATIONS: [f64; 2] = [2.6, 1.6];
const GAME_START: f64 = 4.2;
const TURN: f64 = 2.4;
const HALF: f64 = 0.4 * SIZE;

/// Mascot hop, tilt (degrees), waving flag and facing for snowman `i`.
fn mascot_state(i: usize, t: f64) -> (f64, f64, bool, f64) {
    let u = ((t - STARTS[i]) / DURATIONS[i]).clamp(0.0, 1.0);
    let sign = if i == 0 { -1.0 } else { 1.0 };
    let (mut hop, mut tilt, mut waving) = (0.0, 1.5 * (2.0 * t + 2.0 * i as f64).sin(), false);
    if t >= GAME_START {
        let turn = ((t - GAME_START) / TURN).floor();
        let phase = (t - GAME_START) / TURN - turn;
        if turn as i64 % 2 == i as i64 {
            waving = phase < 0.3;
            if phase < 0.15 {
                tilt = 8.0 * sign * phase / 0.15;
            } else if phase < 0.3 {
                tilt = ease(8.0, -6.0, (phase - 0.15) / 0.15) * sign;
            }
        } else if (0.55..0.8).contains(&phase) {
            let k = (std::f64::consts::PI * (phase - 0.55) / 0.25).sin();
            hop = 6.0 * k;
            tilt = 10.0 * k * sign;
        }
    }
    let walking = u < 1.0;
    if walking {
        hop = (9.0 * t).sin().abs() * 5.0;
        tilt = (9.0 * t).sin() * 5.0;
    }
    let facing = if walking || i == 1 { -1.0 } else { 1.0 };
    (hop, tilt, waving, facing)
}

/// Paint the scene at time `t` seconds into a `ceil(980 * scale)` by
/// `ceil(150 * scale)` pixmap (painted over, not cleared).
pub fn paint(pixmap: &mut Pixmap, scale: f32, t: f64) {
    let height = pixmap.height() as f32;
    let (still, waving) = {
        let mk = |wave: bool| {
            let w = (0.8 * SIZE * f64::from(scale)).ceil() as u32;
            let h = (SIZE * f64::from(scale)).ceil() as u32;
            let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("mascot pixmap");
            fp_snowman::paint_mascot(&mut pm, scale, SIZE, wave);
            pm
        };
        (mk(false), mk(true))
    };
    {
        let mut s = Sketch::new(pixmap, scale);
        s.translate(SLEIGH_X, 4.0);
        art::sleigh(&mut s, false, 0.0);
        for (i, x) in DEER_AT.iter().enumerate() {
            let p = i as f64 * 1.7;
            let mut legs = [0.0f32; 4];
            legs[3] = (-pulse(t + 2.0 * p, 5.0, 0.6) * 0.35) as f32;
            let bob = (-(1.6 * t + p).sin() * 0.6) as f32;
            let head = (-(0.035 * (0.9 * t + p).sin() + 0.16 * pulse(t + 3.0 * p, 7.0, 1.2))) as f32;
            s.at(*x, 0.0, 0.0, 1.0, |s| art::deer(s, legs, bob, head, false));
        }
    }
    for i in 0..2 {
        let u = ((t - STARTS[i]) / DURATIONS[i]).clamp(0.0, 1.0);
        let x = ENTRANCE + (SPOTS[i] - ENTRANCE) * (1.0 - (1.0 - u) * (1.0 - u));
        let (hop, tilt, wave, facing) = mascot_state(i, t);
        let sprite = if wave { &waving } else { &still };
        let k = f64::from(scale);
        let (cx, cy) = (HALF * k, SIZE * k);
        let left = (x - HALF) * k;
        let top = f64::from(height) - (SIZE + hop) * k;
        let tf = Transform::from_translate(left as f32, top as f32)
            .pre_rotate_at(tilt as f32, cx as f32, cy as f32)
            .pre_translate(cx as f32, 0.0)
            .pre_scale(facing as f32, 1.0)
            .pre_translate(-cx as f32, 0.0);
        let paint = PixmapPaint { quality: fp_paint::tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
        pixmap.draw_pixmap(0, 0, sprite.as_ref(), &paint, tf, None);
    }
    let mut s = Sketch::new(pixmap, scale);
    if t >= GAME_START {
        let turn = ((t - GAME_START) / TURN).floor();
        let phase = (t - GAME_START) / TURN - turn;
        if (0.15..0.55).contains(&phase) {
            let q = (phase - 0.15) / 0.4;
            let (a, b) = (SPOTS[0] + 22.0, SPOTS[1] - 22.0);
            let along = if turn as i64 % 2 == 0 { q } else { 1.0 - q };
            let x = a + (b - a) * along;
            let y = 38.0 + (std::f64::consts::PI * q).sin() * 42.0;
            s.ellipse((x - 5.0) as f32, y as f32, 10.0, 10.0, c(0xFFFFFF));
        }
    }
}
