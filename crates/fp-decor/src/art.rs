//! Procedural art for the 16 decorations (decorations 4 + 5).
//!
//! Everything is drawn in Y-up art-box points (origin bottom-left, size from
//! [`Kind::art_size`]). The designs are Frostpane's own; boxes, pivots, part
//! anchors and the animated-part list follow the spec.

use crate::Kind;
use fp_paint::tiny_skia::{Color, Path, PathBuilder, Rect, Stroke};
use fp_paint::{Sketch, Strand, Trace, rgb, rgba};

/// Which animation drives a part (decorations 3.6a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Blink,
    Flicker,
    Glow,
    Steam,
}

/// Drawing routine of a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartArt {
    CandleHalo,
    CandleFlame,
    StarHalo,
    Steam,
    /// Lit bulbs of blink group 0 or 1.
    Lit(usize),
}

/// Animated overlay (decorations 3.6a part record).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub art: PartArt,
    pub behind: bool,
    pub anchor: (f32, f32),
    pub motion: Motion,
    /// Blink delay or glow period in seconds.
    pub value: f64,
    pub solid: bool,
}

/// Animated parts of a kind, in draw order.
pub fn parts(kind: Kind) -> Vec<Part> {
    let mk = |art, behind, anchor, motion, value, solid| Part { art, behind, anchor, motion, value, solid };
    match kind {
        Kind::Lights => vec![
            mk(PartArt::Lit(0), false, (0.0, 0.0), Motion::Blink, 0.0, true),
            mk(PartArt::Lit(1), false, (0.0, 0.0), Motion::Blink, 0.9, true),
        ],
        Kind::Candle => vec![
            mk(PartArt::CandleHalo, true, (26.0, 92.0), Motion::Glow, 1.7, false),
            mk(PartArt::CandleFlame, false, (26.0, 79.0), Motion::Flicker, 0.0, true),
        ],
        Kind::Star => vec![mk(PartArt::StarHalo, true, (36.0, 36.0), Motion::Glow, 2.4, false)],
        Kind::Cocoa => vec![mk(PartArt::Steam, false, (0.0, 0.0), Motion::Steam, 0.0, false)],
        _ => Vec::new(),
    }
}

fn k(r: f64, g: f64, b: f64) -> Color {
    rgb(r, g, b)
}

fn ka(r: f64, g: f64, b: f64, a: f64) -> Color {
    rgba(r, g, b, a)
}

fn red() -> Color {
    k(0.86, 0.15, 0.20)
}
fn dark_red() -> Color {
    k(0.55, 0.05, 0.10)
}
fn light_red() -> Color {
    k(0.98, 0.42, 0.42)
}
fn gold() -> Color {
    k(0.98, 0.78, 0.30)
}
fn dark_gold() -> Color {
    k(0.74, 0.50, 0.12)
}
fn light_gold() -> Color {
    k(1.0, 0.94, 0.66)
}
fn cream() -> Color {
    k(1.0, 0.98, 0.95)
}
fn wire() -> Color {
    k(0.10, 0.24, 0.14)
}

fn path(f: impl FnOnce(&mut Trace)) -> Path {
    let mut t = Trace::new();
    f(&mut t);
    t.finish().expect("art path has points")
}

