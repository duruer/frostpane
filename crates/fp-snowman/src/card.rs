//! Dress-up card model (snowman F4): four tabs of eight chips, the F3 toggle
//! rules, chip icons (F5 framing) and card placement. The egui window is in
//! `fp-ui`.

use crate::art::{box_path, draw_ball, puppet_body};
use crate::{Arms, Decor, Face, Hat, Look, Outfit};
use fp_paint::Sketch;
use fp_paint::tiny_skia::Pixmap;

/// Card body size in points.
pub const CARD_W: f64 = 340.0;
pub const CARD_H: f64 = 290.0;
/// Room around the card for its shadow; the window is `CARD + 2 * ROOM`.
pub const ROOM: f64 = 22.0;
/// Chips per tab.
pub const CHIPS: usize = 8;
/// Chip icon box in points.
pub const ICON: f64 = 44.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Hat,
    Face,
    Body,
    Clothes,
}

impl Tab {
    /// Tabs in UI order (the initial tab is the first, `Hat`).
    pub const ALL: [Tab; 4] = [Tab::Hat, Tab::Face, Tab::Body, Tab::Clothes];
}

/// Apply a chip click with the F3 toggle rules: hat and outfit chips toggle
/// off when the selected one is clicked again; face and arms only replace.
/// An out-of-range index changes nothing.
pub fn apply(look: Look, tab: Tab, index: usize) -> Look {
    let mut look = look;
    match tab {
        Tab::Hat => {
            if let Some(&hat) = Hat::ALL.get(index) {
                look.hat = if look.hat == Some(hat) { None } else { Some(hat) };
            }
        }
        Tab::Face => {
            if let Some(&face) = Face::ALL.get(index) {
                look.face = face;
            }
        }
        Tab::Body => {
            if let Some(&arms) = Arms::ALL.get(index) {
                look.arms = arms;
            }
        }
        Tab::Clothes => {
            if let Some(&outfit) = Outfit::ALL.get(index) {
                look.outfit = if look.outfit == Some(outfit) { None } else { Some(outfit) };
            }
        }
    }
    look
}

/// Index of the chip that is selected on `tab`, if any.
pub fn selected(look: &Look, tab: Tab) -> Option<usize> {
    match tab {
        Tab::Hat => Hat::ALL.iter().position(|&h| Some(h) == look.hat),
        Tab::Face => Face::ALL.iter().position(|&f| f == look.face),
        Tab::Body => Arms::ALL.iter().position(|&a| a == look.arms),
        Tab::Clothes => Outfit::ALL.iter().position(|&o| Some(o) == look.outfit),
    }
}

/// "Start over" is disabled while the look already is the classic one.
pub fn can_reset(look: &Look) -> bool {
    !look.is_classic()
}

/// Start over resets the look to classic.
pub fn reset() -> Look {
    Look::classic()
}

/// Look used for a chip icon: classic face, sticks, no hat, no outfit, except
/// the slot being shown. `None` for an out-of-range index.
pub fn chip_look(tab: Tab, index: usize) -> Option<Look> {
    let base = Look { hat: None, outfit: None, ..Look::classic() };
    if index >= CHIPS {
        return None;
    }
    Some(match tab {
        Tab::Hat => Look { hat: Some(Hat::ALL[index]), ..base },
        Tab::Face => Look { face: Face::ALL[index], ..base },
        Tab::Body => Look { arms: Arms::ALL[index], ..base },
        Tab::Clothes => Look { outfit: Some(Outfit::ALL[index]), ..base },
    })
}

