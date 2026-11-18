//! Outside-press watch for the dress and tree cards (desktop-native 3.11;
//! snowman 104/304). Those cards never take activation, so the original's
//! detector is used: a new left or right press edge over a window that is not
//! this process closes the card. Presses over our own windows (sprites, the
//! settings window, the card itself) are consumed and the watch keeps going.

use fp_tracker::OutsideClick;

/// Poll cadence while a card is open (15 ms).
pub const EVERY_S: f64 = 0.015;
/// Virtual-key codes of the physical mouse buttons.
pub const VK_LBUTTON: u16 = 1;
pub const VK_RBUTTON: u16 = 2;

#[derive(Debug, Default)]
pub struct OutsideWatch {
    det: Option<OutsideClick>,
    next: f64,
}

impl OutsideWatch {
    /// Forget the detector: the next card that opens arms a fresh one, so the
    /// press that opened it does not count.
    pub fn reset(&mut self) {
        self.det = None;
    }

    /// True while a detector is armed (tests).
    #[cfg(test)]
    pub fn armed(&self) -> bool {
        self.det.is_some()
    }

    /// One driver step. `wanted` is `Cards::outside_watch`. `buttons` reads
    /// "left or right is down"; `own` tells whether the pixel under the cursor
    /// belongs to this process (true when the cursor cannot be read).
    /// Returns true when the card must close now.
    pub fn step(&mut self, now: f64, wanted: bool, buttons: impl Fn() -> bool, own: impl Fn() -> bool) -> bool {
        if !wanted {
            self.det = None;
            return false;
        }
        let Some(det) = self.det.as_mut() else {
            // Arm on the current button state: a held button does not fire.
            self.det = Some(OutsideClick::new(buttons()));
            self.next = now + EVERY_S;
            return false;
        };
        if now < self.next {
            return false;
        }
        self.next = now + EVERY_S;
        // Once fired the detector stays stopped; the card fades, `wanted`
        // turns false and clears it.
        det.poll(buttons(), own())
    }
}

/// Left or right mouse button is down right now.
#[cfg(windows)]
pub fn buttons_down() -> bool {
    fp_win::windows_impl::key_down(VK_LBUTTON) || fp_win::windows_impl::key_down(VK_RBUTTON)
}

/// The pixel belongs to a window of this process; unreadable cursor counts as
/// own, so that edge is swallowed.
#[cfg(windows)]
pub fn over_own(cursor: Option<(i32, i32)>) -> bool {
    cursor.is_none_or(|(x, y)| fp_win::windows_impl::point_in_own_process(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::desktop::{Layout, MonitorInfo};
    use fp_core::effect::Lang;
    use fp_core::geom::{PixelRect, Vec2};
    use fp_ui::{CardCtx, Cards};
    use fp_world::CardRequest;
    use std::cell::Cell;

    #[test]
    fn arms_on_first_wanted_step_and_ignores_held_button() {
        let mut w = OutsideWatch::default();
        assert!(!w.armed());
        // Button already held when the card opens: no fire on later steps.
        assert!(!w.step(0.0, true, || true, || false));
        assert!(w.armed());
        assert!(!w.step(0.02, true, || true, || false));
        // Release then a fresh press outside fires once.
        assert!(!w.step(0.04, true, || false, || false));
        assert!(w.step(0.06, true, || true, || false));
        assert!(!w.step(0.08, true, || false, || false));
        assert!(!w.step(0.10, true, || true, || false));
    }

    #[test]
    fn own_process_press_is_consumed_and_watch_continues() {
        let mut w = OutsideWatch::default();
        w.step(0.0, true, || false, || true);
        assert!(!w.step(0.02, true, || true, || true));
        assert!(!w.step(0.04, true, || false, || true));
        assert!(w.step(0.06, true, || true, || false));
    }

    #[test]
    fn polls_at_15_ms_cadence() {
        let mut w = OutsideWatch::default();
        let reads = Cell::new(0);
        let buttons = || {
            reads.set(reads.get() + 1);
            false
        };
        w.step(0.0, true, buttons, || false); // arm: one read
        w.step(0.005, true, buttons, || false); // too early
        w.step(0.010, true, buttons, || false); // too early
        assert_eq!(reads.get(), 1);
        w.step(0.016, true, buttons, || false);
        assert_eq!(reads.get(), 2);
    }

    #[test]
    fn unwanted_or_reset_rearms_fresh() {
        let mut w = OutsideWatch::default();
        w.step(0.0, true, || false, || false);
        assert!(!w.step(1.0, false, || true, || false));
        assert!(!w.armed());
        // Re-armed while the opening press is still down: it does not count.
        assert!(!w.step(2.0, true, || true, || false));
        w.reset();
        assert!(!w.armed());
        assert!(!w.step(3.0, true, || true, || false));
        assert!(!w.step(3.02, true, || true, || false));
    }

    #[cfg(windows)]
    #[test]
    fn unreadable_cursor_counts_as_own() {
        assert!(over_own(None));
    }

    /// The production chain Core::tick runs: Cards::outside_watch ->
    /// OutsideWatch::step -> Cards::close_outside.
    #[test]
    fn outside_press_closes_open_dress_card_only_on_outside_edge() {
        let layout = Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "A".into(),
            pixels: PixelRect { left: 0, top: 0, right: 1920, bottom: 1080 },
            work: PixelRect { left: 0, top: 0, right: 1920, bottom: 1040 },
            scale: 1.0,
            primary: true,
        }]);
        let cx = CardCtx {
            lang: Lang::En,
            layout: &layout,
            sprites: &[],
            cursor: Vec2::new(500.0, 500.0),
            tree_origin: None,
        };
        let mut cards = Cards::new(1);
        let mut w = OutsideWatch::default();
        let tick = |cards: &mut Cards, w: &mut OutsideWatch, now: f64, down: bool, own: bool| {
            if w.step(now, cards.outside_watch(), || down, || own) {
                cards.close_outside(now);
            }
        };
        tick(&mut cards, &mut w, 0.0, false, false);
        assert!(!w.armed());
        cards.open(CardRequest::DressUp { bottom: 1, look: None }, &cx, 0.1);
        w.reset();
        tick(&mut cards, &mut w, 0.1, true, false); // opening press, held
        tick(&mut cards, &mut w, 0.2, true, false);
        assert!(cards.outside_watch());
        tick(&mut cards, &mut w, 0.3, false, false);
        tick(&mut cards, &mut w, 0.4, true, true); // press on own window
        assert!(cards.outside_watch());
        tick(&mut cards, &mut w, 0.5, false, true);
        tick(&mut cards, &mut w, 0.6, true, false); // press outside
        assert!(!cards.outside_watch());
        tick(&mut cards, &mut w, 0.7, false, false);
        assert!(!w.armed());
    }
}