fn oval(x: f32, y: f32, w: f32, h: f32) -> Path {
    PathBuilder::from_oval(Rect::from_xywh(x, y, w, h).expect("oval box")).expect("oval path")
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Path {
    path(|t| {
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
    })
}

fn poly(points: &[(f32, f32)]) -> Path {
    path(|t| {
        t.lines(points).close();
    })
}

/// "Red cloth": horizontal shading across `x0..x1`.
fn cloth(sk: &mut Sketch, p: &Path, x0: f32, x1: f32) {
    let cols = [dark_red(), red(), light_red(), red(), dark_red()];
    sk.across(p, &cols, Some(&[0.0, 0.30, 0.45, 0.72, 1.0]), (x0, 0.0), (x1, 0.0));
}

/// "Gold metal": horizontal shading across `x0..x1`.
fn metal(sk: &mut Sketch, p: &Path, x0: f32, x1: f32) {
    let cols = [dark_gold(), gold(), light_gold(), gold(), dark_gold()];
    sk.across(p, &cols, None, (x0, 0.0), (x1, 0.0));
}

fn berry(sk: &mut Sketch, x: f32, y: f32, r: f32) {
    let cols = [k(1.0, 0.45, 0.45), k(0.88, 0.1, 0.16), k(0.55, 0.02, 0.08)];
    sk.round(
        &oval(x - r, y - r, 2.0 * r, 2.0 * r),
        &cols,
        Some(&[0.0, 0.5, 1.0]),
        (x - r * 0.3, y + r * 0.3),
        (x, y),
        r,
    );
    sk.circle(x - r * 0.35, y + r * 0.4, r * 0.25, ka(1.0, 1.0, 1.0, 0.8));
}

/// Pointed leaf from the local origin along +X, rotated by `turn`.
fn leaf(sk: &mut Sketch, x: f32, y: f32, turn: f32, len: f32, tones: [Color; 2]) {
    sk.at(x, y, turn, 1.0, |s| {
        let w = len * 0.3;
        let p = path(|t| {
            t.move_to(0.0, 0.0).quad(len, 0.0, len * 0.5, w).quad(0.0, 0.0, len * 0.5, -w).close();
        });
        s.across(&p, &tones, None, (0.0, 0.0), (len, 0.0));
        s.line(len * 0.08, 0.0, len * 0.85, 0.0, ka(0.8, 0.95, 0.8, 0.55), 0.8);
    });
}

fn holly_tones() -> [Color; 2] {
    [k(0.25, 0.65, 0.34), k(0.08, 0.38, 0.19)]
}

fn sprig(sk: &mut Sketch, x: f32, y: f32) {
    leaf(sk, x, y, 0.4, 16.0, holly_tones());
    leaf(sk, x, y, 2.74, 16.0, holly_tones());
    berry(sk, x - 2.0, y + 1.0, 3.0);
    berry(sk, x + 2.5, y + 1.5, 3.0);
    berry(sk, x, y + 4.0, 3.0);
}

fn bow(sk: &mut Sketch, x: f32, y: f32, scale: f32, fill: Color, edge: Color) {
    sk.at(x, y, 0.0, scale, |s| {
        s.ellipse(-9.0, -6.5, 18.0, 4.0, ka(0.0, 0.0, 0.0, 0.12));
        s.polygon(&[(-2.0, -1.0), (-9.0, -15.0), (-3.0, -13.0), (0.0, -4.0)], fill);
        s.polygon(&[(2.0, -1.0), (9.0, -15.0), (3.0, -13.0), (0.0, -4.0)], fill);
        s.polyline(&[(-9.0, -15.0), (-3.0, -13.0)], edge, 0.8);
        s.polyline(&[(9.0, -15.0), (3.0, -13.0)], edge, 0.8);
        for sign in [-1.0f32, 1.0] {
            s.at(9.0 * sign, 2.5, -0.35 * sign, 1.0, |l| {
                l.ellipse(-8.0, -5.0, 16.0, 10.0, fill);
                l.ring(-8.0, -5.0, 16.0, 10.0, edge, 0.8);
            });
        }
        s.circle(0.0, 0.0, 3.6, fill);
        s.circle(-0.8, 0.8, 1.2, ka(1.0, 1.0, 1.0, 0.35));
    });
}

fn red_bow(sk: &mut Sketch, x: f32, y: f32, scale: f32) {
    bow(sk, x, y, scale, red(), dark_red());
}

fn star_points(cx: f32, cy: f32, outer: f32, inner: f32) -> Vec<(f32, f32)> {
    (0..10)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let r = if i % 2 == 0 { outer } else { inner };
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn fir(sk: &mut Sketch, x: f32, y: f32, scale: f32) {
    sk.at(x, y, 0.0, scale, |s| {
        s.polygon(&[(-14.0, 0.0), (14.0, 0.0), (0.0, 14.0)], k(0.10, 0.42, 0.22));
        s.polygon(&[(-11.0, 9.0), (11.0, 9.0), (0.0, 23.0)], k(0.14, 0.50, 0.27));
        s.polygon(&[(-8.0, 18.0), (8.0, 18.0), (0.0, 32.0)], k(0.20, 0.58, 0.32));
    });
}

/// Draws the base layer (the part that is shadowed and solid) of a kind.
pub fn draw_base(sk: &mut Sketch, kind: Kind, strand: Option<Strand>) {
    match kind {
        Kind::Lights => lights(sk, strand.unwrap_or(Strand::LIGHTS)),
        Kind::Candle => candle(sk),
        Kind::RedBauble => bauble(sk, (0.86, 0.14, 0.20), (0.98, 0.78, 0.30)),
        Kind::GoldBauble => bauble(sk, (0.96, 0.72, 0.22), (1.0, 0.98, 0.95)),
        Kind::Star => star(sk),
        Kind::Wreath => wreath(sk),
        Kind::Stocking => stocking(sk),
        Kind::CandyCane => candy_cane(sk),
        Kind::Gift => gift(sk),
        Kind::Bells => bells(sk),
        Kind::Gingerbread => gingerbread(sk),
        Kind::Holly => holly(sk),
        Kind::SantaHat => santa_hat(sk),
        Kind::SnowGlobe => snow_globe(sk),
        Kind::Cocoa => cocoa(sk),
        Kind::Boat => boat(sk),
    }
}

/// Draws one animated part at rest.
pub fn draw_part(sk: &mut Sketch, art: PartArt, strand: Option<Strand>) {
    match art {
        PartArt::CandleHalo => sk.halo(26.0, 92.0, 26.0, ka(1.0, 0.78, 0.38, 0.45)),
        PartArt::CandleFlame => flame(sk),
        PartArt::StarHalo => sk.halo(36.0, 36.0, 44.0, ka(1.0, 0.85, 0.35, 0.55)),
        PartArt::Steam => steam(sk),
        PartArt::Lit(group) => lit(sk, strand.unwrap_or(Strand::LIGHTS), group),
    }
}

fn lights(sk: &mut Sketch, st: Strand) {
    let (p0, c, p2) = st.anchors();
    let rope = path(|t| {
        t.move_to(p0.0 as f32, p0.1 as f32).quad(p2.0 as f32, p2.1 as f32, c.0 as f32, c.1 as f32);
    });
    sk.stroke(&rope, wire(), 2.2);
    sk.ring(p0.0 as f32 - 5.0, p0.1 as f32 - 2.0, 6.0, 6.0, wire(), 1.8);
    sk.ring(p2.0 as f32 - 1.0, p2.1 as f32 - 2.0, 6.0, 6.0, wire(), 1.8);
    for i in 0..st.bulb_count() {
        let ((x, y), turn) = st.bulb(i);
        let (r, g, b) = Strand::bulb_tone(i);
        sk.at(x as f32, y as f32, turn as f32, 1.0, |s| {
            s.rect(-3.4, -7.0, 6.8, 7.5, 1.6, k(0.12, 0.30, 0.17));
            let lo = [k(r * 0.75 + 0.2, g * 0.75 + 0.2, b * 0.75 + 0.2), k(r * 0.55, g * 0.55, b * 0.55)];
            s.round(&oval(-4.8, -21.0, 9.6, 15.0), &lo, None, (-1.5, -11.0), (0.0, -13.5), 9.0);
            s.ellipse(-3.0, -12.0, 2.2, 4.5, ka(1.0, 1.0, 1.0, 0.4));
        });
    }
}

fn lit(sk: &mut Sketch, st: Strand, group: usize) {
    for i in (0..st.bulb_count()).filter(|i| i % 2 == group) {
        let ((x, y), turn) = st.bulb(i);
        let (r, g, b) = Strand::bulb_tone(i);
        sk.at(x as f32, y as f32, turn as f32, 1.0, |s| {
            s.halo(0.0, -13.5, 16.0, ka(r, g, b, 0.55));
            let cols = [k(1.0, 1.0, 0.95), k(r + 0.3, g + 0.3, b + 0.3), k(r, g, b)];
            s.round(&oval(-4.8, -21.0, 9.6, 15.0), &cols, Some(&[0.0, 0.35, 1.0]), (-1.0, -12.0), (0.0, -13.5), 9.0);
        });
    }
}

fn candle(sk: &mut Sketch) {
    sk.ring(40.0, 3.0, 10.0, 8.0, dark_gold(), 1.8);
    metal(sk, &rrect(6.0, 2.0, 40.0, 10.0, 5.0), 6.0, 46.0);
    cloth(sk, &rrect(16.0, 10.0, 20.0, 66.0, 3.0), 16.0, 36.0);
    sk.ellipse(16.0, 72.0, 20.0, 8.0, light_red());
    sk.ellipse(23.0, 74.0, 6.0, 3.0, dark_red());
    sk.rect(18.0, 62.0, 3.2, 12.0, 1.6, ka(1.0, 0.98, 0.95, 0.55));
    sk.rect(31.0, 58.0, 3.2, 16.0, 1.6, ka(1.0, 0.98, 0.95, 0.55));
    sk.line(26.0, 77.0, 26.0, 80.0, k(0.25, 0.12, 0.08), 1.4);
    sprig(sk, 14.0, 12.0);
}

fn flame(sk: &mut Sketch) {
    let outer = path(|t| {
        t.move_to(26.0, 78.0)
            .curve(26.0, 105.0, 17.0, 88.0, 24.0, 97.0)
            .curve(26.0, 78.0, 28.0, 97.0, 35.0, 88.0)
            .close();
    });
    let cols = [k(1.0, 0.5, 0.1), k(1.0, 0.8, 0.28), ka(1.0, 0.95, 0.7, 0.9)];
    sk.across(&outer, &cols, Some(&[0.0, 0.45, 1.0]), (26.0, 78.0), (26.0, 105.0));
    let core = path(|t| {
        t.move_to(26.0, 80.0)
            .curve(26.0, 93.0, 22.5, 85.0, 24.5, 89.0)
            .curve(26.0, 80.0, 27.5, 89.0, 29.5, 85.0)
            .close();
    });
    sk.fill(&core, k(1.0, 1.0, 0.92));
    sk.ellipse(23.5, 79.0, 5.0, 4.0, ka(0.45, 0.6, 1.0, 0.55));
}

fn bauble(sk: &mut Sketch, base: (f64, f64, f64), band: (f64, f64, f64)) {
    let (r, g, b) = base;
    let cord = path(|t| {
        t.move_to(28.6, 73.0).line(30.0, 124.5).line(31.4, 73.0);
    });
    sk.stroke(&cord, k(0.78, 0.10, 0.16), 1.4);
    sk.circle(30.0, 124.5, 2.0, k(0.78, 0.10, 0.16));
    sk.ring(26.0, 67.0, 8.0, 8.0, gold(), 1.6);
    metal(sk, &rrect(23.0, 57.0, 14.0, 11.0, 2.5), 23.0, 37.0);
    for x in [27.0f32, 30.0, 33.0] {
        sk.line(x, 58.0, x, 67.0, dark_gold(), 0.8);
    }
    let ball = oval(3.0, 5.0, 54.0, 54.0);
    let cols = [k(r + 0.25, g + 0.25, b + 0.25), k(r, g, b), k(r * 0.55, g * 0.5, b * 0.55)];
    sk.round(&ball, &cols, Some(&[0.0, 0.5, 1.0]), (20.0, 44.0), (30.0, 32.0), 31.0);
    let band_color = k(band.0, band.1, band.2);
    sk.inside(&ball, |s| {
        for y in [27.0f32, 38.0] {
            let curve = path(|t| {
                t.move_to(0.0, y + 3.0).quad(60.0, y + 3.0, 30.0, y - 4.0);
            });
            s.stroke(&curve, band_color, 2.0);
        }
        for i in 0..7 {
            let x = 6.0 + 8.0 * i as f32;
            let n = (x - 30.0) / 30.0;
            s.circle(x, 32.5 + 3.5 * n * n, 2.0, band_color);
        }
    });
    sk.at(18.0, 46.0, 0.6, 1.0, |s| s.ellipse(-6.0, -3.2, 12.0, 6.4, ka(1.0, 1.0, 1.0, 0.8)));
}

fn star(sk: &mut Sketch) {
    let pts = star_points(36.0, 36.0, 32.0, 14.0);
    let body = poly(&pts);
    let cols = [k(1.0, 0.96, 0.7), k(1.0, 0.8, 0.26), k(0.86, 0.56, 0.1)];
    sk.round(&body, &cols, None, (30.0, 44.0), (36.0, 36.0), 34.0);
    for i in (0..10).step_by(2) {
        sk.polygon(&[(36.0, 36.0), pts[i], pts[(i + 1) % 10]], ka(0.6, 0.35, 0.0, 0.14));
    }
    sk.stroke(&body, k(0.74, 0.46, 0.06), 1.2);
    sk.circle(30.0, 46.0, 3.0, ka(1.0, 1.0, 1.0, 0.75));
}

fn wreath(sk: &mut Sketch) {
    let greens = [k(0.07, 0.36, 0.18), k(0.13, 0.50, 0.26), k(0.22, 0.62, 0.32)];
    let mut n = 0usize;
    for row in 0..2 {
        for i in 0..36 {
            let a = (i as f32 + row as f32 * 0.5) * std::f32::consts::TAU / 36.0;
            let rad = 31.0 + [-7.0, 0.0, 7.0][n % 3];
            let wob = if (i + row) % 2 == 0 { 0.5 } else { -0.5 };
            let tone = greens[n % 3];
            let (x, y) = (50.0 + rad * a.cos(), 54.0 + rad * a.sin());
            sk.at(x, y, a + std::f32::consts::FRAC_PI_2 + wob, 1.0, |s| {
                let leaf = path(|t| {
                    t.move_to(-9.5, 0.0).quad(9.5, 0.0, 0.0, 4.6).quad(-9.5, 0.0, 0.0, -4.6).close();
                });
                s.fill(&leaf, tone);
            });
            n += 1;
        }
    }
    for a in [0.25f32, 1.1, 1.95, 2.8, 3.6, 5.85] {
        let (x, y) = (50.0 + 31.0 * a.cos(), 54.0 + 31.0 * a.sin());
        berry(sk, x - 3.0, y, 3.4);
        berry(sk, x + 3.0, y, 3.4);
        berry(sk, x, y + 4.5, 3.4);
    }
    red_bow(sk, 50.0, 22.0, 1.3);
}

fn stocking(sk: &mut Sketch) {
    let body = path(|t| {
        t.move_to(16.0, 74.0)
            .line(16.0, 32.0)
            .curve(24.0, 10.0, 16.0, 20.0, 17.0, 13.0)
            .curve(54.0, 10.0, 34.0, 6.0, 48.0, 6.0)
            .curve(62.0, 22.0, 62.0, 12.0, 64.0, 18.0)
            .curve(46.0, 32.0, 60.0, 28.0, 50.0, 30.0)
            .line(46.0, 74.0)
            .close();
    });
    cloth(sk, &body, 8.0, 64.0);
    sk.inside(&body, |s| {
        s.line(14.0, 52.0, 48.0, 52.0, cream(), 4.0);
        s.line(14.0, 42.0, 48.0, 42.0, cream(), 4.0);
        s.ellipse(18.0, 58.0, 12.0, 9.0, k(0.10, 0.45, 0.22));
        s.ellipse(30.0, 22.0, 12.0, 9.0, k(0.10, 0.45, 0.22));
    });
    sk.stroke(&body, dark_red(), 1.2);
    sk.rect(12.0, 72.0, 40.0, 20.0, 7.0, cream());
    for i in 0..5 {
        sk.circle(16.0 + 8.0 * i as f32, 72.0, 3.5, cream());
    }
    sk.ellipse(16.0, 82.0, 22.0, 3.0, ka(0.7, 0.8, 1.0, 0.35));
    sk.ring(3.5, 78.0, 9.0, 12.0, dark_red(), 1.8);
}

fn candy_cane(sk: &mut Sketch) {
    let centre = path(|t| {
        t.move_to(34.0, 6.0)
            .line(34.0, 70.0)
            .curve(23.0, 81.0, 34.0, 76.0, 29.0, 81.0)
            .curve(12.0, 70.0, 17.0, 81.0, 12.0, 76.0)
            .line(12.0, 60.0);
    });
    let stroke = Stroke { width: 12.0, line_cap: fp_paint::tiny_skia::LineCap::Round, ..Stroke::default() };
    let body = centre.stroke(&stroke, 1.0).expect("cane outline");
    sk.fill(&body, cream());
    sk.inside(&body, |s| {
        s.at(23.0, 40.0, -0.6, 1.0, |r| {
            for i in -9..9 {
                r.rect(i as f32 * 10.0 - 2.3, -80.0, 4.6, 160.0, 0.0, red());
            }
        });
        s.line(31.0, 10.0, 31.0, 66.0, ka(1.0, 1.0, 1.0, 0.45), 2.2);
    });
    sk.stroke(&body, dark_red(), 1.0);
    bow(sk, 34.0, 46.0, 0.6, k(0.10, 0.50, 0.25), k(0.05, 0.28, 0.14));
}

fn gift(sk: &mut Sketch) {
    let top = k(0.86, 0.16, 0.22);
    let bottom = k(0.62, 0.06, 0.12);
    sk.vertical(&rrect(12.0, 4.0, 56.0, 42.0, 3.0), top, bottom);
    for row in 0..5 {
        for col in 0..7 {
            let off = if row % 2 == 1 { 4.0 } else { 0.0 };
            sk.circle(17.0 + col as f32 * 8.0 + off, 10.0 + row as f32 * 8.0, 1.6, ka(1.0, 1.0, 1.0, 0.35));
        }
    }
    metal(sk, &rrect(35.0, 4.0, 10.0, 42.0, 0.5), 35.0, 45.0);
    sk.rect(12.0, 40.0, 56.0, 5.0, 0.0, ka(0.0, 0.0, 0.1, 0.18));
    sk.vertical(&rrect(8.0, 44.0, 64.0, 14.0, 3.0), top, bottom);
    metal(sk, &rrect(35.0, 44.0, 10.0, 14.0, 0.5), 35.0, 45.0);
    sk.rect_outline((8.0, 44.0, 64.0, 14.0), 3.0, dark_red(), 1.0);
    bow(sk, 40.0, 64.0, 0.9, gold(), dark_gold());
}

fn bells(sk: &mut Sketch) {
    for (x, turn) in [(35.0f32, -0.35f32), (51.0, 0.35)] {
        sk.at(x, 64.0, turn, 1.0, |s| {
            s.circle(0.0, -35.0, 4.2, dark_gold());
            let body = path(|t| {
                t.move_to(-12.0, 0.0)
                    .curve(-17.0, -32.0, -12.0, -16.0, -17.0, -22.0)
                    .quad(17.0, -32.0, 0.0, -36.5)
                    .curve(12.0, 0.0, 17.0, -22.0, 12.0, -16.0)
                    .close();
            });
            metal(s, &body, -17.0, 17.0);
            let rim = path(|t| {
                t.move_to(-17.0, -32.0).quad(17.0, -32.0, 0.0, -36.5);
            });
            s.stroke(&rim, dark_gold(), 1.4);
            s.stroke(&body, k(0.6, 0.38, 0.06), 1.0);
            s.ellipse(-8.0, -20.0, 3.0, 13.0, ka(1.0, 1.0, 1.0, 0.55));
        });
    }
    red_bow(sk, 43.0, 68.0, 0.75);
}

fn gingerbread(sk: &mut Sketch) {
    let edge = k(0.55, 0.30, 0.12);
    let fill = k(0.82, 0.52, 0.26);
    let limbs = [
        ((27.0f32, 52.0f32), (10.0f32, 44.0f32), 14.0f32),
        ((45.0, 52.0), (62.0, 44.0), 14.0),
        ((31.0, 34.0), (23.0, 10.0), 15.0),
        ((41.0, 34.0), (49.0, 10.0), 15.0),
    ];
    for (a, b, w) in limbs {
        sk.line(a.0, a.1, b.0, b.1, edge, w + 3.0);
    }
    sk.rect(20.5, 26.5, 31.0, 33.0, 13.5, edge);
    sk.circle(36.0, 68.0, 15.5, edge);
    for (a, b, w) in limbs {
        sk.line(a.0, a.1, b.0, b.1, fill, w);
    }
    sk.rect(22.0, 28.0, 28.0, 30.0, 12.0, fill);
    sk.circle(36.0, 68.0, 14.0, fill);
    sk.polyline(&[(5.0, 50.0), (8.0, 46.0), (11.0, 50.0), (14.0, 46.0)], cream(), 1.4);
    sk.polyline(&[(58.0, 46.0), (61.0, 50.0), (64.0, 46.0), (67.0, 50.0)], cream(), 1.4);
    sk.polyline(&[(20.0, 14.0), (23.0, 18.0), (26.0, 14.0)], cream(), 1.4);
    sk.polyline(&[(46.0, 14.0), (49.0, 18.0), (52.0, 14.0)], cream(), 1.4);
    let smile = path(|t| {
        t.move_to(31.0, 64.0).quad(41.0, 64.0, 36.0, 59.0);
    });
    sk.stroke(&smile, edge, 1.4);
    sk.circle(31.0, 71.0, 2.0, edge);
    sk.circle(41.0, 71.0, 2.0, edge);
    sk.circle(27.0, 66.0, 2.6, ka(1.0, 0.55, 0.6, 0.7));
    sk.circle(45.0, 66.0, 2.6, ka(1.0, 0.55, 0.6, 0.7));
    for (y, c) in [(52.0f32, red()), (45.0, k(0.10, 0.50, 0.25)), (38.0, red())] {
        sk.circle(36.0, y, 2.8, c);
        sk.circle(35.2, y + 0.9, 0.9, ka(1.0, 1.0, 1.0, 0.7));
    }
}

fn holly(sk: &mut Sketch) {
    let leaves = [(2.79f32, 37.0f32), (0.35, 37.0), (-2.07, 28.0), (-1.07, 28.0)];
    for (turn, len) in leaves {
        leaf(sk, 43.0, 32.0, turn, len, holly_tones());
    }
    berry(sk, 38.5, 36.0, 5.6);
    berry(sk, 47.5, 36.0, 5.6);
    berry(sk, 43.0, 42.5, 5.6);
}

fn santa_hat(sk: &mut Sketch) {
    let cone = path(|t| {
        t.move_to(12.0, 24.0)
            .curve(76.0, 47.0, 10.0, 52.0, 50.0, 62.0)
            .curve(82.0, 24.0, 82.0, 40.0, 83.0, 32.0)
            .close();
    });
    sk.round(&cone, &[light_red(), red(), dark_red()], None, (40.0, 46.0), (46.0, 38.0), 46.0);
    let fold = path(|t| {
        t.move_to(30.0, 26.0).quad(66.0, 56.0, 40.0, 48.0);
    });
    sk.stroke(&fold, ka(0.4, 0.0, 0.05, 0.35), 3.0);
    sk.rect(8.0, 6.0, 74.0, 20.0, 9.0, cream());
    for i in 0..8 {
        sk.circle(14.0 + 9.0 * i as f32, 8.0, 4.5, cream());
    }
    sk.ellipse(14.0, 18.0, 60.0, 3.0, ka(0.7, 0.8, 1.0, 0.3));
    sk.circle(78.0, 47.0, 8.5, cream());
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        sk.circle(78.0 + 7.5 * a.cos(), 47.0 + 7.5 * a.sin(), 4.0, cream());
    }
    sk.circle(75.0, 44.0, 3.0, ka(0.8, 0.88, 1.0, 0.5));
}

fn snow_globe(sk: &mut Sketch) {
    let sphere = oval(8.0, 24.0, 60.0, 60.0);
    sk.vertical(&sphere, k(0.82, 0.92, 1.0), k(0.5, 0.72, 0.98));
    sk.inside(&sphere, |s| {
        s.ellipse(12.0, 26.0, 52.0, 18.0, ka(1.0, 1.0, 1.0, 0.92));
        s.rect(35.5, 36.0, 5.0, 9.0, 0.0, k(0.40, 0.24, 0.12));
        s.polygon(&[(26.0, 42.0), (50.0, 42.0), (38.0, 56.0)], k(0.10, 0.42, 0.22));
        s.polygon(&[(28.0, 50.0), (48.0, 50.0), (38.0, 64.0)], k(0.14, 0.50, 0.27));
        s.polygon(&[(30.0, 58.0), (46.0, 58.0), (38.0, 72.0)], k(0.20, 0.58, 0.32));
        s.polygon(&star_points(38.0, 74.0, 6.0, 2.6), gold());
        let mut seed: u64 = 7;
        for _ in 0..26 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let x = 10.0 + ((seed >> 40) & 255) as f32 / 255.0 * 56.0;
            let y = 40.0 + ((seed >> 24) & 255) as f32 / 255.0 * 42.0;
            let r = 0.8 + ((seed >> 16) & 15) as f32 / 15.0;
            s.circle(x, y, r, ka(1.0, 1.0, 1.0, 0.9));
        }
    });
    sk.stroke(&sphere, ka(0.3, 0.5, 0.8, 0.5), 1.0);
    let glint = path(|t| {
        t.move_to(18.0, 64.0).quad(26.0, 76.0, 16.0, 72.0);
    });
    sk.stroke(&glint, ka(1.0, 1.0, 1.0, 0.7), 2.5);
    cloth(sk, &poly(&[(8.0, 4.0), (68.0, 4.0), (60.0, 26.0), (16.0, 26.0)]), 8.0, 68.0);
    metal(sk, &rrect(13.0, 12.0, 50.0, 4.0, 1.0), 13.0, 63.0);
    sk.line(16.0, 26.0, 60.0, 26.0, dark_red(), 2.0);
}

