//! Geometry in the single Frostpane convention.
//!
//! * Points (DIP, 1/96 inch), **Y up**. Global points: the desktop world.
//!   Screen-local points: global minus the screen frame's min corner.
//! * Pixels: raw virtual-desktop device pixels, Y down ([`PixelRect`]).

use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Neg, Sub};

/// A 2D point or vector in points (Y up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn len_sq(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn len(self) -> f64 {
        self.len_sq().sqrt()
    }

    pub fn dist(self, o: Vec2) -> f64 {
        (self - o).len()
    }

    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// Unit vector in the same direction; the zero vector stays zero.
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l > 0.0 { self * (1.0 / l) } else { Vec2::ZERO }
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f64) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// Axis-aligned box in points: `(x, y)` is the bottom-left (min) corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Frame {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn min_x(&self) -> f64 {
        self.x
    }
    pub fn min_y(&self) -> f64 {
        self.y
    }
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.mid_x(), self.y + self.h / 2.0)
    }
    pub fn min(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }

    pub fn offset(&self, dx: f64, dy: f64) -> Frame {
        Frame::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// `(x+dx, y+dy, w-2dx, h-2dy)`; negative values grow the frame.
    pub fn inset(&self, dx: f64, dy: f64) -> Frame {
        Frame::new(self.x + dx, self.y + dy, self.w - 2.0 * dx, self.h - 2.0 * dy)
    }

    /// Half-open point test: min inclusive, max exclusive.
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min_x() && p.x < self.max_x() && p.y >= self.min_y() && p.y < self.max_y()
    }

    /// Closed containment of another frame.
    pub fn contains_frame(&self, o: &Frame) -> bool {
        o.min_x() >= self.min_x() && o.max_x() <= self.max_x() && o.min_y() >= self.min_y() && o.max_y() <= self.max_y()
    }

    /// Strict overlap: frames that only touch do not intersect.
    pub fn intersects(&self, o: &Frame) -> bool {
        self.min_x() < o.max_x() && o.min_x() < self.max_x() && self.min_y() < o.max_y() && o.min_y() < self.max_y()
    }

    pub fn to_rect(&self) -> Rect4 {
        Rect4::new(self.min_x(), self.min_y(), self.max_x(), self.max_y())
    }

    /// Squared distance from a point to this frame (0 inside).
    pub fn dist_sq(&self, p: Vec2) -> f64 {
        let dx = (self.min_x() - p.x).max(0.0).max(p.x - self.max_x());
        let dy = (self.min_y() - p.y).max(0.0).max(p.y - self.max_y());
        dx * dx + dy * dy
    }
}

/// Rectangle given by its corners `(x0, y0)`..`(x1, y1)` in points, Y up.
/// Used for blockers, covers and occluders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect4 {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Rect4 {
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    /// Half-open: `x >= x0 && x < x1 && y >= y0 && y < y1`.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    pub fn to_frame(&self) -> Frame {
        Frame::new(self.x0, self.y0, self.x1 - self.x0, self.y1 - self.y0)
    }

    pub fn offset(&self, dx: f64, dy: f64) -> Rect4 {
        Rect4::new(self.x0 + dx, self.y0 + dy, self.x1 + dx, self.y1 + dy)
    }
}

/// Raw device-pixel rectangle, Y down (Win32 `RECT` semantics).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    /// Half-open point test.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.left as f64 && x < self.right as f64 && y >= self.top as f64 && y < self.bottom as f64
    }
    /// Squared distance from a pixel position to this rect (0 inside).
    pub fn dist_sq(&self, x: f64, y: f64) -> f64 {
        let dx = (self.left as f64 - x).max(0.0).max(x - self.right as f64);
        let dy = (self.top as f64 - y).max(0.0).max(y - self.bottom as f64);
        dx * dx + dy * dy
    }
    pub fn is_empty(&self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_ops() {
        let a = Vec2::new(3.0, 4.0);
        let b = Vec2::new(1.0, 1.0);
        assert_eq!(a.len(), 5.0);
        assert_eq!(a.len_sq(), 25.0);
        assert_eq!((a + b), Vec2::new(4.0, 5.0));
        assert_eq!((a - b), Vec2::new(2.0, 3.0));
        assert_eq!(a * 2.0, Vec2::new(6.0, 8.0));
        assert_eq!(-a, Vec2::new(-3.0, -4.0));
        assert_eq!(a.dot(b), 7.0);
        assert_eq!(a.dist(Vec2::ZERO), 5.0);
        assert!((a.normalized() - Vec2::new(0.6, 0.8)).len() < 1e-12);
        assert_eq!(Vec2::ZERO.normalized(), Vec2::ZERO);
    }

    #[test]
    fn frame_queries() {
        let f = Frame::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!((f.min_x(), f.min_y(), f.max_x(), f.max_y()), (10.0, 20.0, 110.0, 70.0));
        assert_eq!(f.mid_x(), 60.0);
        assert_eq!(f.center(), Vec2::new(60.0, 45.0));
        assert_eq!(f.min(), Vec2::new(10.0, 20.0));
        assert_eq!(f.offset(1.0, 2.0), Frame::new(11.0, 22.0, 100.0, 50.0));
        assert_eq!(f.inset(1.0, 1.0), Frame::new(11.0, 21.0, 98.0, 48.0));
        assert!(f.contains(Vec2::new(10.0, 20.0)));
        assert!(!f.contains(Vec2::new(110.0, 30.0)));
        assert!(!f.contains(Vec2::new(50.0, 70.0)));
        assert!(!f.contains(Vec2::new(9.0, 30.0)));
        assert!(f.contains_frame(&f));
        assert!(!f.contains_frame(&f.inset(-1.0, -1.0)));
        // touching edges do not intersect
        assert!(!f.intersects(&Frame::new(110.0, 20.0, 5.0, 5.0)));
        assert!(f.intersects(&Frame::new(109.0, 69.0, 5.0, 5.0)));
        assert_eq!(f.to_rect(), Rect4::new(10.0, 20.0, 110.0, 70.0));
        assert_eq!(f.dist_sq(Vec2::new(50.0, 30.0)), 0.0);
        assert_eq!(f.dist_sq(Vec2::new(113.0, 74.0)), 25.0);
        assert_eq!(f.dist_sq(Vec2::new(7.0, 16.0)), 25.0);
    }

    #[test]
    fn rect4() {
        let r = Rect4::new(0.0, 0.0, 10.0, 5.0);
        assert!(r.contains(0.0, 0.0));
        assert!(!r.contains(10.0, 1.0));
        assert!(!r.contains(1.0, 5.0));
        assert_eq!(r.to_frame(), Frame::new(0.0, 0.0, 10.0, 5.0));
        assert_eq!(r.offset(1.0, 1.0), Rect4::new(1.0, 1.0, 11.0, 6.0));
    }

    #[test]
    fn pixel_rect() {
        let r = PixelRect::new(0, 0, 1920, 1080);
        assert_eq!((r.width(), r.height()), (1920, 1080));
        assert!(r.contains(0.0, 0.0));
        assert!(!r.contains(1920.0, 0.0));
        assert_eq!(r.dist_sq(10.0, 10.0), 0.0);
        assert_eq!(r.dist_sq(1923.0, 1084.0), 25.0);
        assert_eq!(r.dist_sq(-3.0, -4.0), 25.0);
        assert!(!r.is_empty());
        assert!(PixelRect::new(5, 5, 5, 9).is_empty());
        assert!(PixelRect::new(5, 5, 9, 5).is_empty());
    }
}
