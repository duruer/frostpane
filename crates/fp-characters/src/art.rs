//! Shared procedural art: reindeer, sleigh, Santa, elf. Original designs in
//! Y-up points; every function draws relative to the current transform.

use fp_paint::tiny_skia::{Color, Path};
use fp_paint::{Sketch, Trace, hex, rgba};

pub(crate) fn c(rgb: u32) -> Color {
    hex(rgb)
}

/// Closed polygon as a path (the point list is never empty).
pub(crate) fn poly(points: &[(f32, f32)]) -> Path {
    let mut t = Trace::new();
    t.lines(points).close();
    t.finish().expect("polygon with points")
}

/// Disc path.
pub(crate) fn disc(x: f32, y: f32, r: f32) -> Path {
    fp_paint::tiny_skia::PathBuilder::from_circle(x, y, r).expect("positive radius")
}

/// Hip x offsets of the four reindeer legs (deer-local points).
pub(crate) const HIPS: [f32; 4] = [13.0, 19.0, 33.0, 39.0];
/// Deer x offsets inside the sleigh team (sleigh-local points).
pub(crate) const DEER_AT: [f32; 3] = [96.0, 152.0, 208.0];

const FUR: u32 = 0x8C5C36;
const FUR_DARK: u32 = 0x6B4529;

/// One reindeer in a 58 x 66 box, feet at the bottom. `legs` are the four
/// leg angles, `bob` lifts body and head, `head` tilts the head about the
/// neck, `hat` adds a Santa hat.
pub(crate) fn deer(s: &mut Sketch, legs: [f32; 4], bob: f32, head: f32, hat: bool) {
    for (i, angle) in legs.iter().enumerate() {
        let tone = if i % 2 == 0 { FUR_DARK } else { FUR };
        s.at(HIPS[i] + 2.25, 28.0, *angle, 1.0, |s| {
            s.rect(-2.25, -22.0, 4.5, 22.0, 2.0, c(tone));
            s.rect(-2.25, -22.0, 4.5, 4.0, 1.5, c(0x291F1A));
        });
    }
    s.at(0.0, bob, 0.0, 1.0, |s| {
        s.ellipse(3.0, 26.0, 38.0, 20.0, c(FUR));
        s.ellipse(9.0, 26.0, 26.0, 9.0, c(0xD1A878));
        s.circle(4.0, 39.0, 3.5, c(0xFFFFFF));
        s.polygon(&[(33.0, 38.0), (45.0, 34.0), (50.0, 54.0), (40.0, 54.0)], c(FUR));
        s.rect(34.0, 40.0, 13.0, 4.0, 2.0, c(0xCC2129));
        s.circle(41.0, 40.0, 2.4, c(0xF2BD40));
        s.at(41.0, 44.0, head, 1.0, |s| {
            s.ellipse(-2.0, 6.0, 20.0, 13.0, c(FUR));
            s.circle(17.0, 11.0, 3.2, c(0xCC2129));
            s.circle(9.0, 14.0, 1.4, c(0x171C3D));
            s.polyline(&[(3.0, 18.0), (1.0, 26.0), (-3.0, 30.0)], c(0xDBBF8F), 1.8);
            s.polyline(&[(1.0, 24.0), (5.0, 28.0)], c(0xDBBF8F), 1.8);
            s.polyline(&[(9.0, 18.0), (11.0, 26.0), (15.0, 30.0)], c(0xDBBF8F), 1.8);
            if hat {
                s.polygon(&[(0.0, 17.0), (14.0, 17.0), (5.0, 30.0)], c(0xCC2129));
                s.rect(-1.0, 15.5, 16.0, 3.5, 1.5, c(0xFFFFFF));
                s.circle(5.0, 31.0, 2.6, c(0xFFFFFF));
            }
        });
    });
}

/// The sleigh with optional Santa, in the 266 x 158 team space (runner
/// bottom at y = 0, nose to the right). `arm` is Santa's arm angle.
pub(crate) fn sleigh(s: &mut Sketch, santa: bool, arm: f32) {
    // runner and struts
    s.polyline(&[(2.0, 6.0), (76.0, 6.0), (88.0, 8.0), (94.0, 18.0)], c(0xF2BD40), 2.6);
    s.line(22.0, 6.0, 22.0, 16.0, c(0xF2BD40), 2.6);
    s.line(60.0, 6.0, 60.0, 16.0, c(0xF2BD40), 2.6);
    // body, stripe, outline
    let body = poly(&[(8.0, 16.0), (78.0, 16.0), (92.0, 34.0), (94.0, 48.0), (80.0, 40.0), (8.0, 40.0)]);
    s.fill(&body, c(0xCC2129));
    s.inside(&body, |s| s.rect(0.0, 12.0, 100.0, 6.0, 0.0, c(0x94121C)));
    s.stroke(&body, c(0xF2BD40), 1.6);
    // sack
    s.ellipse(8.0, 38.0, 30.0, 26.0, c(0xEDF2FF));
    s.circle(16.0, 58.0, 4.0, c(0xFFFFFF));
    s.circle(28.0, 56.0, 3.0, c(0xFFFFFF));
    if santa {
        s.ellipse(40.0, 34.0, 28.0, 30.0, c(0xCC2129));
        s.rect(40.0, 46.0, 28.0, 3.5, 1.0, rgba(0.15, 0.12, 0.13, 1.0));
        s.circle(54.0, 72.0, 8.0, c(0xF5D5B8));
        s.polygon(&[(46.0, 70.0), (62.0, 70.0), (54.0, 52.0)], c(0xFFFFFF));
        s.circle(54.0, 70.0, 2.0, c(0xE8A0A0));
        s.polygon(&[(45.0, 76.0), (63.0, 76.0), (52.0, 94.0)], c(0xCC2129));
        s.rect(44.0, 74.0, 20.0, 4.0, 2.0, c(0xFFFFFF));
        s.circle(52.0, 95.0, 3.0, c(0xFFFFFF));
        s.at(50.0, 56.0, arm, 1.0, |s| {
            s.rect(0.0, -3.5, 15.0, 7.0, 3.0, c(0xE02B33));
            s.rect(11.0, -4.0, 4.0, 8.0, 1.5, c(0xFFFFFF));
            s.circle(18.0, 0.0, 4.0, c(0x21854C));
        });
    }
    // reins
    s.polyline(&[(80.0, 42.0), (134.0, 34.0), (190.0, 34.0), (246.0, 34.0)], c(0xB88533), 1.2);
}