fn cocoa(sk: &mut Sketch) {
    sk.ring(52.0, 22.0, 20.0, 26.0, red(), 7.0);
    cloth(sk, &rrect(14.0, 8.0, 48.0, 50.0, 8.0), 14.0, 62.0);
    sk.rect(14.0, 26.0, 48.0, 12.0, 0.0, cream());
    for i in 0..7 {
        sk.circle(20.0 + 6.0 * i as f32, 32.0, 1.8, red());
    }
    sk.ellipse(14.0, 52.0, 48.0, 12.0, cream());
    sk.ellipse(17.0, 54.0, 42.0, 8.0, k(0.42, 0.22, 0.10));
    for (x, y, a) in [(26.0f32, 60.0f32, 0.3f32), (38.0, 61.0, -0.25), (47.0, 59.0, 0.5)] {
        sk.at(x, y, a, 1.0, |s| {
            s.rect(-6.0, -4.0, 12.0, 8.0, 3.0, cream());
            s.ellipse(-4.0, 0.0, 6.0, 2.0, ka(0.7, 0.8, 1.0, 0.4));
        });
    }
}

fn steam(sk: &mut Sketch) {
    for x in [26.0f32, 36.0, 46.0] {
        let curl = path(|t| {
            t.move_to(x, 66.0).curve(x, 80.0, x - 6.0, 70.0, x + 6.0, 75.0);
        });
        sk.stroke(&curl, ka(1.0, 1.0, 1.0, 0.6), 2.6);
    }
}

