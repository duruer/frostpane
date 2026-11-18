//! Procedural tree art (trees 3.17, own design following the proportions of
//! the spec). All drawing is in Y-up points with the origin at the foot.

use crate::look::{Topper, TreeLook, Trim};
use crate::shape::{Patch, Tier, TreeShape, perch};
use fp_paint::tiny_skia::{Color, Path, Pixmap};
use fp_paint::{Sketch, Trace, rgb, rgba};
use std::f32::consts::{FRAC_PI_2, PI};

const BULB_TONES: [(f64, f64, f64); 5] =
    [(1.0, 0.86, 0.55), (1.0, 0.62, 0.42), (1.0, 0.93, 0.72), (0.98, 0.5, 0.48), (1.0, 0.8, 0.4)];
const GOLD_A: (f64, f64, f64) = (0.98, 0.8, 0.38);
const GOLD_B: (f64, f64, f64) = (0.93, 0.68, 0.26);
const RED: (f64, f64, f64) = (0.89, 0.24, 0.3);
const BAUBLE_TINTS: [(f64, f64, f64); 4] = [(0.87, 0.2, 0.27), (0.98, 0.76, 0.3), (0.6, 0.8, 0.97), (0.97, 0.56, 0.65)];
const SNOW_SHADE: (f64, f64, f64) = (0.78, 0.85, 0.95);
const SNOW_WHITE: (f64, f64, f64) = (0.99, 0.995, 1.0);

fn c(t: (f64, f64, f64)) -> Color {
    rgb(t.0, t.1, t.2)
}

fn ca(t: (f64, f64, f64), a: f64) -> Color {
    rgba(t.0, t.1, t.2, a)
}

fn f(v: f64) -> f32 {
    v as f32
}

// ---- layouts (pure, f64) ----

/// A string-light bulb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bulb {
    pub x: f64,
    pub y: f64,
    pub tone: usize,
    pub group: u8,
}

/// A hanging ornament placement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hung {
    pub trim: Trim,
    pub x: f64,
    pub y: f64,
    pub turn: f32,
    pub tint: usize,
}

/// Points of the swag on tier `i`.
fn swag(shape: &TreeShape, i: u32, share: f64, dip: f64, spacing: f64) -> Vec<(f64, f64)> {
    let t = shape.tier(i);
    let next_hem = if i + 1 < shape.tiers { shape.tier(i + 1).hem } else { t.apex - 0.4 * t.height() };
    let span = next_hem - t.hem;
    let y0 = t.hem + span * share;
    let hw = 0.9 * t.width_at(y0);
    let count = ((2.0 * hw / spacing) as i64).max(3);
    (0..=count)
        .map(|j| {
            let u = j as f64 / count as f64;
            (-hw + 2.0 * hw * u, y0 - span * dip * (1.0 - (2.0 * u - 1.0).powi(2)))
        })
        .collect()
}

/// String-light bulbs, tier by tier.
pub fn bulbs(shape: &TreeShape) -> Vec<Bulb> {
    let mut out = Vec::new();
    for i in 0..shape.tiers {
        for (x, y) in swag(shape, i, 0.42, 0.3, shape.unit() * 1.7) {
            out.push(Bulb { x, y, tone: (i as usize + out.len()) % 5, group: (i % 2) as u8 });
        }
    }
    out
}

/// Tinsel bead positions.
pub fn garland(shape: &TreeShape) -> Vec<(f64, f64)> {
    (0..shape.tiers).flat_map(|i| swag(shape, i, 0.74, 0.28, 1.7 * 0.17 * shape.unit())).collect()
}

/// Hanging slots, tier by tier, row 0 then row 1.
pub fn slots(shape: &TreeShape) -> Vec<(f64, f64)> {
    let u = shape.unit();
    let mut out = Vec::new();
    for i in 0..shape.tiers {
        let t = shape.tier(i);
        let next_hem = if i + 1 < shape.tiers { shape.tier(i + 1).hem } else { t.apex - 0.35 * t.height() };
        let span = next_hem - t.hem;
        for (r, share) in [0.12, 0.62].into_iter().enumerate() {
            let y = t.hem + span * share + 1.2 * u;
            let hw = 0.74 * t.width_at(y - u);
            let count = ((2.0 * hw / (3.6 * u)) as i64).max(i64::from(r == 0));
            if count == 0 {
                if hw > u {
                    out.push((0.0, y));
                }
                continue;
            }
            let step = 2.0 * hw / count as f64;
            let offset = if r == 1 { step / 2.0 } else { 0.0 };
            for k in 0..=count {
                let x = -hw + step * k as f64 + offset;
                if x.abs() <= hw + 0.1 {
                    out.push((x, y));
                }
            }
        }
    }
    out
}

