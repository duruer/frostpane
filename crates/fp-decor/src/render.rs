//! Rasterising one decoration window: shadowed base, animated parts, hover
//! chrome (remove button, grips), the solid mask and the picker icons.

use crate::art::{self, Part};
use crate::motion::PartState;
use crate::{Kind, PAD};
use fp_paint::tiny_skia::{Pixmap, PixmapPaint, PremultipliedColorU8, Transform};
use fp_paint::{Sketch, Strand, rgb, rgba};

/// Everything the painter needs about one frame of one decoration.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub zoom: f64,
    /// Hover grow progress 0..1 (1.0 = 1.04x).
    pub grow: f64,
    /// Pop in/out scale factor.
    pub pop: f64,
    /// Sway rotation in radians (Y-up).
    pub sway: f32,
    /// One state per [`art::parts`] entry.
    pub parts: Vec<PartState>,
    /// Opacity of the remove button and grips (0 = hidden).
    pub chrome: f32,
}

/// Unzoomed window box: art + padding on every side.
pub fn box_size(kind: Kind, strand: Option<Strand>) -> (f64, f64) {
    let (w, h) = kind.art_size(strand);
    (w + 2.0 * PAD, h + 2.0 * PAD)
}

/// Window size in points for a zoom: `ceil(box * zoom)` per axis.
pub fn outer_size(kind: Kind, strand: Option<Strand>, zoom: f64) -> (f64, f64) {
    let (w, h) = box_size(kind, strand);
    ((w * zoom).ceil(), (h * zoom).ceil())
}

fn pixmap(w: u32, h: u32) -> Pixmap {
    Pixmap::new(w.max(1), h.max(1)).expect("non-empty pixmap")
}

#[derive(Clone, Copy)]
struct View {
    k: f32,
    off: (f32, f32),
    pivot: (f64, f64),
    sway: f32,
}

fn open<'a>(pm: &'a mut Pixmap, v: View) -> Sketch<'a> {
    let (pivot, sway) = (v.pivot, v.sway);
    let mut sk = Sketch::new(pm, v.k);
    sk.translate(v.off.0, v.off.1);
    sk.translate(PAD as f32, PAD as f32);
    sk.translate(pivot.0 as f32, pivot.1 as f32);
    sk.rotate(sway);
    sk.translate(-pivot.0 as f32, -pivot.1 as f32);
    sk
}

fn blend(dst: &mut Pixmap, src: &Pixmap, opacity: f32) {
    let paint = PixmapPaint { opacity, ..PixmapPaint::default() };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::identity(), None);
}

fn box_blur(a: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let at = |l: usize, i: usize| if horizontal { l * w + i } else { i * w + l };
    let mut line = vec![0.0f32; len];
    for l in 0..lines {
        for (i, v) in line.iter_mut().enumerate() {
            let (lo, hi) = (i.saturating_sub(r), (i + r).min(len - 1));
            *v = (lo..=hi).map(|j| a[at(l, j)]).sum::<f32>() / (2 * r + 1) as f32;
        }
        for (i, v) in line.iter().enumerate() {
            a[at(l, i)] = *v;
        }
    }
}

/// Soft drop shadow of `base` (rgba(0,0,0.05,0.38), down 2, blur 5 x 1.5).
fn shadow(base: &Pixmap, k: f32) -> Pixmap {
    let (w, h) = (base.width() as usize, base.height() as usize);
    let mut a: Vec<f32> = base.pixels().iter().map(|p| f32::from(p.alpha()) / 255.0).collect();
    let r = (2.5 * k).round().max(1.0) as usize;
    for _ in 0..2 {
        box_blur(&mut a, w, h, r, true);
        box_blur(&mut a, w, h, r, false);
    }
    let down = (2.0 * k).round() as usize;
    let mut out = pixmap(w as u32, h as u32);
    let px = out.pixels_mut();
    for y in 0..h.saturating_sub(down) {
        for x in 0..w {
            let alpha = (a[y * w + x] * 0.38 * 255.0).round() as u8;
            let blue = (f32::from(alpha) * 0.05).round() as u8;
            px[(y + down) * w + x] = PremultipliedColorU8::from_rgba(0, 0, blue, alpha).expect("blue <= alpha");
        }
    }
    out
}