fn boat(sk: &mut Sketch) {
    let hull = path(|t| {
        t.move_to(6.0, 38.0)
            .line(90.0, 38.0)
            .curve(84.0, 16.0, 89.0, 26.0, 86.0, 20.0)
            .line(18.0, 12.0)
            .curve(6.0, 38.0, 10.0, 18.0, 6.0, 28.0)
            .close();
    });
    sk.across(&hull, &[k(0.62, 0.06, 0.12), red(), light_red()], None, (0.0, 12.0), (0.0, 38.0));
    sk.inside(&hull, |s| s.rect(0.0, 29.0, 96.0, 4.0, 0.0, cream()));
    sk.line(6.0, 38.0, 90.0, 38.0, gold(), 2.5);
    for x in [30.0f32, 48.0, 66.0] {
        sk.circle(x, 22.0, 4.2, gold());
        sk.circle(x, 22.0, 3.0, k(0.35, 0.6, 1.0));
    }
    sk.line(48.0, 38.0, 48.0, 90.0, k(0.45, 0.28, 0.12), 3.0);
    sk.polygon(&[(49.5, 90.0), (49.5, 82.0), (62.0, 86.0)], k(0.10, 0.50, 0.25));
    let sail = poly(&[(52.0, 50.0), (52.0, 86.0), (84.0, 50.0)]);
    sk.fill(&sail, cream());
    sk.inside(&sail, |s| {
        s.line(50.0, 56.0, 84.0, 56.0, red(), 3.0);
        s.line(50.0, 64.0, 84.0, 64.0, red(), 3.0);
    });
    fir(sk, 20.0, 39.0, 0.9);
    for (x, y, c) in [(15.0f32, 44.0f32, red()), (24.0, 50.0, gold()), (20.0, 58.0, red())] {
        sk.circle(x, y, 1.8, c);
    }
    sk.rect(32.0, 39.0, 10.0, 9.0, 0.5, k(0.10, 0.50, 0.25));
    sk.rect(35.5, 39.0, 3.0, 9.0, 0.0, red());
    sk.rect(41.0, 39.0, 6.0, 6.0, 0.5, red());
    sk.rect(43.5, 39.0, 1.5, 6.0, 0.0, gold());
    sk.polyline(&[(54.0, 39.0), (54.0, 44.0), (88.0, 44.0), (88.0, 39.0)], gold(), 1.0);
    for x in [62.0f32, 70.0, 78.0] {
        sk.line(x, 39.0, x, 44.0, gold(), 1.0);
    }
}