/// Which slots carry which ornament.
pub fn hung(shape: &TreeShape, look: &TreeLook) -> Vec<Hung> {
    let types = look.hanging();
    if types.is_empty() {
        return Vec::new();
    }
    let d = (0.55 + 0.15 * (types.len() - 1) as f64).min(1.0);
    let mut out: Vec<Hung> = Vec::new();
    for (j, (x, y)) in slots(shape).into_iter().enumerate() {
        if ((j + 1) as f64 * d).floor() > (j as f64 * d).floor() {
            let n = out.len();
            let trim = types[n % types.len()];
            let turn = match (trim, n % 2) {
                (Trim::Candy, 0) => 0.14,
                (Trim::Candy, _) => -0.14,
                _ => 0.0,
            };
            out.push(Hung { trim, x, y, turn, tint: n % 4 });
        }
    }
    out
}

// ---- shapes ----

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let mut t = Trace::new();
    t.move_to(x + r, y)
        .line(x + w - r, y)
        .quad(x + w, y + r, x + w, y)
        .line(x + w, y + h - r)
        .quad(x + w - r, y + h, x + w, y + h)
        .line(x + r, y + h)
        .quad(x, y + h - r, x, y + h)
        .line(x, y + r)
        .quad(x + r, y, x, y)
        .close();
    t.finish()
}

fn oval(sk: &mut Sketch, cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
    sk.ellipse(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry, color);
}

fn star_path(cx: f32, cy: f32, outer: f32, inner: f32) -> Option<Path> {
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let a = FRAC_PI_2 + i as f32 * PI / 5.0;
            let r = if i % 2 == 0 { outer } else { inner };
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    let mut t = Trace::new();
    t.lines(&pts).close();
    t.finish()
}

fn draw_star(sk: &mut Sketch, cx: f32, cy: f32, outer: f32, inner: f32) {
    if let Some(p) = star_path(cx, cy, outer, inner) {
        sk.stroke(&p, rgb(1.0, 0.8, 0.3), 0.22 * outer);
        sk.across(
            &p,
            &[rgb(1.0, 0.94, 0.58), rgb(1.0, 0.75, 0.24)],
            None,
            (cx - 0.4 * outer, cy + outer),
            (cx + 0.4 * outer, cy - outer),
        );
    }
}

fn arc_points(cx: f32, cy: f32, r: f32, from: f32, to: f32, n: usize) -> Vec<(f32, f32)> {
    (0..=n).map(|i| from + (to - from) * i as f32 / n as f32).map(|a| (cx + r * a.cos(), cy + r * a.sin())).collect()
}

fn draw_bauble(sk: &mut Sketch, u: f32, tint: usize) {
    let base = BAUBLE_TINTS[tint % 4];
    let lighter = (base.0 + 0.2, base.1 + 0.2, base.2 + 0.2);
    let dark = (base.0 * 0.7, base.1 * 0.68, base.2 * 0.72);
    sk.rect(-0.2 * u, -0.34 * u, 0.4 * u, 0.3 * u, 0.07 * u, c((0.95, 0.8, 0.42)));
    let r = 0.78 * u;
    let cy = -0.28 * u - r;
    let mut t = Trace::new();
    t.arc(0.0, cy, r, 0.0, 2.0 * PI);
    t.close();
    if let Some(p) = t.finish() {
        sk.round(
            &p,
            &[c(lighter), c(base), c(dark)],
            Some(&[0.0, 0.55, 1.0]),
            (-0.3 * r, cy + 0.35 * r),
            (0.0, cy),
            1.15 * r,
        );
    }
    oval(sk, -0.37 * r, cy + 0.33 * r, 0.21 * r, 0.15 * r, rgba(1.0, 1.0, 1.0, 0.85));
}

