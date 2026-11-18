//! The gift box Santa drops (social 2.11). Letter and item looks are dropped.

use crate::art::{c, disc};
use crate::util::{canvas, sprite};
use crate::{StageScreen, StepCtx};
use fp_core::geom::{Frame, Vec2};
use fp_core::scene::{HitMode, Owner, Sprite, SpriteKey, ZBand};
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, rgba};

const SIZE: f64 = 68.0;
const GRAVITY: f64 = 1400.0;
const BOUNCE_MIN: f64 = 120.0;
const BOUNCE: f64 = 0.3;
const DRAG_START: f64 = 4.0;
/// Sprite id of the gift (owner `Gift`).
pub(crate) const GIFT_ID: u64 = 1;

/// A gift lying on the ground.
#[derive(Clone, Debug)]
pub struct GiftBox {
    x: f64,
    bottom: f64,
    fall: f64,
    held: bool,
    /// Press point and grab offset.
    press: Option<(Vec2, Vec2)>,
    dragging: bool,
    was_down: bool,
    alpha: f64,
    now: f64,
}

/// Floor for a gift standing at `x`.
fn floor(screen: &StageScreen, x: f64) -> f64 {
    match screen.dock {
        Some((a, b, top)) if x >= a && x <= b => top - 2.0,
        _ => screen.frame.min_y() + 2.0,
    }
}

impl GiftBox {
    /// A gift falling from `at` (bottom centre).
    pub fn new(at: Vec2) -> Self {
        Self {
            x: at.x,
            bottom: at.y,
            fall: 0.0,
            held: false,
            press: None,
            dragging: false,
            was_down: false,
            alpha: 1.0,
            now: 0.0,
        }
    }

    pub fn position(&self) -> Vec2 {
        Vec2::new(self.x, self.bottom)
    }

    /// Returns the open point when the gift was clicked open.
    pub(crate) fn step(&mut self, c: &StepCtx<'_>) -> Option<Vec2> {
        self.now = c.now;
        let list = c.visit_screens;
        let Some(screen) =
            list.iter().find(|s| s.frame.min_x() <= self.x && self.x <= s.frame.max_x()).or(list.first())
        else {
            self.alpha = 0.0;
            return None;
        };
        if !self.held && (self.x < screen.frame.min_x() || self.x > screen.frame.max_x()) {
            self.x = screen.frame.mid_x();
        }
        self.alpha = if screen.shown { 1.0 } else { 0.0 };
        let mut opened = None;
        let p = c.pointer;
        let down = p.left_down;
        if let (true, false, Some(pos)) = (down, self.was_down, p.pos)
            && p.hot == Some(SpriteKey::new(Owner::Gift, GIFT_ID))
        {
            self.press = Some((pos, Vec2::new(self.x - pos.x, self.bottom - pos.y)));
        }
        if down {
            if let (Some((start, grab)), Some(pos)) = (self.press, p.pos) {
                if !self.dragging && pos.dist(start) > DRAG_START {
                    self.dragging = true;
                    self.held = true;
                }
                if self.dragging {
                    self.x = pos.x + grab.x;
                    self.bottom = pos.y + grab.y;
                }
            }
        } else if self.press.take().is_some() {
            if self.dragging {
                self.dragging = false;
                self.held = false;
                self.fall = 0.0;
            } else {
                opened = Some(Vec2::new(self.x, self.bottom + 24.0));
            }
        }
        self.was_down = down;
        if !self.held {
            let f = floor(screen, self.x);
            self.fall -= GRAVITY * c.dt.max(0.0);
            self.bottom += self.fall * c.dt.max(0.0);
            if self.bottom <= f {
                self.bottom = f;
                self.fall = if -self.fall > BOUNCE_MIN { -self.fall * BOUNCE } else { 0.0 };
            }
        }
        opened
    }

    pub(crate) fn sprite(&self, rev: u64) -> Sprite {
        let frame = Frame::new(self.x - SIZE / 2.0, self.bottom, SIZE, SIZE);
        sprite(Owner::Gift, GIFT_ID, frame, (ZBand::Front, HitMode::Always), rev, self.alpha)
    }

