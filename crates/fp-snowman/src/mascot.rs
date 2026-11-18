//! Small decorative snowman mascot (snowman F10). Drawn in a Y-down 0..1 box
//! scaled by the height `side`; the element is `0.8 * side` wide.

use crate::art::{build, c, ca, disc};
use fp_paint::Sketch;
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{brand, rgba};

/// Balls bottom to top as `(centre y, diameter)` in the Y-down unit box.
const BALLS: [(f32, f32); 3] = [(0.74, 0.46), (0.40, 0.34), (0.16, 0.26)];

/// `side` = height in points, element box `0.8 * side` x `side`. Used by the
/// settings/onboarding pages (`fp-ui`) and GroundLife (`fp-characters`).
pub fn paint_mascot(pixmap: &mut Pixmap, scale: f32, side: f64, waving: bool) {
    let top = pixmap.height() as f32 / scale;
    let side = side.max(1.0) as f32;
    let px = 1.0 / side;
    let mut s = Sketch::new(pixmap, scale);
    s.translate(0.0, top);
    s.scale_by(side, -side);
    // The beanie and pom rise above the 0..1 box; fit everything inside it.
    s.translate(0.02, 0.07);
    s.scale_by(0.93, 0.93);

    // Soft drop shadow under the balls: #4D6699 at 0.18, blur 0.06 * side,
    // 0.03 * side down. Four widening layers stand in for the blur.
    let shadow = ca(0x4D6699, 1.0 - (1.0f64 - 0.18).powf(0.25));
    for (cy, diameter) in BALLS {
        for k in 0..4 {
            s.circle(0.4, cy + 0.03, diameter / 2.0 + 0.06 * (k as f32 / 3.0 - 0.5), shadow);
        }
    }

    // Twig arms first, behind the body.
    let wood = c(0x6B472E);
    let thick = (1.5f32).max(0.022 * side) * px;
    let twig = |s: &mut Sketch, x: f32, y: f32, degrees: f32| {
        s.at(x, y, degrees.to_radians(), 1.0, |s| s.rect(-0.13, -thick / 2.0, 0.26, thick, thick / 2.0, wood));
    };
    twig(&mut s, 0.15, 0.37, 28.0);
    if waving {
        twig(&mut s, 0.65, 0.30, -48.0);
    } else {
        twig(&mut s, 0.65, 0.37, -28.0);
    }

    let frost = (1.0f32).max(0.012 * side) * px;
    let ball = |s: &mut Sketch, cy: f32, diameter: f32| {
        let r = diameter / 2.0;
        let path = disc(0.4, cy, r);
        s.across(&path, &[c(0xFFFFFF), c(0xE6EDFA)], None, (0.4 - r, cy - r), (0.4 + r, cy + r));
        s.ring(0.4 - r + frost / 2.0, cy - r + frost / 2.0, diameter - frost, diameter - frost, c(brand::FROST), frost);
    };
    let [bottom, middle, head] = BALLS;
    ball(&mut s, bottom.0, bottom.1);
    ball(&mut s, middle.0, middle.1);
    for y in [0.3775, 0.4825] {
        s.circle(0.4, y, 0.0175, rgba(0.0, 0.0, 0.0, 0.75));
    }
    ball(&mut s, head.0, head.1);

    // Face.
    for x in [0.354, 0.446] {
        s.circle(x, 0.145, 0.016, rgba(0.0, 0.0, 0.0, 0.85));
    }
    s.polygon(&[(0.40, 0.164), (0.49, 0.18), (0.40, 0.196)], c(0xFF8C26));
    for x in [0.3225, 0.4775] {
        s.circle(x, 0.195, 0.0175, ca(0xFF2D55, 0.35));
    }

    // Beanie and pom.
    let accent = c(brand::ACCENT);
    let beanie = build(|t| {
        t.move_to(0.29, 0.03)
            .curve(0.40, -0.035, 0.29, -0.01, 0.33, -0.035)
            .curve(0.51, 0.03, 0.47, -0.035, 0.51, -0.01)
            .quad(0.49, 0.05, 0.51, 0.04)
            .line(0.31, 0.05)
            .quad(0.29, 0.03, 0.29, 0.04)
            .close();
    });
    s.fill(&beanie, accent);
    s.circle(0.4, -0.0375, 0.0275, c(0xFFFFFF));

    // Scarf band and tail.
    s.rect(0.26, 0.23, 0.28, 0.05, 0.025, accent);
    s.rect(0.46, 0.26, 0.05, 0.12, 0.01, accent);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(side: f64, waving: bool) -> Pixmap {
        let mut pm = Pixmap::new((0.8 * side * 2.0).ceil() as u32, (side * 2.0).ceil() as u32).unwrap();
        paint_mascot(&mut pm, 2.0, side, waving);
        pm
    }

    #[test]
    fn mascot_stands_in_its_box() {
        let pm = draw(92.0, false);
        let (w, h) = (pm.width(), pm.height());
        let lit = |x: u32, y: u32| pm.pixel(x, y).unwrap().alpha();
        // Bottom ball near the floor, centred on x = 0.4.
        assert!(lit((0.4 * 92.0 * 2.0) as u32, (0.76 * 92.0 * 2.0) as u32) > 200);
        // Beanie and pom stay inside the box.
        assert!(lit((0.4 * 92.0 * 2.0) as u32, 14) > 0);
        assert_eq!(lit(w - 2, 2), 0);
        assert_eq!(lit(2, h - 2), 0);
    }

    #[test]
    fn waving_raises_the_right_arm() {
        let rest = draw(92.0, false);
        let wave = draw(92.0, true);
        assert_ne!(rest.data(), wave.data());
        assert_eq!(rest.data(), draw(92.0, false).data());
    }

    #[test]
    fn tiny_side_is_safe() {
        let mut pm = Pixmap::new(4, 4).unwrap();
        paint_mascot(&mut pm, 1.0, 0.0, true);
        paint_mascot(&mut pm, 1.0, f64::NAN, true);
        paint_mascot(&mut pm, 1.0, f64::INFINITY, false);
    }

    #[test]
    fn soft_shadow_falls_below_the_bottom_ball() {
        let pm = draw(92.0, false);
        let side = 92.0 * 2.0;
        // Just under the bottom ball's lowest point (box y 0.97, inset).
        let x = ((0.02 + 0.93 * 0.4) * side) as u32;
        let y = ((0.07 + 0.93 * 0.982) * side) as u32;
        let p = pm.pixel(x, y).unwrap();
        assert!(p.alpha() > 5 && p.alpha() < 80, "{}", p.alpha());
        assert!(p.blue() > p.red());
        // Far to the side of the ball there is no shadow.
        assert_eq!(pm.pixel(2, y).unwrap().alpha(), 0);
    }
}