fn draw_candy(sk: &mut Sketch, u: f32) {
    let w = 0.36 * u;
    let mut pts = vec![(0.42 * u, -1.9 * u), (0.42 * u, -0.5 * u)];
    pts.extend(arc_points(0.0, -0.5 * u, 0.42 * u, 0.0, PI, 10));
    sk.polyline(&pts, c((0.99, 0.97, 0.95)), w);
    // red stripes: one dot per dash period along the path
    let period = 1.667 * w;
    let mut walked = 0.0_f32;
    let mut next = 0.0_f32;
    for pair in pts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        while next < walked + len {
            let t = (next - walked) / len;
            sk.circle(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, 0.45 * w, c(RED));
            next += period;
        }
        walked += len;
    }
}

fn draw_bell(sk: &mut Sketch, top: f32, s: f32) {
    let rim = top - 1.05 * s;
    sk.circle(0.0, top - 1.1 * s, 0.12 * s, c((0.84, 0.56, 0.2)));
    let mut t = Trace::new();
    t.move_to(-0.1 * s, top)
        .curve(-0.56 * s, rim, -0.5 * s, top - 0.1 * s, -0.42 * s, top - 0.7 * s)
        .line(0.56 * s, rim)
        .curve(0.1 * s, top, 0.42 * s, top - 0.7 * s, 0.5 * s, top - 0.1 * s)
        .close();
    if let Some(p) = t.finish() {
        sk.across(&p, &[rgb(1.0, 0.88, 0.5), rgb(0.95, 0.68, 0.26)], None, (-0.56 * s, 0.0), (0.56 * s, 0.0));
    }
    sk.circle(0.0, top + 0.02 * s, 0.14 * s, c(RED));
}

fn draw_ginger(sk: &mut Sketch, u: f32) {
    let body = c((0.74, 0.47, 0.27));
    sk.circle(0.0, -0.55 * u, 0.5 * u, body);
    let w = 0.42 * u;
    sk.line(-0.62 * u, -1.05 * u, 0.62 * u, -1.05 * u, body, w);
    sk.line(0.0, -0.95 * u, 0.0, -1.5 * u, body, w);
    sk.line(0.0, -1.5 * u, -0.38 * u, -2.0 * u, body, w);
    sk.line(0.0, -1.5 * u, 0.38 * u, -2.0 * u, body, w);
    let ink = c((0.2, 0.12, 0.08));
    sk.circle(-0.17 * u, -0.5 * u, 0.065 * u, ink);
    sk.circle(0.17 * u, -0.5 * u, 0.065 * u, ink);
    let smile = arc_points(0.0, -0.62 * u, 0.18 * u, 207.0_f32.to_radians(), 333.0_f32.to_radians(), 6);
    sk.polyline(&smile, ink, 0.07 * u);
    sk.circle(0.0, -1.02 * u, 0.08 * u, c((1.0, 0.98, 0.94)));
    sk.circle(0.0, -1.3 * u, 0.08 * u, c((1.0, 0.98, 0.94)));
}

fn draw_bow(sk: &mut Sketch, y: f32, s: f32) {
    let dark = c((0.72, 0.14, 0.22));
    for side in [-1.0_f32, 1.0] {
        sk.polygon(
            &[
                (0.0, y),
                (0.12 * s * side, y - 0.1 * s),
                (0.5 * s * side, y - 0.75 * s),
                (0.28 * s * side, y - 0.75 * s),
            ],
            dark,
        );
        let mut t = Trace::new();
        t.move_to(0.0, y).curve(0.0, y, 0.95 * s * side, y + 0.62 * s, 0.95 * s * side, y - 0.62 * s).close();
        if let Some(p) = t.finish() {
            sk.fill(&p, c((0.88, 0.22, 0.28)));
        }
    }
    sk.circle(0.0, y, 0.17 * s, dark);
}

fn draw_hanging(sk: &mut Sketch, trim: Trim, u: f32, tint: usize) {
    match trim {
        Trim::Baubles => draw_bauble(sk, u, tint),
        Trim::Candy => draw_candy(sk, u),
        Trim::Bells => draw_bell(sk, -0.1 * u, 1.5 * u),
        Trim::Gingerbread => draw_ginger(sk, u),
        Trim::Bows => draw_bow(sk, -0.75 * u, 1.25 * u),
        // Lights and Tinsel are swags, never hung
        Trim::Stars | Trim::Lights | Trim::Tinsel => draw_star(sk, 0.0, -0.95 * u, 0.95 * u, 0.475 * u),
    }
}