    pub(crate) fn paint(&self, scale: f32) -> Option<Pixmap> {
        let mut pm = canvas(SIZE, SIZE, scale)?;
        let mut s = Sketch::new(&mut pm, scale);
        let phase = self.now.rem_euclid(4.0);
        let wiggle = if phase < 0.6 && !self.held { (phase * 30.0).sin() * 0.12 * (1.0 - phase / 0.6) } else { 0.0 };
        let glow = (0.55 + 0.35 * (self.now * 2.5).sin()) * 0.75;
        let halo = rgba(1.0, 0.86, 0.45, glow);
        let clear = rgba(1.0, 0.86, 0.45, 0.0);
        s.round(
            &disc(34.0, 26.0, 34.0),
            &[clear, clear, halo, clear],
            Some(&[0.0, 0.18, 0.181, 1.0]),
            (34.0, 26.0),
            (34.0, 26.0),
            34.0,
        );
        s.at(34.0, 0.0, wiggle as f32, 1.0, |s| {
            s.translate(-34.0, 0.0);
            s.rect(15.0, 2.0, 38.0, 30.0, 2.0, c(0xD6212B));
            s.rect(46.0, 2.0, 7.0, 30.0, 0.0, c(0xA31421));
            s.rect(12.0, 30.0, 44.0, 11.0, 2.0, c(0xD6212B));
            s.rect(12.0, 30.0, 44.0, 3.0, 0.0, c(0xA31421));
            s.rect(29.0, 2.0, 10.0, 39.0, 0.0, c(0xFAC947));
            s.ring(20.0, 41.0, 14.0, 12.0, c(0xFAC947), 3.0);
            s.ring(34.0, 41.0, 14.0, 12.0, c(0xFAC947), 3.0);
            s.circle(34.0, 42.0, 4.0, c(0xD19929));
        });
        Some(pm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::clock::WallTime;
    use fp_core::scene::PointerInput;

    fn screen(dock: Option<(f64, f64, f64)>, shown: bool) -> StageScreen {
        StageScreen { frame: Frame::new(0.0, 0.0, 1920.0, 1080.0), shown, dock }
    }

    fn step(g: &mut GiftBox, screens: &[StageScreen], ptr: &PointerInput, now: f64) -> Option<Vec2> {
        let c = StepCtx {
            now,
            dt: 1.0 / 60.0,
            wall: WallTime::default(),
            visit_screens: screens,
            plough_screens: screens,
            shown_screens: screens,
            pointer: ptr,
        };
        g.step(&c)
    }

    #[test]
    fn falls_bounces_and_rests_on_the_right_floor() {
        let none = PointerInput::default();
        let screens = [screen(Some((0.0, 100.0, 40.0)), true)];
        let mut g = GiftBox::new(Vec2::new(500.0, 400.0));
        let mut bounced = false;
        let mut last = 400.0;
        for i in 0..600 {
            step(&mut g, &screens, &none, f64::from(i) / 60.0);
            bounced |= g.position().y > last && g.position().y < 100.0;
            last = g.position().y;
        }
        assert!(bounced);
        assert_eq!(g.position().y, 2.0, "outside the ledge span the gift rests on the ground");
        let mut on_dock = GiftBox::new(Vec2::new(50.0, 400.0));
        for i in 0..600 {
            step(&mut on_dock, &screens, &none, f64::from(i) / 60.0);
        }
        assert_eq!(on_dock.position().y, 38.0);
        // idle wiggle at the start of the 4 s cycle, none while held
        assert!(on_dock.paint(1.0).is_some());
        let mut g2 = on_dock.clone();
        g2.now = 0.2;
        assert!(g2.paint(2.0).is_some());
        g2.held = true;
        assert!(g2.paint(1.0).is_some());
        assert_eq!(on_dock.sprite(3).opacity, 1.0);
    }

    #[test]
    fn no_screen_hides_and_stray_gift_snaps_back() {
        let none = PointerInput::default();
        let mut g = GiftBox::new(Vec2::new(5000.0, 100.0));
        assert!(step(&mut g, &[], &none, 0.0).is_none());
        assert_eq!(g.sprite(1).opacity, 0.0);
        let screens = [screen(None, false)];
        step(&mut g, &screens, &none, 0.1);
        assert_eq!(g.position().x, 960.0);
        assert_eq!(g.sprite(1).opacity, 0.0);
    }

    #[test]
    fn drag_moves_the_box_and_click_opens_it() {
        let screens = [screen(None, true)];
        let key = SpriteKey::new(Owner::Gift, GIFT_ID);
        let mut g = GiftBox::new(Vec2::new(500.0, 2.0));
        step(&mut g, &screens, &PointerInput::default(), 0.0);
        let down = |x: f64, y: f64| PointerInput {
            pos: Some(Vec2::new(x, y)),
            left_down: true,
            right_down: false,
            hot: Some(key),
        };
        step(&mut g, &screens, &down(510.0, 20.0), 0.1);
        step(&mut g, &screens, &down(512.0, 21.0), 0.2);
        assert_eq!(g.position(), Vec2::new(500.0, 2.0), "inside the drag threshold");
        step(&mut g, &screens, &PointerInput { pos: None, left_down: true, right_down: false, hot: None }, 0.25);
        step(&mut g, &screens, &down(700.0, 300.0), 0.3);
        assert_eq!(g.position(), Vec2::new(690.0, 282.0));
        assert!(g.held);
        step(&mut g, &screens, &down(710.0, 310.0), 0.4);
        assert_eq!(g.position(), Vec2::new(700.0, 292.0));
        let released =
            PointerInput { pos: Some(Vec2::new(710.0, 310.0)), left_down: false, right_down: false, hot: None };
        assert!(step(&mut g, &screens, &released, 0.5).is_none(), "a drag is not a click");
        assert!(!g.held);
        for i in 0..200 {
            step(&mut g, &screens, &PointerInput::default(), 1.0 + f64::from(i) / 60.0);
        }
        // a press that does not start on the gift does nothing
        let stray = PointerInput { pos: Some(Vec2::new(0.0, 0.0)), left_down: true, right_down: false, hot: None };
        step(&mut g, &screens, &stray, 9.0);
        assert!(step(&mut g, &screens, &PointerInput::default(), 9.1).is_none());
        // press and release in place opens it just above the box
        let p = g.position();
        let press = PointerInput { pos: Some(p), left_down: true, right_down: false, hot: Some(key) };
        step(&mut g, &screens, &press, 9.2);
        let at = step(&mut g, &screens, &PointerInput::default(), 9.3).expect("opened");
        assert_eq!(at, Vec2::new(p.x, p.y + 24.0));
    }
}
