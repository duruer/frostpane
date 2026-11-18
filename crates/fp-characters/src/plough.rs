//! Snowplough (characters 2). The blade goes to the snow field through
//! `fp_core::feed::Blade`.

use crate::art::c;
use crate::santa::Block;
use crate::util::{canvas, find_screen, sprite};
use crate::{StageScreen, StepCtx};
use fp_core::feed::Blade;
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::Rng;
use fp_core::scene::{HitMode, Owner, Sprite, ZBand};
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, Trace, pose, rgba};

const SPEED: f64 = 120.0;
const LEAVE: f64 = 2.5;
const W: f64 = 112.0;
const H: f64 = 66.0;
const TIP: f64 = 110.0;
const WHEEL_R: f64 = 7.0;

/// Snow depth gate for automatic runs: with and without a taskbar ledge.
const DEPTH_DOCK: f64 = 4.0;
const DEPTH_GROUND: f64 = 20.0;

#[derive(Clone, Debug)]
struct Run {
    screen: Frame,
    way: f64,
    bottom: f64,
    on_dock: bool,
    /// Ordered under the taskbar (setting `behindWindows`, 2.1).
    behind: bool,
    front: f64,
    start: f64,
    end: f64,
    sent: bool,
    shown: bool,
    alpha: f64,
    turned: f64,
}

/// The snowplough.
#[derive(Clone, Debug)]
pub struct Plough {
    next_at: f64,
    pending: bool,
    behind: bool,
    run: Option<Run>,
    now: f64,
}

impl Default for Plough {
    fn default() -> Self {
        Self { next_at: f64::INFINITY, pending: false, behind: false, run: None, now: 0.0 }
    }
}

impl Plough {
    pub(crate) fn start(&mut self, now: f64, rng: &mut impl Rng) {
        self.next_at = now + rng.range(300.0, 600.0);
    }

    pub(crate) fn stop(&mut self) {
        self.next_at = f64::INFINITY;
        self.pending = false;
        self.run = None;
    }

    /// Queue a manual run on the first shown screen.
    pub fn send(&mut self) {
        self.pending = true;
    }

    pub fn active(&self) -> bool {
        self.run.is_some()
    }

    /// Setting `behindWindows`: runs started from now on are ordered just
    /// below the taskbar instead of topmost (2.1 window order).
    pub fn set_behind(&mut self, behind: bool) {
        self.behind = behind;
    }

    pub(crate) fn step(&mut self, c: &StepCtx<'_>, comes: bool, depths: &[(Frame, f64)], rng: &mut impl Rng) {
        self.now = c.now;
        if self.run.is_none() {
            let manual = std::mem::take(&mut self.pending);
            if manual {
                if let Some(s) = c.plough_screens.iter().find(|s| s.shown) {
                    self.run = Some(begin(s, true, self.behind, rng));
                }
            } else if comes && c.now >= self.next_at {
                let deep = |s: &&StageScreen| {
                    let depth = depths.iter().find(|(f, _)| *f == s.frame).map_or(0.0, |d| d.1);
                    depth >= if s.dock.is_some() { DEPTH_DOCK } else { DEPTH_GROUND }
                };
                if let Some(s) = c.plough_screens.iter().filter(|s| s.shown).find(deep) {
                    self.run = Some(begin(s, false, self.behind, rng));
                }
            }
        }
        let Some(run) = self.run.as_mut() else { return };
        let moved = SPEED * c.dt.max(0.0);
        run.front += run.way * moved;
        run.turned -= moved / WHEEL_R;
        let screen = find_screen(c.plough_screens, run.screen).filter(|_| (run.front - run.end) * run.way <= 0.0);
        let Some(screen) = screen else {
            if !run.sent {
                self.next_at = c.now + rng.range(600.0, 1200.0);
            }
            self.run = None;
            return;
        };
        run.shown = screen.shown;
        run.alpha = if screen.shown { 1.0 } else { 0.0 };
        if run.on_dock {
            let edge = ((run.front - run.start) * run.way - W - 4.0).min((run.end - run.front) * run.way);
            run.alpha *= (edge / 30.0).clamp(0.0, 1.0);
        }
    }

    /// Screen with a plough on it: the nose x (fight 4.7 panic).
    pub fn front_on(&self, screen: Frame) -> Option<f64> {
        self.run.as_ref().filter(|r| r.screen == screen).map(|r| r.front)
    }

    /// The blade for the snow field.
    pub fn blade(&self) -> Option<(Vec2, Blade)> {
        let r = self.run.as_ref()?;
        let blade =
            Blade { front: r.front - r.screen.min_x(), way: r.way, speed: SPEED, leave: LEAVE, on_dock: r.on_dock };
        Some((r.screen.min(), blade))
    }