fn draw_part(pm: &mut Pixmap, part: &Part, st: &PartState, strand: Option<Strand>, v: View) {
    let mut layer = pixmap(pm.width(), pm.height());
    {
        let mut sk = open(&mut layer, v);
        sk.translate(part.anchor.0, part.anchor.1 + st.ty);
        sk.rotate(st.rot);
        sk.scale_by(st.sx, st.sy);
        sk.translate(-part.anchor.0, -part.anchor.1);
        art::draw_part(&mut sk, part.art, strand);
    }
    blend(pm, &layer, st.opacity);
}

/// Centre of the remove button in window points (Y up).
pub fn button_center(kind: Kind, strand: Option<Strand>, zoom: f64) -> (f64, f64) {
    let (aw, ah) = kind.art_size(strand);
    let (ow, oh) = outer_size(kind, strand, zoom);
    if kind == Kind::Lights {
        (ow / 2.0, (PAD + ah - 2.0) * zoom)
    } else {
        (((PAD + aw - 2.0) * zoom).min(ow - 10.0), ((PAD + ah - 2.0) * zoom).min(oh - 10.0))
    }
}

/// Active grip centres `(index, x, y)` in window points (Y up).
pub fn grips(kind: Kind, strand: Option<Strand>, zoom: f64) -> Vec<(usize, f64, f64)> {
    let (aw, ah) = kind.art_size(strand);
    let (ow, _) = outer_size(kind, strand, zoom);
    let p = zoom;
    if kind == Kind::Lights {
        let g = ah - 8.0;
        let len = strand.unwrap_or(Strand::LIGHTS).length;
        vec![
            (0, 16.0 * p, (PAD + g + 1.0) * p),
            (1, (PAD + aw - 4.0) * p, (PAD + g + 1.0) * p),
            (2, (PAD + len / 2.0) * p, (PAD + 38.0) * p),
        ]
    } else {
        vec![(3, ((PAD + aw) * p).min(ow - 7.0), (PAD * p).max(7.0))]
    }
}

fn chrome(pm: &mut Pixmap, kind: Kind, strand: Option<Strand>, zoom: f64, scale: f32, opacity: f32) {
    let mut layer = pixmap(pm.width(), pm.height());
    let mut sk = Sketch::new(&mut layer, scale);
    let (bx, by) = button_center(kind, strand, zoom);
    let (bx, by) = (bx as f32, by as f32);
    sk.circle(bx, by, 9.0, rgba(0.1, 0.1, 0.14, 0.82));
    sk.ring(bx - 9.0, by - 9.0, 18.0, 18.0, rgba(1.0, 1.0, 1.0, 0.9), 1.5);
    sk.line(bx - 3.0, by - 3.0, bx + 3.0, by + 3.0, rgb(1.0, 1.0, 1.0), 1.8);
    sk.line(bx - 3.0, by + 3.0, bx + 3.0, by - 3.0, rgb(1.0, 1.0, 1.0), 1.8);
    for (_, gx, gy) in grips(kind, strand, zoom) {
        sk.circle(gx as f32, gy as f32, 6.0, rgba(1.0, 1.0, 1.0, 0.95));
        sk.ring(gx as f32 - 6.0, gy as f32 - 6.0, 12.0, 12.0, rgb(0.10, 0.24, 0.14), 2.0);
    }
    blend(pm, &layer, opacity);
}