fn draw_topper(sk: &mut Sketch, topper: Topper, s: f32) {
    match topper {
        Topper::Star => {
            sk.halo(0.0, 0.82 * s, 0.9 * s, rgba(1.0, 0.84, 0.4, 0.7));
            draw_star(sk, 0.0, 0.82 * s, 0.78 * s, 0.39 * s);
        }
        Topper::Snowflake => {
            let arm = 0.8 * s;
            let pale = rgb(0.82, 0.92, 1.0);
            for k in 0..6 {
                let a = FRAC_PI_2 + k as f32 * PI / 3.0;
                let tip = (arm * a.cos(), 0.9 * s + arm * a.sin());
                sk.line(0.0, 0.9 * s, tip.0, tip.1, pale, 0.13 * s);
                let from = (0.62 * tip.0, 0.9 * s + 0.62 * arm * a.sin());
                for d in [-0.75_f32, 0.75] {
                    let b = a + d;
                    sk.line(from.0, from.1, from.0 + 0.3 * arm * b.cos(), from.1 + 0.3 * arm * b.sin(), pale, 0.13 * s);
                }
            }
        }
        Topper::Bow => draw_bow(sk, 0.75 * s, 0.95 * s),
        Topper::Bell => sk.at(0.0, 1.5 * s, 0.0, 1.0, |sk| draw_bell(sk, 0.0, 1.25 * s)),
        Topper::Heart => {
            let mut t = Trace::new();
            t.move_to(0.0, 0.1 * s)
                .curve(0.0, 1.15 * s, -1.2 * s, 0.8 * s, -0.55 * s, 1.62 * s)
                .curve(0.0, 0.1 * s, 0.55 * s, 1.62 * s, 1.2 * s, 0.8 * s)
                .close();
            if let Some(p) = t.finish() {
                sk.across(&p, &[rgb(0.98, 0.42, 0.5), rgb(0.84, 0.16, 0.28)], None, (0.0, 1.5 * s), (0.0, 0.0));
            }
        }
        Topper::Spire => {
            sk.polygon(&[(-0.1 * s, 1.05 * s), (0.0, 1.95 * s), (0.1 * s, 1.05 * s)], c(GOLD_A));
            sk.at(0.0, 1.15 * s, 0.0, 1.0, |sk| draw_bauble(sk, 0.72 * s, 0));
        }
    }
}

fn tier_path(t: &Tier) -> Option<Path> {
    let (half, hem, apex) = (f(t.half), f(t.hem), f(t.apex));
    let h = apex - hem;
    let cc = 0.12 * half;
    let yb = hem + 0.05 * h;
    let mut tr = Trace::new();
    tr.move_to(-0.7 * cc, apex - 0.5 * cc).quad(0.7 * cc, apex - 0.5 * cc, 0.0, apex + 0.25 * cc).curve(
        half,
        yb,
        0.42 * half,
        apex - 0.42 * h,
        0.98 * half,
        hem + 0.32 * h,
    );
    let n = ((half / 17.0) as i32).max(3);
    for k in 0..n {
        let x_prev = half - 2.0 * half * k as f32 / n as f32;
        let x_next = half - 2.0 * half * (k + 1) as f32 / n as f32;
        let d = x_prev - x_next;
        tr.curve(x_next, yb, x_prev + 0.04 * d, yb - 0.46 * d, x_next - 0.04 * d, yb - 0.46 * d);
    }
    tr.curve(-0.7 * cc, apex - 0.5 * cc, -0.98 * half, hem + 0.32 * h, -0.42 * half, apex - 0.42 * h).close();
    tr.finish()
}

