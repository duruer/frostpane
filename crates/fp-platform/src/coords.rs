//! Pixel spaces of backends whose OS works in points (porting design 2.4).
//!
//! The core works in virtual-desktop pixels (Y down) plus a per-monitor
//! scale. macOS and Wayland publish a synthetic pixel space: OS points times
//! one global scale `S`, so that the core's global points equal the OS's
//! points and pixel rects of mixed-scale monitors never overlap.

use fp_core::geom::PixelRect;

/// The global scale: the largest finite positive monitor scale, else 1.
pub fn global_scale(scales: &[f64]) -> f64 {
    scales
        .iter()
        .copied()
        .filter(|s| s.is_finite() && *s > 0.0)
        .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))
        .unwrap_or(1.0)
}

fn px(v: f64) -> i32 {
    v.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

/// A Y-down point rect (`x`, `y` top-left; Wayland logical, Quartz) in pixels.
pub fn points_to_pixels(x: f64, y: f64, w: f64, h: f64, s: f64) -> PixelRect {
    PixelRect::new(px(x * s), px(y * s), px((x + w) * s), px((y + h) * s))
}

/// A Y-up Cocoa rect (`x`, `y` bottom-left) in pixels; `primary_h` is the
/// primary screen's height in points.
pub fn cocoa_to_pixels(x: f64, y: f64, w: f64, h: f64, primary_h: f64, s: f64) -> PixelRect {
    points_to_pixels(x, primary_h - y - h, w, h, s)
}

/// A pixel position back to Y-up Cocoa points (cursor, window origins).
pub fn pixels_to_cocoa(px_x: i32, px_y: i32, primary_h: f64, s: f64) -> (f64, f64) {
    let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    (f64::from(px_x) / s, primary_h - f64::from(px_y) / s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_is_the_largest() {
        assert_eq!(global_scale(&[1.0, 2.0, 1.5]), 2.0);
        assert_eq!(global_scale(&[f64::NAN, -1.0, 0.0]), 1.0);
        assert_eq!(global_scale(&[]), 1.0);
        assert_eq!(global_scale(&[1.25]), 1.25);
    }

    #[test]
    fn y_down_points() {
        assert_eq!(points_to_pixels(0.0, 0.0, 1440.0, 900.0, 2.0), PixelRect::new(0, 0, 2880, 1800));
        assert_eq!(points_to_pixels(1440.0, -100.0, 1920.0, 1080.0, 2.0), PixelRect::new(2880, -200, 6720, 1960));
        assert_eq!(points_to_pixels(0.0, 0.0, 1e12, 1.0, 1e3).right, i32::MAX);
    }

    #[test]
    fn cocoa_round_trip() {
        // Primary 1440x900 at the origin; an external screen above it.
        let primary = cocoa_to_pixels(0.0, 0.0, 1440.0, 900.0, 900.0, 2.0);
        assert_eq!(primary, PixelRect::new(0, 0, 2880, 1800));
        let above = cocoa_to_pixels(0.0, 900.0, 1920.0, 1080.0, 900.0, 2.0);
        assert_eq!(above, PixelRect::new(0, -2160, 3840, 0));
        assert_eq!(pixels_to_cocoa(200, 1800, 900.0, 2.0), (100.0, 0.0));
        assert_eq!(pixels_to_cocoa(10, 0, 900.0, f64::NAN), (10.0, 900.0));
    }
}
