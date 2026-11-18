//! Platform-independent decisions of the layered host (all unit tested).
//!
//! Spec: decorations 3.5 (3x3 solid sampling, pick/hysteresis), 3.7 (stack
//! order check, taskbar settle), desktop-native 3.10, core-app 2.7 (tray
//! pointer placement and timing).

use fp_core::geom::{Frame, PixelRect, Vec2};
use fp_paint::tiny_skia::Pixmap;
pub use fp_platform::sprites::stack_ok;

/// Alpha above which a sampled pixel counts as solid (of 255).
pub const HIT_ALPHA: u8 = 60;
/// Distance between the 3x3 sample points, in pixels.
pub const SAMPLE_STEP: i32 = 3;

/// 8-bit alpha buffer of a painted sprite, Y down, used for hit tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    w: i32,
    h: i32,
    alpha: Vec<u8>,
}

impl Mask {
    /// Mask from raw alpha rows. A length mismatch yields an empty mask.
    pub fn from_alpha(w: i32, h: i32, alpha: Vec<u8>) -> Self {
        if w <= 0 || h <= 0 || alpha.len() != (w as usize) * (h as usize) {
            return Self { w: 0, h: 0, alpha: Vec::new() };
        }
        Self { w, h, alpha }
    }

    /// Alpha channel of a premultiplied RGBA pixmap.
    pub fn from_pixmap(p: &Pixmap) -> Self {
        let alpha = p.pixels().iter().map(|c| c.alpha()).collect();
        Self::from_alpha(p.width() as i32, p.height() as i32, alpha)
    }

    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    /// Alpha at a pixel, `None` outside the buffer.
    pub fn alpha_at(&self, x: i32, y: i32) -> Option<u8> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        self.alpha.get((y * self.w + x) as usize).copied()
    }

    /// 3x3 grid, spacing 3 px, solid when any in-bounds sample is above
    /// [`HIT_ALPHA`] (decorations 3.5).
    pub fn solid(&self, x: i32, y: i32) -> bool {
        self.solid_step(x, y, SAMPLE_STEP)
    }

    /// [`Mask::solid`] with a custom sample spacing in mask pixels (at least 1).
    pub fn solid_step(&self, x: i32, y: i32, step: i32) -> bool {
        let step = step.max(1);
        for dy in [-step, 0, step] {
            for dx in [-step, 0, step] {
                if self.alpha_at(x + dx, y + dy).is_some_and(|a| a > HIT_ALPHA) {
                    return true;
                }
            }
        }
        false
    }
}

/// One hit-test candidate: the window's pixel rect and its mask
/// (`None` = the whole rect is clickable).
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    pub rect: PixelRect,
    pub mask: Option<&'a Mask>,
    /// Monitor scale of the window: the 3 px sample spacing is in points
    /// (the spec's mask is at scale 1), so it becomes `3 * scale` pixels.
    pub scale: f64,
}

impl Candidate<'_> {
    fn contains(&self, p: (i32, i32)) -> bool {
        self.rect.contains(p.0 as f64, p.1 as f64)
    }
    fn solid_at(&self, p: (i32, i32)) -> bool {
        match self.mask {
            None => true,
            Some(m) => m.solid_step(p.0 - self.rect.left, p.1 - self.rect.top, sample_step(self.scale)),
        }
    }
}

/// Sample spacing in pixels for a monitor scale (bad scale counts as 1).
pub fn sample_step(scale: f64) -> i32 {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    ((SAMPLE_STEP as f64 * s).round() as i32).max(1)
}

/// Monitor scale with the spec fallback: non-finite or `<= 0` gives 1.0
/// (decorations 3.4).
pub fn sane_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

/// Which candidate the cursor is on (desktop-native 3.10 `Track`).
/// `candidates` are back to front. A still-hovered candidate whose rect holds
/// the cursor is kept (stays hot over transparent pixels); otherwise the
/// topmost candidate with a solid pixel under the cursor is picked. A pick
/// that is covered by another window yields `None`.
pub fn pick(
    candidates: &[Candidate],
    cursor: (i32, i32),
    hovered: Option<usize>,
    covered: &mut dyn FnMut(usize) -> bool,
) -> Option<usize> {
    let hit = match hovered.filter(|&h| candidates.get(h).is_some_and(|c| c.contains(cursor))) {
        Some(h) => h,
        None => candidates.iter().rposition(|c| c.contains(cursor) && c.solid_at(cursor))?,
    };
    if covered(hit) { None } else { Some(hit) }
}

