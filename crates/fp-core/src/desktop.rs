//! Monitors and the pixel <-> point mapping (desktop-native 2, snowballs 2).
//!
//! Global points: X = pixel left / scale; Y = primaryTop - pixel bottom / scale,
//! where primaryTop = primary pixel height / primary scale. This reproduces the
//! original layout (Y up, origin at the primary monitor's bottom-left) and is
//! exact when all monitors share one scale.

use crate::geom::{Frame, PixelRect, Vec2};

/// One monitor as reported by the platform layer.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    /// Stable display id (device path, else GDI name).
    pub id: String,
    /// Friendly name for the UI.
    pub name: String,
    pub pixels: PixelRect,
    pub work: PixelRect,
    /// Effective DPI / 96. Values <= 0 or NaN are treated as 1.0.
    pub scale: f64,
    pub primary: bool,
}

/// A monitor placed in global points.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub info: MonitorInfo,
    /// Whole monitor in global points.
    pub points: Frame,
    /// Work area (monitor minus taskbars) in global points.
    pub work_points: Frame,
}

impl Screen {
    pub fn scale(&self) -> f64 {
        self.info.scale
    }

    /// Screen-local frame (Y up, origin at this monitor's bottom-left) of a pixel rect.
    pub fn local(&self, r: PixelRect) -> Frame {
        let s = self.info.scale;
        let p = self.info.pixels;
        Frame::new(
            (r.left - p.left) as f64 / s,
            (p.bottom - r.bottom) as f64 / s,
            r.width() as f64 / s,
            r.height() as f64 / s,
        )
    }

    /// Global-points frame of a pixel rect, using this monitor's scale.
    pub fn map(&self, r: PixelRect) -> Frame {
        self.local(r).offset(self.points.x, self.points.y)
    }

    pub fn to_points(&self, px: f64, py: f64) -> Vec2 {
        let s = self.info.scale;
        let p = self.info.pixels;
        Vec2::new(self.points.x + (px - p.left as f64) / s, self.points.y + (p.bottom as f64 - py) / s)
    }

    pub fn to_pixels(&self, v: Vec2) -> (f64, f64) {
        let s = self.info.scale;
        let p = self.info.pixels;
        (p.left as f64 + (v.x - self.points.x) * s, p.bottom as f64 - (v.y - self.points.y) * s)
    }
}

/// All monitors in global points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    screens: Vec<Screen>,
}