/// Window pixmap for one frame: `ceil(outer * scale)` pixels per axis. A
/// non-finite or non-positive `scale` counts as 1.
pub fn paint(kind: Kind, strand: Option<Strand>, look: &Look, scale: f32) -> Pixmap {
    let scale = if scale > 0.0 && scale.is_finite() { scale } else { 1.0 };
    let (bw, bh) = box_size(kind, strand);
    let (ow, oh) = outer_size(kind, strand, look.zoom);
    let (w, h) = ((ow * f64::from(scale)).ceil() as u32, (oh * f64::from(scale)).ceil() as u32);
    let grow = 1.0 + 0.04 * look.grow as f32;
    let k = (scale * look.zoom as f32 * grow * look.pop as f32).max(0.01);
    let off = (((w as f32 - bw as f32 * k) / 2.0) / k, ((h as f32 - bh as f32 * k) / 2.0) / k);
    let view = View { k, off, pivot: kind.pivot(strand), sway: look.sway };
    let list = art::parts(kind);
    let mut out = pixmap(w, h);
    let draw = |out: &mut Pixmap, behind: bool| {
        for (part, st) in list.iter().zip(&look.parts).filter(|(p, _)| p.behind == behind) {
            draw_part(out, part, st, strand, view);
        }
    };
    draw(&mut out, true);
    let mut base = pixmap(w, h);
    art::draw_base(&mut open(&mut base, view), kind, strand);
    let sh = shadow(&base, k);
    blend(&mut out, &sh, 1.0);
    blend(&mut out, &base, 1.0);
    draw(&mut out, false);
    if look.chrome > 0.0 {
        chrome(&mut out, kind, strand, look.zoom, scale, look.chrome.min(1.0));
    }
    out
}

/// 8-bit alpha of the base art (no shadow) plus solid parts at rest over the
/// whole padded box at scale 1, Y-down rows (decorations 3.5).
#[derive(Clone, Debug, PartialEq)]
pub struct AlphaMask {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

pub fn mask(kind: Kind, strand: Option<Strand>) -> AlphaMask {
    let (bw, bh) = box_size(kind, strand);
    let (w, h) = (bw.ceil() as u32, bh.ceil() as u32);
    let mut pm = pixmap(w, h);
    {
        let mut sk = open(&mut pm, View { k: 1.0, off: (0.0, 0.0), pivot: (0.0, 0.0), sway: 0.0 });
        art::draw_base(&mut sk, kind, strand);
        for part in art::parts(kind).iter().filter(|p| p.solid) {
            art::draw_part(&mut sk, part.art, strand);
        }
    }
    AlphaMask { w, h, data: pm.pixels().iter().map(|p| p.alpha()).collect() }
}

impl AlphaMask {
    /// Solid test at a Y-down box coordinate: 3x3 samples 3 apart, any
    /// in-bounds alpha > 60.
    pub fn solid(&self, x: f64, y: f64) -> bool {
        let (cx, cy) = (x.trunc() as i64, y.trunc() as i64);
        [-3i64, 0, 3].iter().any(|dy| {
            [-3i64, 0, 3].iter().any(|dx| {
                let (sx, sy) = (cx + dx, cy + dy);
                sx >= 0
                    && sy >= 0
                    && sx < i64::from(self.w)
                    && sy < i64::from(self.h)
                    && self.data[(sy as usize) * self.w as usize + sx as usize] > 60
            })
        })
    }
}

/// Picker tile icon: the full picture at `scale`, no shadow, no padding.
pub fn icon(kind: Kind, scale: f32) -> Pixmap {
    let (w, h) = kind.art_size(None);
    let mut pm = pixmap((w * f64::from(scale)).ceil() as u32, (h * f64::from(scale)).ceil() as u32);
    paint_rest(&mut Sketch::new(&mut pm, scale), kind);
    pm
}

/// Icon fitted uniformly and centred into a `side` x `side` square.
pub fn icon_square(kind: Kind, side: u32) -> Pixmap {
    let (w, h) = kind.art_size(None);
    let mut pm = pixmap(side, side);
    let k = (side as f32 / w as f32).min(side as f32 / h as f32);
    let mut sk = Sketch::new(&mut pm, k);
    sk.translate((side as f32 / k - w as f32) / 2.0, (side as f32 / k - h as f32) / 2.0);
    paint_rest(&mut sk, kind);
    pm
}

fn paint_rest(sk: &mut Sketch, kind: Kind) {
    let list = art::parts(kind);
    for part in list.iter().filter(|p| p.behind) {
        art::draw_part(sk, part.art, None);
    }
    art::draw_base(sk, kind, None);
    for part in list.iter().filter(|p| !p.behind) {
        art::draw_part(sk, part.art, None);
    }
}