fn draw_body(sk: &mut Sketch, shape: &TreeShape) {
    let th = f(shape.trunk_half());
    if let Some(p) = rrect(-th, -3.0, 2.0 * th, f(shape.trunk_top()) + 14.0, 0.4 * th) {
        sk.across(&p, &[rgb(0.68, 0.46, 0.29), rgb(0.54, 0.34, 0.2)], None, (-th, 0.0), (th, 0.0));
    }
    for (i, t, p) in shape.tier_list().iter().enumerate().filter_map(|(i, t)| tier_path(t).map(|p| (i, t, p))) {
        let (half, hem, apex) = (f(t.half), f(t.hem), f(t.apex));
        let h = apex - hem;
        if i > 0 {
            sk.at(0.0, -2.5, 0.0, 1.0, |sk| sk.fill(&p, rgba(0.03, 0.22, 0.17, 0.35)));
        }
        sk.fill(&p, rgb(0.16, 0.52, 0.38));
        sk.inside(&p, |sk| {
            let stops = [rgb(0.5, 0.84, 0.56), rgb(0.26, 0.67, 0.45), rgb(0.15, 0.5, 0.38)];
            sk.across(&p, &stops, Some(&[0.0, 0.55, 1.0]), (0.0, apex), (0.0, hem - 0.05 * h));
            let spot = (-0.3 * half, hem + 0.62 * h);
            sk.round(&p, &[rgba(1.0, 1.0, 0.9, 0.3), rgba(1.0, 1.0, 0.9, 0.0)], None, spot, spot, 0.75 * half);
        });
    }
}

fn draw_snow(sk: &mut Sketch, patches: &[Patch]) {
    let on: Vec<&Patch> = patches.iter().filter(|p| p.depth > 0.03).collect();
    // shadow, shaded pass, then the white pass lifted by 0.22 r and shrunk to 0.84
    let passes = [
        ((0.08, 0.14, 0.3), 0.0, -1.0, 1.0, 0.2),
        (SNOW_SHADE, 0.0, 0.0, 1.0, 1.0),
        (SNOW_WHITE, 0.22, 0.0, 0.84, 1.0),
    ];
    for (color, lift, drop, shrink, alpha) in passes {
        for p in &on {
            let r = f(p.radius());
            let dy0 = f(lift) * r + f(drop);
            sk.at(f(p.x), f(p.y), f(p.angle), 1.0, |sk| {
                for (dx, dy, k) in [(-0.82, -0.08, 0.64), (0.0, 0.08, 0.86), (0.8, -0.1, 0.6)] {
                    sk.circle(dx * r, dy * r + dy0, k * r * f(shrink), ca(color, alpha));
                }
            });
        }
    }
}

fn draw_cap(sk: &mut Sketch, shape: &TreeShape, topper: Option<Topper>, cap: f64) {
    if cap <= 0.04 {
        return;
    }
    let (px, py, wide) = perch(topper, shape.topper_size());
    let py = py + shape.topper_origin();
    let rx = f(wide * (0.35 + 0.65 * cap));
    let ry = rx / 2.0;
    let (px, py) = (f(px), f(py));
    let w = c(SNOW_WHITE);
    oval(sk, px, py + 0.6 * ry, rx, ry, w);
    oval(sk, px - 0.1 * rx, py + 0.9 * ry, 0.6 * rx, ry, w);
}

fn draw_decor(sk: &mut Sketch, shape: &TreeShape, look: &TreeLook) {
    let u = f(shape.unit());
    if look.has(Trim::Tinsel) {
        for (i, (x, y)) in garland(shape).into_iter().enumerate() {
            let bead = c(if i % 2 == 0 { GOLD_A } else { GOLD_B });
            sk.circle(f(x), f(y), 0.17 * u, bead);
            sk.circle(f(x) - 0.3 * 0.17 * u, f(y) + 0.3 * 0.17 * u, 0.3 * 0.17 * u, rgba(1.0, 0.96, 0.8, 0.8));
        }
    }
    if look.has(Trim::Lights) {
        for b in bulbs(shape) {
            let t = BULB_TONES[b.tone];
            sk.circle(f(b.x), f(b.y), 0.2 * u, rgb(t.0 * 0.82, t.1 * 0.74, t.2 * 0.6));
        }
    }
    for h in hung(shape, look) {
        sk.at(f(h.x), f(h.y), h.turn, 1.0, |sk| draw_hanging(sk, h.trim, u, h.tint));
    }
    if let Some(t) = look.topper {
        sk.at(0.0, f(shape.topper_origin()), 0.0, 1.0, |sk| draw_topper(sk, t, f(shape.topper_size())));
    }
}

fn draw_lit(sk: &mut Sketch, shape: &TreeShape, lit: u8) {
    let u = f(shape.unit());
    for b in bulbs(shape).into_iter().filter(|b| b.group == lit) {
        let t = BULB_TONES[b.tone];
        sk.halo(f(b.x), f(b.y), 4.0 * 0.27 * u, ca(t, 0.65));
        sk.circle(f(b.x), f(b.y), 0.27 * u, rgb(t.0 + 0.3, t.1 + 0.3, t.2 + 0.3));
    }
}