/// Chip icon: 44 x 44 points, transparent, clipped to the box, painted with
/// the normal snowball art in the F5 framing of each slot. `None` for an
/// out-of-range index or a scale too large for a pixmap.
pub fn chip_icon(tab: Tab, index: usize, scale: f32) -> Option<Pixmap> {
    let look = chip_look(tab, index)?;
    let n = (ICON as f32 * scale).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(n, n)?;
    let mut s = Sketch::new(&mut pixmap, scale);
    let frame = box_path(0.0, 0.0, ICON as f32, ICON as f32);
    s.inside(&frame, |s| match tab {
        Tab::Hat => s.at(22.0, -3.0, 0.0, 1.0, |s| draw_ball(s, 15.0, 0.0, 7, Decor::Head, &look)),
        Tab::Face => s.at(19.0, 20.0, 0.0, 1.0, |s| draw_ball(s, 16.0, 0.0, 7, Decor::Head, &look)),
        Tab::Body => s.at(20.0, 17.0, 0.0, 1.0, |s| draw_ball(s, 11.0, 0.0, 7, Decor::Body, &look)),
        Tab::Clothes => {
            // Big lower ball wears the jacket (or buttons) with no twig arms,
            // small upper ball the neck item, so clothes read clearly.
            s.at(21.0, 12.0, 0.0, 1.0, |s| puppet_body(s, 15.0, 7, &look));
            s.at(21.0, 32.5, 0.0, 1.0, |s| draw_ball(s, 10.0, 0.0, 9, Decor::Head, &look));
        }
    });
    Some(pixmap)
}