fn sane_scale(s: f64) -> f64 {
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

impl Layout {
    pub fn new(monitors: Vec<MonitorInfo>) -> Self {
        let monitors: Vec<MonitorInfo> = monitors
            .into_iter()
            .map(|mut m| {
                m.scale = sane_scale(m.scale);
                m
            })
            .collect();
        let primary_top = monitors
            .iter()
            .find(|m| m.primary)
            .or(monitors.first())
            .map(|m| m.pixels.height() as f64 / m.scale)
            .unwrap_or(0.0);
        let screens = monitors
            .into_iter()
            .map(|info| {
                let s = info.scale;
                let p = info.pixels;
                let points = Frame::new(
                    p.left as f64 / s,
                    primary_top - p.bottom as f64 / s,
                    p.width() as f64 / s,
                    p.height() as f64 / s,
                );
                let w = info.work;
                let work_points = Frame::new(
                    points.x + (w.left - p.left) as f64 / s,
                    points.y + (p.bottom - w.bottom) as f64 / s,
                    w.width() as f64 / s,
                    w.height() as f64 / s,
                );
                Screen { info, points, work_points }
            })
            .collect();
        Self { screens }
    }

    pub fn screens(&self) -> &[Screen] {
        &self.screens
    }

    pub fn is_empty(&self) -> bool {
        self.screens.is_empty()
    }

    /// Primary monitor, else the first one.
    pub fn primary(&self) -> Option<&Screen> {
        self.screens.iter().find(|s| s.info.primary).or(self.screens.first())
    }

    pub fn by_id(&self, id: &str) -> Option<&Screen> {
        self.screens.iter().find(|s| s.info.id == id)
    }

    fn nearest(&self, d: impl Fn(&Screen) -> f64) -> Option<&Screen> {
        let mut best: Option<(&Screen, f64)> = None;
        for s in &self.screens {
            let v = d(s);
            if best.is_none_or(|(_, b)| v < b) {
                best = Some((s, v));
            }
        }
        best.map(|(s, _)| s)
    }

    /// Monitor containing the pixel (half-open), else the nearest one.
    pub fn at_pixel(&self, px: f64, py: f64) -> Option<&Screen> {
        self.screens
            .iter()
            .find(|s| s.info.pixels.contains(px, py))
            .or_else(|| self.nearest(|s| s.info.pixels.dist_sq(px, py)))
    }

    /// Monitor containing the point (half-open), else the nearest one.
    pub fn at_point(&self, p: Vec2) -> Option<&Screen> {
        self.screens.iter().find(|s| s.points.contains(p)).or_else(|| self.nearest(|s| s.points.dist_sq(p)))
    }

    /// Pixels -> global points. Unchanged when there are no monitors.
    pub fn to_points(&self, px: f64, py: f64) -> Vec2 {
        match self.at_pixel(px, py) {
            Some(s) => s.to_points(px, py),
            None => Vec2::new(px, py),
        }
    }

    /// Global points -> pixels. Unchanged when there are no monitors.
    pub fn to_pixels(&self, p: Vec2) -> (f64, f64) {
        match self.at_point(p) {
            Some(s) => s.to_pixels(p),
            None => (p.x, p.y),
        }
    }

    /// Window rect in pixels for a frame in global points, using the monitor
    /// nearest to the frame centre (snowballs 2 `ToPixels`). Size at least 1 px.
    pub fn frame_to_pixels(&self, f: Frame) -> PixelRect {
        let (left, top, scale) = match self.nearest(|s| s.points.dist_sq(f.center())) {
            Some(s) => {
                let sc = s.info.scale;
                let p = s.info.pixels;
                (p.left as f64 + (f.min_x() - s.points.x) * sc, p.bottom as f64 - (f.max_y() - s.points.y) * sc, sc)
            }
            None => (f.min_x(), -f.max_y(), 1.0),
        };
        let w = ((f.w * scale).round() as i32).max(1);
        let h = ((f.h * scale).round() as i32).max(1);
        let (l, t) = (left.round() as i32, top.round() as i32);
        PixelRect::new(l, t, l + w, t + h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, px: PixelRect, scale: f64, primary: bool) -> MonitorInfo {
        let work = PixelRect::new(px.left, px.top, px.right, px.bottom - 48);
        MonitorInfo { id: id.into(), name: id.into(), pixels: px, work, scale, primary }
    }

    fn two() -> Layout {
        Layout::new(vec![
            mon("b", PixelRect::new(1920, 0, 3840, 1080), 1.0, false),
            mon("a", PixelRect::new(0, 0, 1920, 1080), 1.0, true),
        ])
    }

    #[test]
    fn frames_and_primary() {
        let l = two();
        assert!(!l.is_empty());
        assert_eq!(l.primary().unwrap().info.id, "a");
        assert_eq!(l.by_id("b").unwrap().points, Frame::new(1920.0, 0.0, 1920.0, 1080.0));
        assert!(l.by_id("zz").is_none());
        let a = l.by_id("a").unwrap();
        assert_eq!(a.points, Frame::new(0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(a.work_points, Frame::new(0.0, 48.0, 1920.0, 1032.0));
        assert_eq!(a.scale(), 1.0);
        assert_eq!(l.screens().len(), 2);
    }

    #[test]
    fn scaled_and_fallback_primary() {
        // no primary flag: first monitor is the reference; bad scale -> 1.0
        let l = Layout::new(vec![
            mon("x", PixelRect::new(0, 0, 2880, 1800), 2.0, false),
            mon("y", PixelRect::new(2880, 0, 3880, 1000), f64::NAN, false),
        ]);
        let x = &l.screens()[0];
        assert_eq!(x.points, Frame::new(0.0, 0.0, 1440.0, 900.0));
        assert_eq!(l.primary().unwrap().info.id, "x");
        assert_eq!(l.screens()[1].info.scale, 1.0);
        // local / map
        let r = PixelRect::new(200, 1000, 600, 1400);
        assert_eq!(x.local(r), Frame::new(100.0, 200.0, 200.0, 200.0));
        assert_eq!(x.map(r), Frame::new(100.0, 200.0, 200.0, 200.0));
    }

    #[test]
    fn conversions() {
        let l = two();
        let p = l.to_points(100.0, 1000.0);
        assert_eq!(p, Vec2::new(100.0, 80.0));
        assert_eq!(l.to_pixels(p), (100.0, 1000.0));
        // off-screen pixel -> nearest monitor
        assert_eq!(l.at_pixel(5000.0, 10.0).unwrap().info.id, "b");
        assert_eq!(l.at_point(Vec2::new(-50.0, 10.0)).unwrap().info.id, "a");
        assert_eq!(l.at_point(Vec2::new(2000.0, 10.0)).unwrap().info.id, "b");
        let empty = Layout::default();
        assert_eq!(empty.to_points(3.0, 4.0), Vec2::new(3.0, 4.0));
        assert_eq!(empty.to_pixels(Vec2::new(3.0, 4.0)), (3.0, 4.0));
        assert!(empty.primary().is_none());
    }

    #[test]
    fn frame_to_pixels_rounds_and_flips() {
        let l = two();
        let r = l.frame_to_pixels(Frame::new(10.0, 1000.0, 20.4, 0.2));
        assert_eq!(r, PixelRect::new(10, 80, 30, 81));
        let e = Layout::default().frame_to_pixels(Frame::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(e, PixelRect::new(1, -6, 4, -2));
    }
}