fn draw_hit(sk: &mut Sketch, shape: &TreeShape, has_topper: bool) {
    let ink = rgba(1.0, 1.0, 1.0, 1.0 / 255.0);
    let (top, th) = (f(shape.trunk_top()), f(shape.trunk_half()) + 12.0);
    sk.polygon(&[(-th, -12.0), (th, -12.0), (th, top), (-th, top)], ink);
    let (half, wy) = (f(shape.half), top - 2.0 * f(shape.unit()));
    sk.polygon(&[(-half, wy), (half, wy), (half, top), (-half, top)], ink);
    for t in shape.tier_list() {
        let (h, hem, apex) = (f(t.half), f(t.hem), f(t.apex));
        sk.polygon(
            &[
                (-h - 12.0, hem - 12.0),
                (h + 12.0, hem - 12.0),
                (h + 12.0, hem),
                (12.0, apex),
                (12.0, apex + 12.0),
                (-12.0, apex + 12.0),
                (-12.0, apex),
                (-h - 12.0, hem),
            ],
            ink,
        );
    }
    if has_topper {
        let (s, height) = (f(shape.topper_size()), f(shape.height));
        let (w, y0, y1) = (s * 1.4 + 12.0, height - 12.0, height + 2.0 * s + 12.0);
        sk.polygon(&[(-w, y0), (w, y0), (w, y1), (-w, y1)], ink);
    }
}

/// Everything needed to draw one tree.
pub struct TreePaint<'a> {
    pub shape: &'a TreeShape,
    pub look: &'a TreeLook,
    pub patches: &'a [Patch],
    pub cap: f64,
    /// Which blink group is lit (0 or 1).
    pub lit: u8,
    /// Uniform visual scale about the foot (growth).
    pub grow: f64,
    /// Rotation about the foot in radians (Y-up, counter-clockwise).
    pub tilt: f64,
}

fn canvas(w: f64, h: f64, scale: f32) -> Option<Pixmap> {
    Pixmap::new((w * f64::from(scale)).ceil() as u32, (h * f64::from(scale)).ceil() as u32)
}

/// Draws the whole window canvas: hit layer, body, snow, decor, lit bulbs, cap.
pub fn paint_tree(p: &TreePaint, scale: f32) -> Option<Pixmap> {
    let (w, h) = p.shape.canvas();
    let mut pm = canvas(w, h, scale)?;
    let mut sk = Sketch::new(&mut pm, scale);
    sk.translate(f(p.shape.pad()), 8.0);
    sk.rotate(f(p.tilt));
    sk.scale_by(f(p.grow), f(p.grow));
    draw_hit(&mut sk, p.shape, p.look.topper.is_some());
    draw_body(&mut sk, p.shape);
    draw_snow(&mut sk, p.patches);
    draw_decor(&mut sk, p.shape, p.look);
    if p.look.has(Trim::Lights) {
        draw_lit(&mut sk, p.shape, p.lit);
    }
    draw_cap(&mut sk, p.shape, p.look.topper, p.cap);
    drop(sk);
    Some(pm)
}

/// Style card chip icon for a topper (34 x 34 points).
pub fn topper_icon(topper: Topper, scale: f32) -> Option<Pixmap> {
    let mut pm = canvas(34.0, 34.0, scale)?;
    let mut sk = Sketch::new(&mut pm, scale);
    sk.at(17.0, 4.0, 0.0, 1.0, |sk| draw_topper(sk, topper, 15.0));
    drop(sk);
    Some(pm)
}