/// Where the card goes (all Y-up, `[min_x, min_y, max_x, max_y]`): beside the
/// snowman's bounding box on the right when it fits in `screen` (the work area
/// of the monitor holding the snowman's bottom centre), else on the left; the
/// card bottom sits 20 above the snowman bottom, clamped to the screen with an
/// 8 margin. Returns the card's bottom-left corner; the window origin is
/// `ROOM` further down-left.
pub fn placement(snowman: [f64; 4], screen: [f64; 4]) -> (f64, f64) {
    let x = if snowman[2] + 10.0 + CARD_W <= screen[2] - 8.0 {
        snowman[2] + 10.0
    } else {
        (screen[0] + 8.0).max(snowman[0] - 10.0 - CARD_W)
    };
    let y = (snowman[1] + 20.0).min(screen[3] - CARD_H - 8.0).max(screen[1] + 8.0);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hat_and_outfit_toggle_off() {
        let l = apply(Look::classic(), Tab::Hat, 2);
        assert_eq!(l.hat, Some(Hat::Crown));
        assert_eq!(apply(l, Tab::Hat, 2).hat, None);
        assert_eq!(apply(l, Tab::Hat, 3).hat, Some(Hat::Cowboy));
        let l = apply(Look::classic(), Tab::Clothes, 0);
        assert_eq!(l.outfit, None);
        assert_eq!(apply(l, Tab::Clothes, 6).outfit, Some(Outfit::Tuxedo));
        assert_eq!(apply(apply(l, Tab::Clothes, 6), Tab::Clothes, 6).outfit, None);
    }

    #[test]
    fn face_and_arms_only_replace() {
        let l = apply(Look::classic(), Tab::Face, 2);
        assert_eq!(l.face, Face::Wink);
        assert_eq!(apply(l, Tab::Face, 2).face, Face::Wink);
        let l = apply(Look::classic(), Tab::Body, 4);
        assert_eq!(l.arms, Arms::CandyCane);
        assert_eq!(apply(l, Tab::Body, 4).arms, Arms::CandyCane);
    }

    #[test]
    fn out_of_range_changes_nothing() {
        for tab in Tab::ALL {
            assert_eq!(apply(Look::classic(), tab, 8), Look::classic());
            assert!(chip_look(tab, 8).is_none());
            assert!(chip_icon(tab, 99, 1.0).is_none());
        }
    }

    #[test]
    fn selection_and_reset() {
        let l = Look { hat: Some(Hat::Viking), face: Face::Love, arms: Arms::Gift, outfit: None };
        assert_eq!(selected(&l, Tab::Hat), Some(6));
        assert_eq!(selected(&l, Tab::Face), Some(7));
        assert_eq!(selected(&l, Tab::Body), Some(6));
        assert_eq!(selected(&l, Tab::Clothes), None);
        assert_eq!(selected(&Look::classic(), Tab::Hat), None);
        assert_eq!(selected(&Look::classic(), Tab::Clothes), Some(0));
        assert!(can_reset(&l));
        assert!(!can_reset(&Look::classic()));
        assert_eq!(reset(), Look::classic());
        assert_eq!(Tab::ALL[0], Tab::Hat);
    }

    #[test]
    fn chip_looks_vary_only_the_shown_slot() {
        let l = chip_look(Tab::Hat, 1).unwrap();
        assert_eq!((l.hat, l.face, l.arms, l.outfit), (Some(Hat::Bobble), Face::Classic, Arms::Sticks, None));
        assert_eq!(chip_look(Tab::Clothes, 7).unwrap().outfit, Some(Outfit::GoldChain));
        assert_eq!(chip_look(Tab::Face, 3).unwrap().face, Face::Cool);
        assert_eq!(chip_look(Tab::Body, 5).unwrap().arms, Arms::Cocoa);
    }

    #[test]
    fn every_chip_icon_is_drawn_inside_its_box() {
        for tab in Tab::ALL {
            let mut seen: Vec<Vec<u8>> = Vec::new();
            for i in 0..CHIPS {
                let icon = chip_icon(tab, i, 2.0).unwrap();
                assert_eq!((icon.width(), icon.height()), (88, 88));
                assert!(icon.pixels().iter().any(|p| p.alpha() > 0), "{tab:?} {i}");
                assert!(!seen.contains(&icon.data().to_vec()), "{tab:?} {i}");
                seen.push(icon.data().to_vec());
            }
        }
        // Fractional scales round up; an impossible pixmap is refused.
        assert_eq!(chip_icon(Tab::Hat, 0, 1.25).unwrap().width(), 55);
        assert!(chip_icon(Tab::Hat, 0, f32::INFINITY).is_none());
    }

    #[test]
    fn outfit_icons_have_no_twig_arms() {
        // A point on the right twig of the lower ball (72 % along it), clear
        // of both balls: empty on every clothes icon, wood on the arms icon
        // framing of the same ball.
        let (x, y) = (21.0 + 15.0 * 1.306, 12.0 + 15.0 * 0.424);
        let alpha = |icon: &Pixmap| icon.pixel((x * 2.0) as u32, ((ICON - y) * 2.0) as u32).unwrap().alpha();
        for i in 0..CHIPS {
            assert_eq!(alpha(&chip_icon(Tab::Clothes, i, 2.0).unwrap()), 0, "{i}");
        }
        let mut twigs = Pixmap::new(88, 88).unwrap();
        let mut s = Sketch::new(&mut twigs, 2.0);
        s.at(21.0, 12.0, 0.0, 1.0, |s| draw_ball(s, 15.0, 0.0, 7, Decor::Body, &Look::classic()));
        assert!(alpha(&twigs) > 0);
    }

    #[test]
    fn icon_framings_follow_the_slot() {
        // Hat icons: only the top of the head shows, so the bottom-centre
        // pixel is snow and the top rows hold the hat.
        let hat = chip_icon(Tab::Hat, 0, 1.0).unwrap();
        assert!(hat.pixel(22, 43).unwrap().alpha() == 255);
        assert_eq!(hat.pixel(22, 2).unwrap().alpha(), 0);
        assert!(hat.pixel(22, 25).unwrap().alpha() > 0);
        // Face icons: the ball centre (19, 20) Y-up is inside the box.
        let face = chip_icon(Tab::Face, 0, 1.0).unwrap();
        assert!(face.pixel(10, 24).unwrap().alpha() == 255);
    }

    #[test]
    fn placement_prefers_the_right_side() {
        let screen = [0.0, 0.0, 1920.0, 1080.0];
        assert_eq!(placement([100.0, 50.0, 160.0, 200.0], screen), (170.0, 70.0));
        // No room on the right: left of the snowman.
        assert_eq!(placement([1700.0, 50.0, 1760.0, 200.0], screen), (1350.0, 70.0));
        // No room on either side: pinned to the left margin.
        assert_eq!(placement([100.0, 50.0, 160.0, 200.0], [0.0, 0.0, 400.0, 1080.0]).0, 8.0);
        // Vertical clamps.
        assert_eq!(placement([100.0, -500.0, 160.0, 0.0], screen).1, 8.0);
        assert_eq!(placement([100.0, 1070.0, 160.0, 1200.0], screen).1, 1080.0 - 290.0 - 8.0);
    }
}