/// Pose of one elf (characters 4 `Show`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ElfLook {
    pub t: f64,
    pub walking: bool,
    pub carrying: bool,
    pub waving: bool,
    pub hopping: bool,
}

/// Arm and leg angles for a look.
pub(crate) fn elf_angles(l: &ElfLook) -> ([f32; 2], [f32; 2]) {
    let s = (11.0 * l.t).sin();
    let swing = if l.walking { 0.6 * s } else { 0.0 };
    let bend = if l.hopping { 0.8 } else { 0.0 };
    let legs = [(swing + bend) as f32, (-swing + 0.6 * bend) as f32];
    let mut arms = [(-swing * 0.8) as f32, (swing * 0.8) as f32];
    if l.carrying {
        let k = 0.05 * (3.0 * l.t).sin();
        arms = [(2.7916 + k) as f32, (-2.7916 - k) as f32];
    } else if l.waving {
        arms[1] = (-2.6416 + 0.35 * (9.0 * l.t).sin()) as f32;
    }
    if l.hopping && !l.carrying {
        arms = [0.9, -0.9];
    }
    (legs, arms)
}

/// One elf in a 34 x 40 box with the feet at the origin of the current
/// transform (x centred).
pub(crate) fn elf(s: &mut Sketch, l: &ElfLook) {
    let (legs, arms) = elf_angles(l);
    let lift = if l.walking { 1.2 * (11.0 * l.t).sin().abs() as f32 } else { 0.0 };
    for (i, a) in legs.iter().enumerate() {
        let x = if i == 0 { -4.0 } else { 4.0 };
        s.at(x, 15.0 + lift, *a, 1.0, |s| {
            s.rect(-2.0, -15.0, 4.0, 15.0, 1.5, c(0xCC212B));
            s.rect(-2.0, -9.0, 4.0, 2.0, 0.0, c(0xFFFFFF));
            s.rect(-2.5, -15.0, 6.5, 4.5, 2.0, c(0x21804C));
        });
    }
    s.at(0.0, lift, 0.0, 1.0, |s| {
        s.rect(-8.0, 13.0, 16.0, 19.0, 4.0, c(0xCC212B));
        s.rect(-8.0, 19.0, 16.0, 3.0, 0.0, c(0xF2BF45));
        s.rect(-2.0, 18.5, 4.0, 4.0, 1.0, c(0x33333D));
        s.line(-5.0, 31.5, 5.0, 31.5, c(0xF4F8FF), 1.6);
        s.ellipse(-4.5, 31.0, 9.0, 8.5, c(0xFAD6B8));
        s.polygon(&[(-4.5, 35.0), (-8.5, 38.0), (-4.0, 36.5)], c(0xFAD6B8));
        s.polygon(&[(4.5, 35.0), (8.5, 38.0), (4.0, 36.5)], c(0xFAD6B8));
        s.circle(-1.8, 35.0, 0.8, c(0x33333D));
        s.circle(1.8, 35.0, 0.8, c(0x33333D));
        s.polygon(&[(-5.0, 38.0), (5.0, 38.0), (3.0, 47.0)], c(0x21804C));
        s.rect(-5.5, 37.0, 11.0, 2.5, 1.0, c(0xFFFFFF));
        s.circle(3.0, 47.5, 1.8, c(0xFFFFFF));
        for (i, a) in arms.iter().enumerate() {
            let x = if i == 0 { -8.0 } else { 8.0 };
            s.at(x, 30.0, *a, 1.0, |s| {
                s.rect(-2.0, -11.0, 4.0, 11.0, 1.8, c(0xCC212B));
                s.rect(-2.2, -12.0, 4.4, 2.0, 0.0, c(0xFFFFFF));
                s.circle(0.0, -13.0, 2.0, c(0xFAD6B8));
            });
        }
    });
}
