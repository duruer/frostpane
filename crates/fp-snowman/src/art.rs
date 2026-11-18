//! Procedural art for the dressable snowman (snowman F2, F5). Everything is
//! drawn in Y-up units of the ball radius `R`, relative to the ball centre.
//! Colours and proportions are Frostpane's own design.

use crate::{Arms, Decor, Face, Hat, Look, Outfit};
use fp_core::rng::{Lcg64, Rng};
use fp_paint::tiny_skia::{Color, Path, PathBuilder, Rect};
use fp_paint::{Sketch, Trace, hex, solid};
use std::f32::consts::PI;

pub(crate) fn c(h: u32) -> Color {
    hex(h)
}

pub(crate) fn ca(h: u32, alpha: f64) -> Color {
    solid(hex(h), alpha)
}

/// Stroke width in ball units for a width of `max(min, k * r)` pixels.
fn sw(r: f32, min: f32, k: f32) -> f32 {
    min.max(k * r) / r
}

pub(crate) fn build(f: impl FnOnce(&mut Trace)) -> Path {
    let mut t = Trace::new();
    f(&mut t);
    t.finish().expect("art paths are never empty")
}

pub(crate) fn disc(x: f32, y: f32, r: f32) -> Path {
    PathBuilder::from_circle(x, y, r).expect("art circles have a radius")
}

pub(crate) fn box_path(x: f32, y: f32, w: f32, h: f32) -> Path {
    PathBuilder::from_rect(Rect::from_xywh(x, y, w, h).expect("art boxes have a size"))
}

/// Strokes `pts` as flat-capped dashes of `on` / `off` length (user units).
/// Each dash is filled as a ribbon, because the sketch strokes always use
/// round caps, which would close the gaps between short dashes.
pub(crate) fn dash(s: &mut Sketch, pts: &[(f32, f32)], (on, off): (f32, f32), color: Color, width: f32) {
    let mut drawing = true;
    let mut left = on;
    let mut run: Vec<(f32, f32)> = pts.iter().take(1).copied().collect();
    for w in pts.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut len = (b.0 - a.0).hypot(b.1 - a.1);
        while len > 1e-6 {
            let step = len.min(left);
            let t = step / len;
            let m = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if drawing {
                run.push(m);
            }
            a = m;
            len -= step;
            left -= step;
            if left <= 1e-6 {
                if drawing {
                    ribbon(s, &run, color, width);
                }
                drawing = !drawing;
                left = if drawing { on } else { off };
                run.clear();
                run.push(a);
            }
        }
    }
    if drawing {
        ribbon(s, &run, color, width);
    }
}

/// Fills a flat-ended band of `width` along the polyline `pts`.
fn ribbon(s: &mut Sketch, pts: &[(f32, f32)], color: Color, width: f32) {
    let n = pts.len();
    let h = width / 2.0;
    let offset = |i: usize, side: f32| {
        let (p, q) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let len = dx.hypot(dy).max(1e-9);
        (pts[i].0 - dy / len * h * side, pts[i].1 + dx / len * h * side)
    };
    let mut outline: Vec<(f32, f32)> = (0..n).map(|i| offset(i, 1.0)).collect();
    outline.extend((0..n).rev().map(|i| offset(i, -1.0)));
    s.polygon(&outline, color);
}

fn cubic_points(p: [(f32, f32); 4], n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let k = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let x = k[0] * p[0].0 + k[1] * p[1].0 + k[2] * p[2].0 + k[3] * p[3].0;
            let y = k[0] * p[0].1 + k[1] * p[1].1 + k[2] * p[2].1 + k[3] * p[3].1;
            (x, y)
        })
        .collect()
}