/// Style card chip icon for a trim (34 x 34 points).
pub fn trim_icon(trim: Trim, scale: f32) -> Option<Pixmap> {
    let mut pm = canvas(34.0, 34.0, scale)?;
    let mut sk = Sketch::new(&mut pm, scale);
    match trim {
        Trim::Lights => {
            for (i, (x, y)) in [(0.15, 0.62), (0.38, 0.42), (0.62, 0.42), (0.85, 0.62)].into_iter().enumerate() {
                let t = BULB_TONES[i];
                sk.halo(34.0 * x, 34.0 * y, 9.0, ca(t, 0.7));
                sk.circle(34.0 * x, 34.0 * y, 3.2, rgb(t.0 + 0.3, t.1 + 0.3, t.2 + 0.3));
            }
        }
        Trim::Tinsel => {
            for i in 0..17 {
                let t = i as f32 / 16.0;
                let (x, y) = (34.0 * (0.1 + 0.8 * t), 34.0 * (0.62 - 0.3 * (1.0 - (2.0 * t - 1.0).powi(2))));
                sk.circle(x, y, 2.1, c(if i % 2 == 0 { GOLD_A } else { GOLD_B }));
            }
        }
        Trim::Candy => sk.at(17.0, 26.0, 0.1, 1.0, |sk| draw_hanging(sk, trim, 10.0, 0)),
        _ => sk.at(17.0, 30.0, 0.1, 1.0, |sk| draw_hanging(sk, trim, 12.0, 0)),
    }
    drop(sk);
    Some(pm)
}