    /// Body rectangle for Santa's hop.
    pub(crate) fn block(&self) -> Option<Block> {
        let r = self.run.as_ref()?;
        let back = r.front - r.way * W;
        Some((r.screen, r.front.min(back), r.front.max(back), r.bottom, r.bottom + H))
    }

    pub(crate) fn sprites(&self, rev: u64, out: &mut Vec<Sprite>) {
        if let Some(r) = &self.run {
            let x = if r.way > 0.0 { r.front - TIP } else { r.front - (W - TIP) };
            let frame = Frame::new(x, r.bottom, W, H);
            let z = if r.behind { ZBand::UnderDock } else { ZBand::Front };
            out.push(sprite(Owner::Character, 2, frame, (z, HitMode::Never), rev, r.alpha));
        }
    }

    pub(crate) fn paint(&self, scale: f32) -> Option<Pixmap> {
        let r = self.run.as_ref()?;
        let mut pm = canvas(W, H, scale)?;
        let mut s = Sketch::new(&mut pm, scale);
        s.concat(pose((56.0, 33.0), (r.way as f32, 1.0), 0.0, (56.0, 33.0)));
        let mut blade = Trace::new();
        blade.move_to(94.0, 0.0).line(110.0, 0.0).quad(107.0, 38.0, 99.0, 16.0).line(97.0, 36.0).close();
        let blade = blade.finish().expect("blade path");
        s.rect(4.0, 1.0, 80.0, 18.0, 9.0, c(0x2B303B));
        s.rect(8.0, 18.0, 74.0, 15.0, 4.0, c(0xDB9414));
        s.rect(20.0, 32.0, 42.0, 26.0, 6.0, c(0xFCC433));
        let glass = crate::art::poly(&[(26.0, 37.0), (56.0, 37.0), (56.0, 53.0), (26.0, 53.0)]);
        s.across(&glass, &[c(0x78ADE6), c(0xCCEDFF)], None, (0.0, 53.0), (0.0, 37.0));
        s.polygon(&[(26.0, 40.0), (36.0, 53.0), (30.0, 53.0), (26.0, 48.0)], rgba(1.0, 1.0, 1.0, 0.45));
        s.rect(18.0, 56.0, 46.0, 4.0, 1.5, c(0x40454F));
        s.rect(66.0, 32.0, 4.0, 14.0, 1.0, c(0x40454F));
        s.rect(80.0, 11.0, 16.0, 6.0, 1.5, c(0x4D5461));
        s.across(&blade, &[c(0xDBE6F2), c(0x8C9EB8)], None, (94.0, 0.0), (110.0, 0.0));
        s.stroke(&blade, c(0x4D596B), 1.2);
        s.rect(95.0, 0.0, 15.0, 2.5, 1.0, c(0xE64D33));
        for x in [15.0f32, 33.0, 51.0, 69.0] {
            s.at(x, 10.0, r.turned as f32, 1.0, |s| {
                s.circle(0.0, 0.0, 6.5, c(0xBDC4D1));
                for k in 0..3 {
                    let a = k as f32 * std::f32::consts::TAU / 3.0;
                    s.line(0.0, 0.0, 5.5 * a.cos(), 5.5 * a.sin(), c(0x4D5461), 1.4);
                }
                s.circle(0.0, 0.0, 2.0, c(0x40454F));
            });
        }
        let on = (self.now / 0.35).floor() as i64 % 2 == 0;
        s.rect(37.0, 59.0, 8.0, 6.0, 2.0, rgba(1.0, 0.55, 0.1, if on { 1.0 } else { 0.35 }));
        Some(pm)
    }
}

fn begin(screen: &StageScreen, manual: bool, behind: bool, rng: &mut impl Rng) -> Run {
    let frame = screen.frame;
    let way = if rng.coin() { 1.0 } else { -1.0 };
    let (bottom, span, on_dock) = match screen.dock {
        Some((x0, x1, top)) => (top - 1.0, (x0, x1), true),
        None => (frame.min_y() - 1.0, (frame.min_x(), frame.max_x()), false),
    };
    let start = if way > 0.0 { span.0 - 4.0 } else { span.1 + 4.0 };
    let e = if on_dock { 4.0 } else { W + 4.0 };
    let end = if way > 0.0 { span.1 + e } else { span.0 - e };
    let front = if on_dock { start + way * (W + 4.0) } else { start };
    Run {
        screen: frame,
        way,
        bottom,
        on_dock,
        behind,
        front,
        start,
        end,
        sent: manual,
        shown: screen.shown,
        alpha: 1.0,
        turned: 0.0,
    }
}