fn overlap(a: &PixelRect, b: &PixelRect) -> bool {
    !a.is_empty() && !b.is_empty() && a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Up to four non-empty parts of a monitor outside its work area:
/// bottom, top, left, right (desktop-native 3.10 `TaskbarAreas`).
pub fn taskbar_areas(pixels: PixelRect, work: PixelRect) -> Vec<PixelRect> {
    [
        PixelRect::new(pixels.left, work.bottom, pixels.right, pixels.bottom),
        PixelRect::new(pixels.left, pixels.top, pixels.right, work.top),
        PixelRect::new(pixels.left, pixels.top, work.left, pixels.bottom),
        PixelRect::new(work.right, pixels.top, pixels.right, pixels.bottom),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// Monitor counts as fullscreen-covered when one of `fullscreen` (foreground
/// window rects) covers the whole monitor (decorations 3.7, 7). A maximized
/// window that leaves the taskbar visible does not count.
pub fn covered_by_fullscreen(monitor: PixelRect, fullscreen: &[PixelRect]) -> bool {
    !monitor.is_empty()
        && fullscreen.iter().any(|r| {
            r.left <= monitor.left && r.top <= monitor.top && r.right >= monitor.right && r.bottom >= monitor.bottom
        })
}

/// Pixel frame shrunk by `min(12*scale, w/4)` on left/right and
/// `min(12*scale, h/4)` on top/bottom (decorations 3.7 Settle).
pub fn shrink_for_settle(frame: PixelRect, scale: f64) -> PixelRect {
    let m = 12.0 * sane_scale(scale);
    let dx = m.min(frame.width() as f64 / 4.0);
    let dy = m.min(frame.height() as f64 / 4.0);
    PixelRect::new(frame.left + dx as i32, frame.top + dy as i32, frame.right - dx as i32, frame.bottom - dy as i32)
}

/// Does the (shrunk) frame overlap the taskbar of a monitor that is not
/// covered by a fullscreen app? `monitors` = (pixels, work) pairs.
pub fn touches_taskbar(
    frame: PixelRect,
    scale: f64,
    monitors: &[(PixelRect, PixelRect)],
    fullscreen: &[PixelRect],
) -> bool {
    if frame.is_empty() {
        return false;
    }
    let shrunk = shrink_for_settle(frame, scale);
    monitors
        .iter()
        .filter(|(px, _)| !covered_by_fullscreen(*px, fullscreen))
        .any(|(px, work)| taskbar_areas(*px, *work).iter().any(|t| overlap(t, &shrunk)))
}

/// Action the settle step asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settle {
    Nothing,
    /// Become topmost (over the taskbar).
    Raise,
    /// Leave topmost, then restack on the desktop.
    Lower,
}

/// Settle state machine (decorations 3.7): raise when touching the taskbar
/// and either not yet raised or the taskbar now sits above; lower when no
/// longer touching.
pub fn settle(touching: bool, over: bool, taskbar_above: impl FnOnce() -> bool) -> Settle {
    if touching {
        if !over || taskbar_above() { Settle::Raise } else { Settle::Nothing }
    } else if over {
        Settle::Lower
    } else {
        Settle::Nothing
    }
}

/// Window alpha byte from sprite opacity (NaN gives 0).
pub fn opacity_byte(o: f32) -> u8 {
    if o.is_nan() { 0 } else { (o.clamp(0.0, 1.0) * 255.0).round() as u8 }
}

/// Tray pointer window size in points (core-app 2.7).
pub const TRAY_W: f64 = 240.0;
pub const TRAY_H: f64 = 170.0;
/// Seconds the pointer stays before fading out.
pub const TRAY_SHOW_SECS: f64 = 5.0;
const TRAY_FADE_IN: f64 = 0.2;
const TRAY_FADE_OUT: f64 = 0.4;

/// Placement of the tray pointer window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrayPointer {
    /// Window box in global points (Y up).
    pub frame: Frame,
    /// Icon is in the upper half (taskbar on top): arrow points up, label below.
    pub below: bool,
    /// Ring centre in global points.
    pub ring_center: Vec2,
    /// Ring side: `max(icon w, icon h) + 14`.
    pub ring_side: f64,
}

/// Plan the tray pointer window for an icon rect (global points, Y up).
/// `work` = work area of the icon's monitor, `virtual_screen` = bounds of all
/// monitors; the window is clamped horizontally into the latter.
pub fn tray_pointer(icon: Frame, work: Frame, virtual_screen: Frame) -> TrayPointer {
    let below = icon.max_y() > work.y + work.h / 2.0;
    let hi = (virtual_screen.x + virtual_screen.w - TRAY_W).max(virtual_screen.x);
    let left = (icon.mid_x() - TRAY_W / 2.0).clamp(virtual_screen.x, hi);
    let y = if below { icon.max_y() - TRAY_H } else { icon.min_y() };
    TrayPointer {
        frame: Frame::new(left, y, TRAY_W, TRAY_H),
        below,
        ring_center: icon.center(),
        ring_side: icon.w.max(icon.h) + 14.0,
    }
}

/// Window opacity `t` seconds after the pointer was shown: 0 to 1 over 0.2 s,
/// hold until 5 s, then 1 to 0 over 0.4 s (linear). `None` once it is over
/// and the window should close.
pub fn tray_pointer_opacity(t: f64) -> Option<f32> {
    if t.is_nan() || t < 0.0 {
        return Some(0.0);
    }
    if t >= TRAY_SHOW_SECS + TRAY_FADE_OUT {
        return None;
    }
    let v = if t < TRAY_FADE_IN {
        t / TRAY_FADE_IN
    } else if t < TRAY_SHOW_SECS {
        1.0
    } else {
        1.0 - (t - TRAY_SHOW_SECS) / TRAY_FADE_OUT
    };
    Some(v as f32)
}

/// Pulse ring `index` (0 or 1) at time `t`: `(scale, opacity)`. Scale 1 to 1.9
/// (quadratic ease-out) and opacity 0.9 to 0 (linear) over 1.3 s, repeating;
/// ring 1 starts 0.65 s later and is invisible before its first cycle.
pub fn tray_ring_pulse(index: usize, t: f64) -> (f64, f64) {
    const CYCLE: f64 = 1.3;
    let t = t - if index == 0 { 0.0 } else { 0.65 };
    if t.is_nan() || t < 0.0 {
        return (1.0, 0.0);
    }
    let p = (t % CYCLE) / CYCLE;
    let eased = 1.0 - (1.0 - p) * (1.0 - p);
    (1.0 + 0.9 * eased, 0.9 * (1.0 - p))
}

/// Vertical bob of the arrow/label stack at time `t`: from 8 to 0 in 0.55 s,
/// sine ease-in-out, auto-reverse. Offset in Y-down window points: positive
/// (moves down) for `below`, negative (moves up) otherwise.
pub fn tray_bob(below: bool, t: f64) -> f64 {
    const HALF: f64 = 0.55;
    let phase = if t.is_nan() || t < 0.0 { 0.0 } else { t % (2.0 * HALF) };
    let p = if phase < HALF { phase / HALF } else { 2.0 - phase / HALF };
    let eased = 0.5 - 0.5 * (std::f64::consts::PI * p).cos();
    let off = 8.0 * (1.0 - eased);
    if below { off } else { -off }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_dot() -> Mask {
        let mut a = vec![0u8; 20 * 20];
        a[10 * 20 + 10] = 200;
        Mask::from_alpha(20, 20, a)
    }

    #[test]
    fn mask_basics() {
        let m = mask_dot();
        assert_eq!(m.size(), (20, 20));
        assert_eq!(m.alpha_at(10, 10), Some(200));
        assert_eq!(m.alpha_at(-1, 0), None);
        assert_eq!(m.alpha_at(20, 0), None);
        assert_eq!(m.alpha_at(0, 20), None);
        assert_eq!(Mask::from_alpha(2, 2, vec![0; 3]).size(), (0, 0));
        assert_eq!(Mask::from_alpha(0, 2, vec![]).size(), (0, 0));
    }

    #[test]
    fn solid_samples_3x3() {
        let m = mask_dot();
        assert!(m.solid(10, 10));
        assert!(m.solid(13, 7));
        assert!(m.solid(7, 13));
        assert!(!m.solid(14, 10));
        assert!(!m.solid(11, 14));
        assert!(!m.solid(-50, -50));
        assert!(!Mask::from_alpha(1, 1, vec![60]).solid(0, 0), "alpha must be above 60");
        assert!(Mask::from_alpha(1, 1, vec![61]).solid(0, 0));
    }

    #[test]
    fn mask_from_pixmap_takes_alpha() {
        let mut p = Pixmap::new(2, 1).unwrap();
        p.data_mut().copy_from_slice(&[1, 2, 3, 77, 4, 5, 6, 0]);
        let m = Mask::from_pixmap(&p);
        assert_eq!(m.alpha_at(0, 0), Some(77));
        assert_eq!(m.alpha_at(1, 0), Some(0));
    }

    fn cands(m: &Mask) -> Vec<Candidate<'_>> {
        vec![
            Candidate { rect: PixelRect::new(0, 0, 100, 100), mask: None, scale: 1.0 },
            Candidate { rect: PixelRect::new(40, 40, 60, 60), mask: Some(m), scale: 1.0 },
        ]
    }

    #[test]
    fn pick_topmost_solid() {
        let m = mask_dot();
        let c = cands(&m);
        let mut no = |_: usize| false;
        assert_eq!(pick(&c, (50, 50), None, &mut no), Some(1));
        assert_eq!(pick(&c, (59, 41), None, &mut no), Some(0));
        assert_eq!(pick(&c, (500, 500), None, &mut no), None);
        assert_eq!(pick(&[], (0, 0), None, &mut no), None);
    }

    #[test]
    fn pick_hysteresis_and_cover() {
        let m = mask_dot();
        let c = cands(&m);
        let mut no = |_: usize| false;
        assert_eq!(pick(&c, (59, 41), Some(1), &mut no), Some(1));
        assert_eq!(pick(&c, (10, 10), Some(1), &mut no), Some(0));
        assert_eq!(pick(&c, (10, 10), Some(9), &mut no), Some(0));
        let mut yes = |_: usize| true;
        assert_eq!(pick(&c, (50, 50), None, &mut yes), None);
        assert_eq!(pick(&c, (59, 41), Some(1), &mut yes), None);
    }

    #[test]
    fn sample_step_follows_scale() {
        assert_eq!(sample_step(1.0), 3);
        assert_eq!(sample_step(2.0), 6);
        assert_eq!(sample_step(1.25), 4);
        assert_eq!(sample_step(0.0), 3);
        assert_eq!(sample_step(f64::NAN), 3);
        assert_eq!(sample_step(0.01), 1);
        assert_eq!(sane_scale(-2.0), 1.0);
        assert_eq!(sane_scale(f64::INFINITY), 1.0);
        assert_eq!(sane_scale(1.5), 1.5);
        let m = mask_dot();
        assert!(!m.solid_step(16, 10, 3));
        assert!(m.solid_step(16, 10, 6), "wider grid at scale 2 reaches the dot");
        assert!(m.solid_step(10, 10, 0), "step clamps to 1");
        let c = [Candidate { rect: PixelRect::new(0, 0, 20, 20), mask: Some(&m), scale: 2.0 }];
        assert_eq!(pick(&c, (16, 10), None, &mut |_| false), Some(0));
    }

    #[test]
    fn fullscreen_cover_needs_whole_monitor() {
        let m = PixelRect::new(0, 0, 1920, 1080);
        assert!(covered_by_fullscreen(m, &[m]));
        assert!(!covered_by_fullscreen(m, &[PixelRect::new(0, 0, 1920, 1032)]));
        assert!(!covered_by_fullscreen(PixelRect::new(1920, 0, 3840, 1080), &[m]));
        assert!(!covered_by_fullscreen(m, &[]));
        assert!(!covered_by_fullscreen(PixelRect::new(0, 0, 0, 0), &[m]));
    }

    #[test]
    fn taskbar_geometry() {
        let px = PixelRect::new(0, 0, 1920, 1080);
        let work = PixelRect::new(0, 0, 1920, 1032);
        assert_eq!(taskbar_areas(px, work), vec![PixelRect::new(0, 1032, 1920, 1080)]);
        assert_eq!(taskbar_areas(px, PixelRect::new(10, 20, 1900, 1000)).len(), 4);
        assert!(taskbar_areas(px, px).is_empty());
    }

    #[test]
    fn settle_shrink_and_touch() {
        let px = PixelRect::new(0, 0, 1920, 1080);
        let work = PixelRect::new(0, 0, 1920, 1032);
        let mons = [(px, work)];
        let f = PixelRect::new(100, 940, 200, 1040);
        assert_eq!(shrink_for_settle(f, 1.0), PixelRect::new(112, 952, 188, 1028));
        assert!(!touches_taskbar(f, 1.0, &mons, &[]), "shrunk frame ends above the bar");
        let g = PixelRect::new(100, 960, 200, 1060);
        assert!(touches_taskbar(g, 1.0, &mons, &[]));
        assert!(!touches_taskbar(g, 1.0, &mons, &[px]));
        assert!(!touches_taskbar(g, 1.0, &mons, &[PixelRect::new(-8, -8, 1928, 1088)]), "borderless fullscreen");
        // a maximized window leaves the taskbar visible: still raise
        assert!(touches_taskbar(g, 1.0, &mons, &[work]));
        assert!(touches_taskbar(g, f64::NAN, &mons, &[]));
        assert_eq!(shrink_for_settle(PixelRect::new(0, 0, 8, 8), 1.0), PixelRect::new(2, 2, 6, 6));
        assert_eq!(shrink_for_settle(f, 0.0), shrink_for_settle(f, 1.0));
        assert!(!touches_taskbar(PixelRect::new(5, 5, 5, 5), 1.0, &mons, &[]));
    }

    #[test]
    fn settle_machine() {
        assert_eq!(settle(true, false, || false), Settle::Raise);
        assert_eq!(settle(true, true, || true), Settle::Raise);
        assert_eq!(settle(true, true, || false), Settle::Nothing);
        assert_eq!(settle(false, true, || true), Settle::Lower);
        assert_eq!(settle(false, false, || true), Settle::Nothing);
    }

    #[test]
    fn opacity_bytes() {
        assert_eq!(opacity_byte(0.0), 0);
        assert_eq!(opacity_byte(1.0), 255);
        assert_eq!(opacity_byte(2.0), 255);
        assert_eq!(opacity_byte(-1.0), 0);
        assert_eq!(opacity_byte(f32::NAN), 0);
        assert_eq!(opacity_byte(0.5), 128);
    }

    #[test]
    fn tray_placement() {
        let vs = Frame::new(0.0, 0.0, 1920.0, 1080.0);
        let work = Frame::new(0.0, 48.0, 1920.0, 1032.0);
        let icon = Frame::new(1880.0, 8.0, 32.0, 32.0);
        let p = tray_pointer(icon, work, vs);
        assert!(!p.below);
        assert_eq!(p.frame, Frame::new(1680.0, 8.0, 240.0, 170.0));
        assert_eq!(p.ring_side, 46.0);
        assert_eq!(p.ring_center, Vec2::new(1896.0, 24.0));
        let icon = Frame::new(100.0, 1040.0, 24.0, 28.0);
        let p = tray_pointer(icon, Frame::new(0.0, 0.0, 1920.0, 1032.0), vs);
        assert!(p.below);
        assert_eq!(p.frame.y, 1068.0 - 170.0);
        assert_eq!(p.frame.x, 0.0);
        let p = tray_pointer(icon, work, Frame::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(p.frame.x, 0.0);
    }

    #[test]
    fn tray_timing() {
        assert_eq!(tray_pointer_opacity(-1.0), Some(0.0));
        assert_eq!(tray_pointer_opacity(f64::NAN), Some(0.0));
        assert_eq!(tray_pointer_opacity(0.1), Some(0.5));
        assert_eq!(tray_pointer_opacity(2.0), Some(1.0));
        let f = tray_pointer_opacity(5.2).unwrap();
        assert!((f - 0.5).abs() < 1e-6);
        assert_eq!(tray_pointer_opacity(5.4), None);
        assert_eq!(tray_ring_pulse(0, 0.0), (1.0, 0.9));
        assert_eq!(tray_ring_pulse(1, 0.3), (1.0, 0.0));
        let (s, o) = tray_ring_pulse(0, 0.65);
        assert!((s - 1.675).abs() < 1e-9 && (o - 0.45).abs() < 1e-9);
        let (s, _) = tray_ring_pulse(1, 0.65);
        assert_eq!(s, 1.0);
        assert_eq!(tray_ring_pulse(0, f64::NAN), (1.0, 0.0));
        assert_eq!(tray_bob(true, 0.0), 8.0);
        assert_eq!(tray_bob(false, 0.0), -8.0);
        assert!(tray_bob(true, 0.55).abs() < 1e-9);
        assert!((tray_bob(true, 1.1) - 8.0).abs() < 1e-9);
        assert_eq!(tray_bob(true, f64::NAN), 8.0);
    }
}