/// Standalone decorative tree icon (96 x 128 points, drawn in Y-down space).
pub fn little_tree(scale: f32) -> Option<Pixmap> {
    let mut pm = canvas(96.0, 128.0, scale)?;
    let mut sk = Sketch::new(&mut pm, scale);
    sk.translate(0.0, 128.0);
    sk.scale_by(1.0, -1.0);
    sk.rect(41.0, 106.0, 14.0, 22.0, 2.0, rgb(0.42, 0.27, 0.16));
    for (top, base, w, col) in [
        (56.0, 110.0, 42.0, (0.09, 0.4, 0.24)),
        (33.0, 80.0, 32.0, (0.11, 0.47, 0.27)),
        (16.0, 52.0, 22.0, (0.14, 0.54, 0.31)),
    ] {
        let tri = [(48.0, top), (48.0 + w, base), (48.0 - w, base)];
        sk.polygon(&tri, c(col));
        let mut t = Trace::new();
        t.lines(&tri).close();
        if let Some(p) = t.finish() {
            sk.stroke(&p, c(col), 5.0);
        }
    }
    let dots = [
        (40.0, 96.0),
        (58.0, 100.0),
        (30.0, 70.0),
        (64.0, 72.0),
        (48.0, 62.0),
        (40.0, 44.0),
        (56.0, 42.0),
        (48.0, 26.0),
        (48.0, 86.0),
    ];
    for (i, (x, y)) in dots.into_iter().enumerate() {
        let col = [(0.8, 0.13, 0.16), (0.95, 0.74, 0.25), (1.0, 1.0, 1.0)][i % 3];
        sk.circle(x, y, 3.5, c(col));
    }
    sk.at(48.0, 11.0, PI, 1.0, |sk| draw_star(sk, 0.0, 0.0, 10.0, 4.2));
    drop(sk);
    Some(pm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::generate_patches;

    fn opaque(pm: &Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.alpha() > 8).count()
    }

    #[test]
    fn layouts() {
        for level in 0..=5 {
            let s = TreeShape::from_level(level);
            assert!(bulbs(&s).len() >= 4 * s.tiers as usize);
            assert!(bulbs(&s).iter().all(|b| b.tone < 5 && b.group < 2));
            assert!(garland(&s).len() > bulbs(&s).len());
            assert!(!slots(&s).is_empty());
        }
        let s = TreeShape::from_level(0);
        let b = bulbs(&s);
        assert_eq!(b[0].tone, 0);
        assert_eq!(b[1].tone, 1);
        let look = TreeLook { level: 2, topper: None, trims: vec![Trim::Candy] };
        let items = hung(&TreeShape::from_level(2), &look);
        assert!(items.iter().all(|h| h.trim == Trim::Candy));
        assert!(items.iter().any(|h| h.turn > 0.0) && items.iter().any(|h| h.turn < 0.0));
        let none = TreeLook { level: 2, topper: None, trims: vec![Trim::Lights] };
        assert!(hung(&TreeShape::from_level(2), &none).is_empty());
        let two = TreeLook { level: 2, topper: None, trims: vec![Trim::Baubles, Trim::Bells] };
        let mixed = hung(&TreeShape::from_level(2), &two);
        assert!(mixed.iter().any(|h| h.trim == Trim::Baubles) && mixed.iter().any(|h| h.trim == Trim::Bells));
        let many =
            TreeLook { level: 2, topper: None, trims: vec![Trim::Baubles, Trim::Candy, Trim::Bells, Trim::Bows] };
        assert!(hung(&TreeShape::from_level(2), &many).len() > mixed.len() / 2);
    }

    #[test]
    fn slots_cover_a_centre_slot_on_small_trees() {
        let s = TreeShape::from_level(0);
        assert!(slots(&s).iter().any(|(x, _)| *x == 0.0));
    }

    #[test]
    fn full_tree_paints() {
        let look = TreeLook { level: 3, topper: Some(Topper::Heart), trims: Trim::ALL.to_vec() };
        let shape = TreeShape::from_level(3);
        let mut patches = generate_patches(&shape, 9);
        for p in &mut patches {
            p.depth = 0.5;
        }
        let tp = TreePaint { shape: &shape, look: &look, patches: &patches, cap: 1.0, lit: 0, grow: 0.9, tilt: 0.1 };
        let a = paint_tree(&tp, 1.0).unwrap();
        let (w, h) = shape.canvas();
        assert_eq!((a.width(), a.height()), (w.ceil() as u32, h.ceil() as u32));
        assert!(opaque(&a) > 5000);
        let b = paint_tree(&TreePaint { lit: 1, ..tp }, 1.0).unwrap();
        assert_ne!(a.data(), b.data());
        let big = paint_tree(&tp, 2.0).unwrap();
        assert_eq!(big.width(), (w * 2.0).ceil() as u32);
    }

    #[test]
    fn every_topper_and_trim_paints() {
        let shape = TreeShape::from_level(1);
        for t in Topper::ALL {
            let look = TreeLook { level: 1, topper: Some(t), trims: vec![] };
            let tp = TreePaint { shape: &shape, look: &look, patches: &[], cap: 0.0, lit: 0, grow: 1.0, tilt: 0.0 };
            let with = opaque(&paint_tree(&tp, 1.0).unwrap());
            let none = TreeLook { level: 1, topper: None, trims: vec![] };
            let without = opaque(&paint_tree(&TreePaint { look: &none, ..tp }, 1.0).unwrap());
            assert!(with > without, "{t:?}");
            assert!(opaque(&topper_icon(t, 1.0).unwrap()) > 20, "{t:?}");
        }
        for t in Trim::ALL {
            assert!(opaque(&trim_icon(t, 1.0).unwrap()) > 20, "{t:?}");
        }
        for t in Trim::ALL {
            let look = TreeLook { level: 1, topper: None, trims: vec![t] };
            let tp = TreePaint { shape: &shape, look: &look, patches: &[], cap: 0.0, lit: 0, grow: 1.0, tilt: 0.0 };
            let with = paint_tree(&tp, 1.0).unwrap();
            let bare = TreeLook { level: 1, topper: None, trims: vec![] };
            let without = paint_tree(&TreePaint { look: &bare, ..tp }, 1.0).unwrap();
            assert!(with.data() != without.data(), "{t:?}");
        }
    }

    #[test]
    fn hit_layer_is_nearly_transparent_and_covers_the_slop() {
        let shape = TreeShape::from_level(2);
        let look = TreeLook { level: 2, topper: Some(Topper::Star), trims: vec![] };
        let tp = TreePaint { shape: &shape, look: &look, patches: &[], cap: 0.0, lit: 0, grow: 1.0, tilt: 0.0 };
        let pm = paint_tree(&tp, 1.0).unwrap();
        // 8 pt below the foot is inside the 12 pt slop of the trunk, far left is not
        let (pad, h) = (shape.pad() as u32, pm.height());
        let at = |x: u32, y_up: u32| pm.pixel(x, h - 1 - y_up).unwrap().alpha();
        assert_eq!(at(pad + 20, 3), 1);
        assert_eq!(at(2, 3), 0);
    }

    #[test]
    fn little_tree_paints() {
        let pm = little_tree(1.0).unwrap();
        assert_eq!((pm.width(), pm.height()), (96, 128));
        assert!(opaque(&pm) > 2000);
        // the star sits at the top: Y-down, so the first rows have colour
        assert!(pm.pixel(48, 11).unwrap().alpha() > 200);
    }

    #[test]
    fn empty_canvas_is_none() {
        assert!(canvas(0.0, 5.0, 1.0).is_none());
    }
}