fn arc_points(cx: f32, cy: f32, r: f32, a0: f32, a1: f32, n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// Filled heart of size `size` centred at `(x, y)`.
pub(crate) fn heart(s: &mut Sketch, x: f32, y: f32, size: f32, color: Color) {
    let k = |a: f32, b: f32| (x + a * size, y + b * size);
    let p = build(|t| {
        let tip = k(0.0, -0.5);
        t.move_to(tip.0, tip.1);
        let steps = [
            (k(-0.5, 0.15), k(-0.2, -0.25), k(-0.5, -0.1)),
            (k(0.0, 0.2), k(-0.5, 0.55), k(-0.1, 0.58)),
            (k(0.5, 0.15), k(0.1, 0.58), k(0.5, 0.55)),
            (tip, k(0.5, -0.1), k(0.2, -0.25)),
        ];
        for (end, c1, c2) in steps {
            t.curve(end.0, end.1, c1.0, c1.1, c2.0, c2.1);
        }
        t.close();
    });
    s.fill(&p, color);
}

/// Paints one ball centred on the sketch origin with radius `r` points.
pub(crate) fn draw_ball(s: &mut Sketch, r: f32, angle: f32, seed: u64, decor: Decor, look: &Look) {
    if decor == Decor::Body {
        s.at(0.0, 0.0, 0.0, r, |s| arms_back(s, r, look.arms));
    }
    s.at(0.0, 0.0, 0.0, r, |s| snow_ball(s, r, angle, seed));
    match decor {
        Decor::Plain => {}
        Decor::Head => s.at(0.0, 0.0, 0.0, r, |s| head_front(s, r, look, false)),
        Decor::Body => s.at(0.0, 0.0, 0.0, r, |s| body_front(s, r, look, true)),
    }
}

/// Snow sphere with shading, rim and specks, in units of `r`.
pub(crate) fn snow_ball(s: &mut Sketch, r: f32, angle: f32, seed: u64) {
    let body = disc(0.0, 0.0, (r - 0.5) / r);
    let tones = [c(0xFFFFFF), c(0xF5F9FF), c(0xD6E0F2), c(0xB8C7E3)];
    s.round(&body, &tones, Some(&[0.0, 0.4, 0.82, 1.0]), (-0.3, 0.35), (0.0, 0.0), 1.05);
    s.across(&body, &[ca(0x8094B8, 0.28), ca(0x8094B8, 0.0)], None, (0.0, -1.0), (0.0, 0.0));
    let rim = (r - 0.8) / r;
    s.ring(-rim, -rim, rim * 2.0, rim * 2.0, ca(0x7387A8, 0.35), 1.0 / r);
    let mut rng = Lcg64::new(seed);
    let count = 8 + (r / 5.0).floor() as usize;
    s.inside(&disc(0.0, 0.0, (r - 1.0) / r), |s| {
        s.at(0.0, 0.0, angle, 1.0, |s| {
            for i in 0..count {
                let a = rng.unit() as f32 * 2.0 * PI;
                let d = (rng.unit() as f32).sqrt() * 0.8;
                let size = 0.07 + rng.unit() as f32 * 0.11;
                let alpha = 0.1 + rng.unit() * 0.12;
                let color = if i % 3 == 0 { ca(0xFFFFFF, 0.7) } else { ca(0x99AECC, alpha) };
                s.ellipse(d * a.cos() - size, d * a.sin() - size * 0.8, size * 2.0, size * 1.6, color);
            }
        });
    });
}

const COAL: u32 = 0x212129;

/// Plain snow ball of radius `r` at the origin (puppet legs).
pub(crate) fn snow_ball_plain(s: &mut Sketch, r: f32, seed: u64) {
    s.at(0.0, 0.0, 0.0, r, |s| snow_ball(s, r, 0.0, seed));
}

/// Stick arm with a red mitten pivoting at `pivot`; `angle` 0 points up and
/// positive turns counter-clockwise.
pub(crate) fn draw_arm_stick(s: &mut Sketch, pivot: (f32, f32), angle: f32) {
    s.at(pivot.0, pivot.1, angle, 1.0, |s| {
        s.line(0.0, 1.0, 0.0, 16.0, c(0x6B4726), 1.7);
        s.circle(0.0, 18.0, 3.3, c(0xDE3340));
    });
}

/// Torso with no arms or hand item: ball plus jacket or buttons (puppet and
/// outfit chip icon).
pub(crate) fn puppet_body(s: &mut Sketch, r: f32, seed: u64, look: &Look) {
    s.at(0.0, 0.0, 0.0, r, |s| {
        snow_ball(s, r, 0.0, seed);
        body_front(s, r, look, false);
    });
}

/// Puppet head: ball plus face (eyes shut when `blink`), neck outfit and hat.
pub(crate) fn puppet_head(s: &mut Sketch, r: f32, seed: u64, look: &Look, blink: bool) {
    s.at(0.0, 0.0, 0.0, r, |s| {
        snow_ball(s, r, 0.0, seed);
        head_front(s, r, look, blink);
    });
}

/// Head-ball front art: face, neck outfit, hat.
pub(crate) fn head_front(s: &mut Sketch, r: f32, look: &Look, blink: bool) {
    face(s, r, look.face, blink);
    if let Some(o) = look.outfit.filter(|o| o.is_neck()) {
        neck(s, r, o);
    }
    if let Some(h) = look.hat {
        hat(s, r, h);
    }
}

/// Body-ball front art: jacket or buttons, then the hand item.
pub(crate) fn body_front(s: &mut Sketch, r: f32, look: &Look, hand: bool) {
    match look.outfit {
        Some(o) if !o.is_neck() => jacket(s, r, o),
        _ => {
            for y in [0.1, -0.22, -0.54] {
                s.circle(0.0, y, 0.075, c(COAL));
            }
        }
    }
    if hand {
        hand_item(s, r, look.arms);
    }
}

const EYE_L: (f32, f32) = (-0.30, 0.22);
const EYE_R: (f32, f32) = (0.16, 0.25);

fn smile(s: &mut Sketch) {
    for i in 0..5 {
        let a = -2.35 + i as f32 / 4.0 * 1.6;
        s.circle(a.cos() * 0.45 - 0.05, a.sin() * 0.42 + 0.02, 0.045, c(COAL));
    }
}

fn frown(s: &mut Sketch) {
    for i in 0..5 {
        let a = 0.75 + i as f32 / 4.0 * 1.6;
        s.circle(a.cos() * 0.45 - 0.05, -0.62 + a.sin() * 0.3, 0.045, c(COAL));
    }
}

fn cheeks(s: &mut Sketch) {
    s.ellipse(-0.66, -0.16, 0.28, 0.18, ca(0xFF738C, 0.45));
    s.ellipse(0.34, -0.12, 0.26, 0.17, ca(0xFF738C, 0.45));
}

/// One dot eye, or a closed lid (a small bowl) while `shut`.
fn eye(s: &mut Sketch, (x, y): (f32, f32), rad: f32, shut: bool, width: f32) {
    if shut {
        brow(s, (x, y + 0.03), 0.1, false, width);
    } else {
        s.circle(x, y, rad, c(COAL));
    }
}

fn eyes(s: &mut Sketch, rad: f32, shut: bool, width: f32) {
    eye(s, EYE_L, rad, shut, width);
    eye(s, EYE_R, rad, shut, width);
}

/// Brow arc around `(x, y)`: `up` is a bump, otherwise a bowl.
fn brow(s: &mut Sketch, (x, y): (f32, f32), rad: f32, up: bool, width: f32) {
    let (a0, a1) = if up { (0.2, PI - 0.2) } else { (PI + 0.2, 2.0 * PI - 0.2) };
    let n = (((a1 - a0) / (PI / 32.0)).floor() as usize).max(8);
    s.polyline(&arc_points(x, y, rad, a0, a1, n), c(COAL), width);
}

/// Face art. `shut` closes the dot eyes (the puppet's blink); faces whose
/// eyes are already closed, hidden or hearts are unchanged.
pub(crate) fn face(s: &mut Sketch, r: f32, face: Face, shut: bool) {
    let thin = sw(r, 1.0, 0.065);
    match face {
        Face::Classic => {
            eyes(s, 0.085, shut, thin);
            smile(s);
        }
        Face::Happy => {
            brow(s, (EYE_L.0, EYE_L.1 - 0.04), 0.1, true, thin);
            brow(s, (EYE_R.0, EYE_R.1 - 0.04), 0.1, true, thin);
            cheeks(s);
            let mouth = build(|t| {
                t.move_to(-0.42, -0.2).curve(0.24, -0.2, -0.36, -0.62, 0.18, -0.62).close();
            });
            s.fill(&mouth, c(COAL));
            s.inside(&mouth, |s| s.ellipse(-0.2, -0.47, 0.24, 0.11, c(0xF26B73)));
        }
        Face::Wink => {
            eye(s, EYE_L, 0.085, shut, thin);
            let (rx, ry) = EYE_R;
            s.polyline(&[(rx - 0.11, ry + 0.06), (rx + 0.04, ry), (rx - 0.11, ry - 0.06)], c(COAL), thin);
            smile(s);
            cheeks(s);
        }
        Face::Cool => {
            let lens = c(0x14141C);
            for (ex, ey) in [EYE_L, EYE_R] {
                s.rect(ex - 0.19, ey - 0.13, 0.38, 0.25, 0.08, lens);
            }
            let fw = sw(r, 1.0, 0.05);
            s.line(EYE_L.0 + 0.18, EYE_L.1 + 0.06, EYE_R.0 - 0.18, EYE_R.1 + 0.06, lens, fw);
            s.line(EYE_L.0 - 0.18, EYE_L.1 + 0.06, -0.82, 0.32, lens, fw);
            for (ex, ey) in [EYE_L, EYE_R] {
                s.line(ex - 0.11, ey + 0.05, ex - 0.03, ey - 0.04, ca(0xFFFFFF, 0.55), sw(r, 0.6, 0.03));
            }
            let smirk = build(|t| {
                t.move_to(-0.36, -0.3).quad(0.18, -0.2, -0.05, -0.42);
            });
            s.stroke(&smirk, c(COAL), sw(r, 1.0, 0.06));
        }
        Face::Surprised => {
            for (ex, ey) in [EYE_L, EYE_R] {
                eye(s, (ex, ey), 0.11, shut, thin);
                if !shut {
                    s.circle(ex + 0.035, ey + 0.04, 0.035, c(0xFFFFFF));
                }
                brow(s, (ex, ey + 0.08), 0.17, true, sw(r, 1.0, 0.045));
            }
            s.ring(-0.2, -0.5, 0.2, 0.26, c(COAL), sw(r, 1.2, 0.07));
        }
        Face::Sleepy => {
            brow(s, (EYE_L.0, EYE_L.1 + 0.03), 0.1, false, thin);
            brow(s, (EYE_R.0, EYE_R.1 + 0.03), 0.1, false, thin);
            let mouth = build(|t| {
                t.move_to(-0.2, -0.34).quad(0.02, -0.34, -0.09, -0.4);
            });
            s.stroke(&mouth, c(COAL), sw(r, 1.0, 0.055));
            for (x, y, z) in [(0.5, 0.62, 0.16), (0.78, 0.9, 0.22)] {
                s.polyline(&[(x, y + z), (x + z, y + z), (x, y), (x + z, y)], c(0x5980D9), sw(r, 1.0, 0.05));
            }
        }
        Face::Grumpy => {
            eyes(s, 0.085, shut, thin);
            let w = sw(r, 1.0, 0.07);
            s.line(EYE_L.0 - 0.15, EYE_L.1 + 0.24, EYE_L.0 + 0.12, EYE_L.1 + 0.12, c(COAL), w);
            s.line(EYE_R.0 + 0.15, EYE_R.1 + 0.24, EYE_R.0 - 0.12, EYE_R.1 + 0.12, c(COAL), w);
            frown(s);
        }
        Face::Love => {
            for (ex, ey) in [EYE_L, EYE_R] {
                heart(s, ex, ey - 0.02, 0.22, c(0xE61F40));
            }
            smile(s);
            cheeks(s);
        }
    }
    // Carrot nose, always last.
    s.polygon(&[(-0.02, 0.07), (0.82, -0.08), (-0.02, -0.10)], c(0xF5801F));
    for i in 1..=2 {
        let i = i as f32;
        s.line(0.2 * i, 0.05 - 0.03 * i, 0.2 * i - 0.03, -0.02 - 0.02 * i, ca(0xB8540F, 0.8), sw(r, 0.8, 0.025));
    }
}

fn hat(s: &mut Sketch, r: f32, hat: Hat) {
    match hat {
        Hat::TopHat => {
            let ink = c(0x1A1A21);
            s.rect(-0.5, 0.8, 1.0, 0.92, 0.0, ink);
            s.ellipse(-0.5, 1.64, 1.0, 0.16, ink);
            s.ellipse(-0.82, 0.66, 1.64, 0.26, ink);
            s.rect(-0.5, 0.9, 1.0, 0.16, 0.0, c(0xD62933));
            s.rect(-0.4, 1.1, 0.1, 0.56, 0.0, ca(0xFFFFFF, 0.14));
        }
        Hat::Bobble => {
            let dome = build(|t| {
                t.move_to(-0.9, 0.55).curve(0.9, 0.55, -0.92, 1.62, 0.92, 1.62).close();
            });
            s.fill(&dome, c(0x3373DB));
            s.inside(&dome, |s| {
                s.rect(-1.0, 0.92, 2.0, 0.12, 0.0, c(0xFFFFFF));
                for i in 0..6 {
                    s.circle(-0.62 + i as f32 * 0.25, 1.16, 0.045, c(0xF24C59));
                }
            });
            s.rect(-0.96, 0.42, 1.92, 0.28, 0.12, c(0x2457B2));
            for i in 0..9 {
                let x = -0.8 + i as f32 * 0.2;
                s.line(x, 0.46, x, 0.66, ca(0xFFFFFF, 0.25), sw(r, 0.6, 0.03));
            }
            s.circle(0.0, 1.42, 0.22, c(0xFFFFFF));
        }
        Hat::Santa => {
            let cap = build(|t| {
                t.move_to(-0.84, 0.62)
                    .curve(1.0, 1.14, -0.6, 1.72, 0.5, 1.78)
                    .curve(0.84, 0.62, 0.76, 1.3, 0.8, 1.0)
                    .close();
            });
            s.fill(&cap, c(0xD62933));
            s.rect(-0.96, 0.46, 1.92, 0.3, 0.15, c(0xFFFFFF));
            s.circle(1.02, 1.12, 0.2, c(0xFFFFFF));
        }
        Hat::Crown => {
            let gold = c(0xFAC233);
            s.polygon(
                &[(-0.62, 0.72), (-0.66, 1.42), (-0.33, 1.08), (0.0, 1.52), (0.33, 1.08), (0.66, 1.42), (0.62, 0.72)],
                gold,
            );
            s.rect(-0.63, 0.72, 1.26, 0.14, 0.0, c(0xD9941A));
            for (x, y) in [(-0.66, 1.44), (0.0, 1.54), (0.66, 1.44)] {
                s.circle(x, y, 0.07, c(0xD9941A));
            }
            s.circle(0.0, 0.98, 0.09, c(0xD62933));
            for x in [-0.36, 0.36] {
                s.circle(x, 0.95, 0.065, c(0x3373E6));
            }
        }
        Hat::Cowboy => {
            let crown = build(|t| {
                t.move_to(-0.52, 0.76)
                    .curve(-0.08, 1.4, -0.6, 1.2, -0.4, 1.46)
                    .quad(0.08, 1.4, 0.0, 1.3)
                    .curve(0.52, 0.76, 0.4, 1.46, 0.6, 1.2)
                    .close();
            });
            s.fill(&crown, c(0x996133));
            s.rect(-0.54, 0.8, 1.08, 0.14, 0.0, c(0x4C2B14));
            let brim = build(|t| {
                t.move_to(-1.18, 0.98)
                    .curve(1.18, 0.98, -0.7, 0.5, 0.7, 0.5)
                    .curve(-1.18, 0.98, 0.7, 0.76, -0.7, 0.76)
                    .close();
            });
            s.fill(&brim, c(0x855229));
        }
        // Leans slightly back (left, away from the nose); snowman Q2.
        Hat::Party => s.at(0.05, 0.82, 0.18, 1.0, |s| {
            let cone = build(|t| {
                t.lines(&[(-0.45, 0.0), (0.0, 0.9), (0.45, 0.0)]).close();
            });
            s.fill(&cone, c(0x9E4CE0));
            s.inside(&cone, |s| {
                for (x, y) in [(-0.2, 0.15), (0.15, 0.3), (-0.05, 0.5), (0.2, 0.08), (0.02, 0.72)] {
                    s.circle(x, y, 0.06, c(0xFFD640));
                }
            });
            s.circle(0.0, 0.93, 0.13, c(0xFF7399));
        }),
        Hat::Earmuffs => {
            let band = build(|t| {
                t.arc(0.0, 0.28, 0.96, 0.08, 3.06);
            });
            s.stroke(&band, c(0x4C4C5C), sw(r, 1.5, 0.1));
            s.circle(-0.97, 0.26, 0.27, c(0xFF8CB8));
            s.circle(0.95, 0.32, 0.27, c(0xFF8CB8));
            s.circle(-1.03, 0.34, 0.09, ca(0xFFFFFF, 0.35));
            s.circle(0.89, 0.40, 0.09, ca(0xFFFFFF, 0.35));
        }
        Hat::Viking => {
            for sd in [-1.0f32, 1.0] {
                let horn = build(|t| {
                    t.move_to(sd * 0.62, 0.92)
                        .curve(sd * 1.32, 1.62, sd * 1.15, 0.92, sd * 1.3, 1.2)
                        .curve(sd * 0.7, 1.18, sd * 1.1, 1.3, sd * 0.95, 1.18)
                        .close();
                });
                s.fill(&horn, c(0xF5EBCC));
            }
            let helm = build(|t| {
                t.move_to(-0.84, 0.66).curve(0.84, 0.66, -0.86, 1.6, 0.86, 1.6).close();
            });
            s.fill(&helm, c(0x9EA8B8));
            s.ellipse(-0.5, 1.05, 0.3, 0.25, ca(0xFFFFFF, 0.25));
            s.rect(-0.9, 0.56, 1.8, 0.22, 0.08, c(0x8C6133));
            for i in 0..5 {
                s.circle(-0.64 + i as f32 * 0.32, 0.67, 0.045, c(0xD9BF73));
            }
        }
    }
}

fn neck(s: &mut Sketch, r: f32, outfit: Outfit) {
    match outfit {
        Outfit::Scarf => scarf(s, r, (0xD62933, 0xFADBDB), sw(r, 1.0, 0.05), &[-0.82], &[0.2]),
        Outfit::StripedScarf => {
            scarf(s, r, (0x1F8C52, 0xFAFAFF), sw(r, 0.6, 0.05), &[-0.89, -0.80, -0.71], &[0.2, 0.38, 0.56])
        }
        Outfit::BowTie => {
            for sd in [-1.0f32, 1.0] {
                let wing = build(|t| {
                    t.move_to(0.0, -0.93).line(sd * 0.36, -0.78).quad(sd * 0.36, -1.1, sd * 0.42, -0.94).close();
                });
                s.fill(&wing, c(0xD11A2E));
            }
            for (x, y) in [(-0.22, -0.88), (-0.24, -1.0), (0.2, -0.86), (0.25, -0.99)] {
                s.circle(x, y, 0.035, ca(0xFFFFFF, 0.75));
            }
            s.rect(-0.09, -1.02, 0.18, 0.18, 0.05, c(0x990D1F));
        }
        _ => {
            let blue = c(0x294CB8);
            s.polygon(&[(-0.12, -0.86), (0.12, -0.86), (0.08, -1.04), (-0.08, -1.04)], blue);
            let blade = build(|t| {
                t.lines(&[(-0.08, -1.04), (0.08, -1.04), (0.2, -1.72), (0.0, -1.88), (-0.2, -1.72)]).close();
            });
            s.fill(&blade, blue);
            s.inside(&blade, |s| {
                for i in 0..5 {
                    let y = -1.08 - 0.18 * i as f32;
                    s.line(-0.25, y, 0.25, y - 0.14, c(0xFAC233), sw(r, 1.0, 0.06));
                }
            });
        }
    }
}

/// Scarf in `(main, stripe)` colours; stripes are `thick` ball units high.
fn scarf(s: &mut Sketch, r: f32, (main, stripe): (u32, u32), thick: f32, band: &[f32], tail: &[f32]) {
    let px = 1.0 / r;
    s.inside(&disc(0.0, 0.0, 1.0 + px), |s| {
        s.rect(-1.0 - 2.0 * px, -0.94, 2.0 + 4.0 * px, 0.3, 0.0, c(main));
        for &y in band {
            s.rect(-1.0 - 2.0 * px, y, 2.0 + 4.0 * px, thick, 0.0, c(stripe));
        }
    });
    s.at(0.32, -0.68, 0.16, 1.0, |s| {
        s.rect(-0.13, -0.82, 0.26, 0.82, 0.05, c(main));
        for &up in tail {
            s.rect(-0.13, -0.82 + up, 0.26, thick, 0.0, c(stripe));
        }
        for k in 0..4 {
            let x = -0.13 + 0.26 * (k as f32 + 0.5) / 4.0;
            s.line(x, -0.82 + px, x, -0.92, c(main), sw(r, 1.0, 0.04));
        }
    });
}

/// Jacket outfits. The gold chain is neither clipped nor shaded; the cloth
/// jackets are clipped to the ball and get the spherical shading overlay.
fn jacket(s: &mut Sketch, r: f32, outfit: Outfit) {
    if outfit == Outfit::GoldChain {
        gold_chain(s, r);
        return;
    }
    let body = disc(0.0, 0.0, (r - 0.5) / r);
    s.inside(&body, |s| {
        match outfit {
            Outfit::Sweater => sweater(s, r),
            Outfit::Puffer => puffer(s, r),
            _ => tuxedo(s, r),
        }
        let shade = [ca(0xFFFFFF, 0.22), ca(0xFFFFFF, 0.0), ca(0x00001A, 0.32)];
        s.round(&body, &shade, Some(&[0.0, 0.5, 1.0]), (-0.3, 0.35), (0.0, 0.0), 1.05);
    });
}

fn sweater(s: &mut Sketch, r: f32) {
    s.rect(-1.1, -0.72, 2.2, 1.82, 0.0, c(0xC71F2B));
    s.rect(-1.1, -0.72, 2.2, 0.16, 0.0, c(0x990F1F));
    let w = sw(r, 1.0, 0.07);
    let zig = |base: f32, peak: f32| -> Vec<(f32, f32)> {
        (0..=10).map(|i| (-1.0 + 0.2 * i as f32, if i % 2 == 0 { base } else { peak })).collect()
    };
    s.polyline(&zig(0.30, 0.42), c(0xFFFFFF), w);
    s.polyline(&zig(-0.10, -0.22), c(0xFFFFFF), w);
    for i in 0..5 {
        s.circle(-0.8 + 0.4 * i as f32, 0.10, 0.04, c(0xFFFFFF));
    }
    for i in 0..4 {
        s.circle(-0.6 + 0.4 * i as f32, -0.38, 0.05, c(0x339959));
    }
}

fn puffer(s: &mut Sketch, r: f32) {
    s.rect(-1.1, -0.8, 2.2, 1.9, 0.0, c(0x298FDB));
    for y in [0.5, 0.12, -0.26, -0.62] {
        let seam = build(|t| {
            t.move_to(-1.1, y).quad(1.1, y, 0.0, y - 0.12);
        });
        s.stroke(&seam, ca(0x145C9E, 0.8), sw(r, 1.0, 0.045));
    }
    s.line(0.05, 0.9, 0.05, -0.8, c(0xD9E0EB), sw(r, 1.0, 0.05));
    s.rect(0.02, 0.38, 0.12, 0.18, 0.0, c(0xD9E0EB));
}

fn tuxedo(s: &mut Sketch, r: f32) {
    s.rect(-1.1, -1.1, 2.2, 2.2, 0.0, c(0x1A1A21));
    s.polygon(&[(-0.36, 1.0), (0.36, 1.0), (0.0, -0.3)], c(0xFFFFFF));
    let lapel = sw(r, 1.0, 0.045);
    s.line(-0.44, 0.9, -0.04, -0.34, c(0x4C4C5C), lapel);
    s.line(0.44, 0.9, 0.04, -0.34, c(0x4C4C5C), lapel);
    for y in [0.30, 0.06] {
        s.circle(0.0, y, 0.045, c(COAL));
    }
    s.circle(-0.52, 0.34, 0.09, c(0xD91A33));
    for sd in [-1.0f32, 1.0] {
        s.polygon(&[(0.0, 0.62), (sd * 0.26, 0.74), (sd * 0.26, 0.50)], c(COAL));
    }
    s.circle(0.0, 0.62, 0.06, c(COAL));
    for y in [-0.5, -0.74] {
        s.circle(-0.18, y, 0.05, c(COAL));
    }
}

fn gold_chain(s: &mut Sketch, r: f32) {
    let pts = cubic_points([(-0.5, 0.72), (-0.4, 0.0), (0.4, 0.0), (0.5, 0.72)], 48);
    dash(s, &pts, (sw(r, 1.5, 0.07), sw(r, 0.8, 0.035)), c(0xFAC233), sw(r, 1.2, 0.07));
    s.circle(0.0, 0.2, 0.17, c(0xFAC233));
    s.circle(0.0, 0.2, 0.1, c(0xD9941A));
    s.circle(-0.06, 0.27, 0.035, ca(0xFFFFFF, 0.6));
    for y in [-0.22, -0.54] {
        s.circle(0.0, y, 0.075, c(COAL));
    }
}

fn twig(s: &mut Sketch, r: f32, from: (f32, f32), to: (f32, f32), fork: (f32, f32)) {
    let wood = c(0x6B4526);
    let w = sw(r, 1.5, 0.06);
    s.line(from.0, from.1, to.0, to.1, wood, w);
    let p = (from.0 + (to.0 - from.0) * 0.72, from.1 + (to.1 - from.1) * 0.72);
    s.line(p.0, p.1, p.0 + fork.0, p.1 + fork.1, wood, w);
}

/// Twig arms behind the body ball (always drawn; waving raises the right one).
pub(crate) fn arms_back(s: &mut Sketch, r: f32, arms: Arms) {
    twig(s, r, (-0.55, 0.1), (-1.6, 0.55), (-0.12, 0.3));
    if arms == Arms::Waving {
        twig(s, r, (0.55, 0.25), (1.28, 1.42), (0.28, 0.08));
        let wood = c(0x6B4526);
        let w = sw(r, 1.5, 0.06);
        s.line(1.28, 1.42, 1.14, 1.56, wood, w);
        s.line(1.28, 1.42, 1.44, 1.50, wood, w);
    } else {
        twig(s, r, (0.55, 0.1), (1.6, 0.55), (0.12, 0.3));
    }
}

fn hand_item(s: &mut Sketch, r: f32, arms: Arms) {
    match arms {
        Arms::Sticks | Arms::Waving => {}
        Arms::Mittens => {
            for sd in [-1.0f32, 1.0] {
                let x = 1.58 * sd;
                s.ellipse(x - 0.16, 0.43, 0.32, 0.36, c(0xD62933));
                let thumb = if sd > 0.0 { x - 0.26 } else { x + 0.06 };
                s.ellipse(thumb, 0.53, 0.2, 0.16, c(0xD62933));
                s.rect(x - 0.17, 0.35, 0.34, 0.14, 0.05, c(0xFFFFFF));
            }
        }
        Arms::Broom => {
            s.line(1.55, 1.35, 1.55, -0.5, c(0xB8854C), sw(r, 1.5, 0.07));
            s.polygon(&[(1.45, -0.5), (1.65, -0.5), (1.74, -1.05), (1.36, -1.05)], c(0xEBC25C));
            for i in 0..4 {
                let i = i as f32;
                s.line(1.47 + i * 0.055, -0.6, 1.4 + i * 0.1, -1.02, ca(0xB88C33, 0.8), sw(r, 0.6, 0.025));
            }
            s.rect(1.44, -0.6, 0.22, 0.1, 0.0, c(0xD62933));
        }
        Arms::CandyCane => {
            let mut pts = vec![(1.6, -0.45), (1.6, 1.08)];
            pts.extend(arc_points(1.42, 1.08, 0.18, 0.0, PI, 16));
            pts.push((1.24, 0.95));
            let w = sw(r, 2.0, 0.13);
            s.polyline(&pts, c(0xFFFFFF), w);
            let d = sw(r, 1.5, 0.1);
            dash(s, &pts, (d, d), c(0xD62933), w);
        }
        Arms::Cocoa => {
            for x in [1.42f32, 1.56] {
                let steam = build(|t| {
                    t.move_to(x, 0.84).curve(x, 1.24, x + 0.08, 0.96, x - 0.08, 1.1);
                });
                s.stroke(&steam, ca(0xFFFFFF, 0.75), sw(r, 0.8, 0.035));
            }
            s.ring(1.22, 0.5, 0.16, 0.2, c(0xD93340), sw(r, 1.0, 0.05));
            s.rect(1.3, 0.4, 0.36, 0.4, 0.05, c(0xD93340));
            s.ellipse(1.32, 0.74, 0.32, 0.1, c(0x6B3D1F));
            s.rect(1.3, 0.52, 0.36, 0.06, 0.0, c(0xFFFFFF));
        }
        Arms::Gift => {
            s.rect(1.26, 0.38, 0.44, 0.42, 0.0, c(0x29995C));
            s.rect(1.44, 0.38, 0.08, 0.42, 0.0, c(0xD62933));
            s.rect(1.26, 0.55, 0.44, 0.08, 0.0, c(0xD62933));
            s.ring(1.30, 0.80, 0.18, 0.14, c(0xD62933), sw(r, 1.0, 0.06));
            s.ring(1.48, 0.80, 0.18, 0.14, c(0xD62933), sw(r, 1.0, 0.06));
        }
        Arms::Flowers => {
            let heads = [((1.42, 1.02), 0xFF73A6), ((1.62, 0.94), 0xFFD140), ((1.52, 1.22), 0xE6334C)];
            for ((x, y), _) in heads {
                s.line(1.58, 0.5, x, y, c(0x338C40), sw(r, 1.0, 0.04));
            }
            for ((x, y), tone) in heads {
                for k in 0..5 {
                    let a = k as f32 * 2.0 * PI / 5.0;
                    s.circle(x + 0.07 * a.cos(), y + 0.07 * a.sin(), 0.06, c(tone));
                }
                s.circle(x, y, 0.04, c(0xFFF299));
            }
            s.polygon(&[(1.42, 0.72), (1.74, 0.72), (1.6, 0.36)], c(0xFAD9E6));
        }
    }
}
