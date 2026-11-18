//! Santa's sleigh (characters 1).

use crate::art::{self, DEER_AT};
use crate::util::{canvas, ease, find_screen, sprite};
use crate::{StageScreen, StepCtx};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::Rng;
use fp_core::scene::{HitMode, Owner, Sprite, ZBand};
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, pose};

/// Cruise speed in points per second.
const SPEED: f64 = 130.0;
/// Sleigh design length and window height.
const LEN: f64 = 266.0;
const WIN_H: f64 = 158.0;
/// Room below the runners inside the window.
const ROOM: f64 = 36.0;
/// Steepest swoop slope (radians) and highest rise (screen height share).
const STEEPEST: f64 = 0.55;
const HIGHEST: f64 = 0.45;
const TOUCHDOWN: f64 = 0.3;
const DOCK_SCALE: f64 = 0.8;
const DOCK_CHANCE: f64 = 0.5;
const HOP_REACH: f64 = 120.0;
const HOP_CLEARANCE: f64 = 8.0;
const HOP_PACE: f64 = 8.0;
const SLEIGH_H: f64 = 86.0;

/// A rectangle obstacle `(screen, x0, x1, bottom, top)`; only obstacles on
/// the sleigh's own screen count (characters 1.3).
pub type Block = (Frame, f64, f64, f64, f64);

#[derive(Clone, Debug)]
struct Run {
    screen: Frame,
    way: f64,
    scale: f64,
    floor: f64,
    center: f64,
    touch: (f64, f64),
    reach: (f64, f64),
    rise: (f64, f64),
    drop_at: f64,
    sent: bool,
    gives: bool,
    dropped: bool,
    dropped_at: f64,
    lift: f64,
    bottom: f64,
    tilt: f64,
    arm: f64,
    shown: bool,
}

impl Run {
    /// Sleigh runner height above the floor profile at `x` (no hop lift).
    fn profile(&self, x: f64) -> f64 {
        let swoop = |d: f64, reach: f64, rise: f64| {
            let d = d.min(reach);
            rise * (1.0 - (std::f64::consts::PI * d / reach).cos()) / 2.0
        };
        let h = if x < self.touch.0 {
            swoop(self.touch.0 - x, self.reach.0, self.rise.0)
        } else if x > self.touch.1 {
            swoop(x - self.touch.1, self.reach.1, self.rise.1)
        } else {
            0.0
        };
        self.floor + h
    }

    fn half(&self) -> f64 {
        LEN * self.scale / 2.0
    }
}

/// What one step reports.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SantaOut {
    /// An automatic run just began (persist `lastSantaVisit`).
    pub came: bool,
    /// An automatic run just ended (schedule the next visit).
    pub ended_auto: bool,
    /// Where the gift sack opened.
    pub drop: Option<Vec2>,
}

/// Santa's sleigh. One run at a time.
#[derive(Clone, Debug, Default)]
pub struct Santa {
    pending: Option<bool>,
    run: Option<Run>,
    now: f64,
}

impl Santa {
    /// Queue a manual run (`on_dock` = ride the taskbar ledge when there is one).
    pub fn send(&mut self, on_dock: bool) {
        self.pending = Some(on_dock);
    }

    pub fn active(&self) -> bool {
        self.run.is_some()
    }

    pub(crate) fn stop(&mut self) {
        self.run = None;
        self.pending = None;
    }

    /// Plough or fight obstacles are passed as rectangles.
    pub(crate) fn step(
        &mut self,
        c: &StepCtx<'_>,
        due: bool,
        (fighters, plough): (&[Block], Option<Block>),
        dropping: bool,
        rng: &mut impl Rng,
    ) -> SantaOut {
        self.now = c.now;
        let mut out = SantaOut::default();
        if self.run.is_none() {
            if self.pending.is_none() && !due {
                return out;
            }
            out.came = self.begin(c, rng);
        }
        let Some(run) = self.run.as_mut() else { return out };
        let dt = c.dt.max(0.0);
        run.center += run.way * SPEED * dt;
        let frame = run.screen;
        let end = if run.way > 0.0 {
            frame.max_x() + LEN / 2.0 * run.scale + 10.0
        } else {
            frame.min_x() - LEN / 2.0 * run.scale - 10.0
        };
        let Some(screen) = find_screen(c.visit_screens, frame).filter(|_| (run.center - end) * run.way <= 0.0) else {
            out.ended_auto = !run.sent;
            self.run = None;
            return out;
        };
        run.shown = screen.shown;
        let path = run.profile(run.center);
        let half = run.half();
        let mut target = 0.0f64;
        for (_, x0, x1, bottom, top) in fighters.iter().copied().chain(plough).filter(|b| b.0 == frame) {
            let gap = (x0 - (run.center + half)).max((run.center - half) - x1).max(0.0);
            if path < top && path + SLEIGH_H * run.scale > bottom && gap < HOP_REACH {
                target = target.max(top + HOP_CLEARANCE - path);
            }
        }
        let before = run.lift;
        run.lift += (target - run.lift) * (1.0 - (-HOP_PACE * dt).exp());
        let slope = (run.profile(run.center + 1.0) - run.profile(run.center - 1.0)) / 2.0;
        let climb = if dt > 0.0 { (run.lift - before) / dt / (SPEED * run.way) } else { 0.0 };
        run.tilt = (slope + climb).atan().clamp(-STEEPEST, STEEPEST);
        run.bottom = path + run.lift;
        if run.shown && dropping && run.gives && !run.dropped && (run.center - run.drop_at) * run.way >= 0.0 {
            run.dropped = true;
            run.dropped_at = c.now;
            out.drop = Some(Vec2::new(run.center - run.way * 20.0 * run.scale, run.bottom + 30.0 * run.scale));
        }
        let since = if run.dropped { c.now - run.dropped_at } else { 1e9 };
        let until =
            if dropping && run.gives && !run.dropped { (run.drop_at - run.center) * run.way / SPEED } else { -1.0 };
        run.arm = if since < 0.12 {
            ease(0.5, -1.1, (since + 0.12) / 0.24)
        } else if (0.0..0.12).contains(&until) {
            ease(0.5, -1.1, (0.12 - until) / 0.24)
        } else if (0.0..0.45).contains(&until) {
            ease(-1.72, 0.5, (0.45 - until) / 0.33)
        } else {
            ease(-1.1, -1.72, (since - 0.12) / 0.5)
        };
        out
    }

    /// Start a run when allowed. Returns whether it was an automatic one.
    fn begin(&mut self, c: &StepCtx<'_>, rng: &mut impl Rng) -> bool {
        let sent = self.pending.take();
        let list = c.visit_screens;
        let Some(screen) = list.iter().find(|s| s.shown && s.dock.is_some()).or_else(|| list.iter().find(|s| s.shown))
        else {
            return false;
        };
        let on_dock = match sent {
            Some(v) => v,
            None => rng.unit() < DOCK_CHANCE,
        } && screen.dock.is_some();
        self.run = Some(start_run(screen, on_dock, sent, rng, c.now));
        sent.is_none()
    }

    /// Sprite of the sleigh window.
    pub(crate) fn sprites(&self, rev: u64, out: &mut Vec<Sprite>) {
        if let Some(r) = &self.run {
            let alpha = if r.shown { 1.0 } else { 0.0 };
            out.push(sprite(Owner::Character, 1, window(r), (ZBand::Front, HitMode::Never), rev, alpha));
        }
    }

    pub(crate) fn paint(&self, scale: f32) -> Option<Pixmap> {
        let r = self.run.as_ref()?;
        let k = r.scale as f32;
        let mut pm = canvas(LEN * r.scale, WIN_H * r.scale, scale)?;
        let mut s = Sketch::new(&mut pm, scale * k);
        let layer = pose((133.0, 43.0), (r.way as f32, 1.0), r.tilt as f32, (133.0, (ROOM + 43.0) as f32));
        s.concat(layer);
        let t = self.now;
        for (i, x) in DEER_AT.iter().enumerate() {
            let mut legs = [0.0f32; 4];
            for (j, a) in legs.iter_mut().enumerate() {
                let n = (i * 4 + j) as f64;
                *a = (0.6 * (t * 13.0 + (n % 4.0) * 1.4 + (n / 4.0).floor() * 0.9).sin()) as f32;
            }
            let bob = (2.0 + 3.0 * (t * 6.5 + i as f64 * 0.9).sin().abs()) as f32;
            s.at(*x, 4.0, 0.0, 1.0, |s| art::deer(s, legs, bob, 0.0, i == 2));
        }
        art::sleigh(&mut s, true, r.arm as f32);
        Some(pm)
    }
}

fn window(r: &Run) -> Frame {
    Frame::new(r.center - LEN / 2.0 * r.scale, r.bottom - ROOM * r.scale, LEN * r.scale, WIN_H * r.scale)
}

fn start_run(screen: &StageScreen, on_dock: bool, sent: Option<bool>, rng: &mut impl Rng, now: f64) -> Run {
    let frame = screen.frame;
    let way = if rng.coin() { 1.0 } else { -1.0 };
    let scale = if on_dock { DOCK_SCALE } else { 1.0 };
    let half = LEN * scale / 2.0;
    let (floor, span) = match screen.dock {
        Some((x0, x1, top)) if on_dock => (top - 1.0, (x0, x1)),
        _ => (frame.min_y(), (frame.min_x(), frame.max_x())),
    };
    let w = span.1 - span.0;
    let touch = (span.0 + TOUCHDOWN * w, span.1 - TOUCHDOWN * w);
    let reach = ((touch.0 - (frame.min_x() - half)).max(1.0), (frame.max_x() + half - touch.1).max(1.0));
    let ceiling = (frame.min_y() + HIGHEST * frame.h - floor).max(0.0);
    let cap = |reach: f64| ceiling.min(2.0 * reach * STEEPEST.tan() / std::f64::consts::PI);
    let center = if way > 0.0 { frame.min_x() - half } else { frame.max_x() + half };
    Run {
        screen: frame,
        way,
        scale,
        floor,
        center,
        touch,
        reach,
        rise: (cap(reach.0), cap(reach.1)),
        drop_at: span.0 + w * rng.range(0.35, 0.65),
        sent: sent.is_some(),
        gives: sent != Some(true),
        dropped: false,
        dropped_at: now,
        lift: 0.0,
        bottom: floor,
        tilt: 0.0,
        arm: -1.72,
        shown: screen.shown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::rng::Lcg64;

    #[test]
    fn swoop_profile_matches_the_flight_rules() {
        let screen = StageScreen { frame: Frame::new(0.0, 0.0, 1920.0, 1080.0), shown: true, dock: None };
        let run = start_run(&screen, false, None, &mut Lcg64::new(1), 0.0);
        let half = 133.0;
        assert_eq!(run.touch, (576.0, 1344.0));
        assert_eq!(run.reach.0, 576.0 + half);
        // flat on the touch segment, full rise at and beyond the reach
        assert_eq!(run.profile(960.0), 0.0);
        let cap = (2.0 * run.reach.0 * STEEPEST.tan() / std::f64::consts::PI).min(0.45 * 1080.0);
        assert!((run.rise.0 - cap).abs() < 1e-9);
        assert!((run.profile(-half) - run.rise.0).abs() < 1e-9);
        assert!((run.profile(-5000.0) - run.rise.0).abs() < 1e-9);
        // steepest slope stays within tan(0.55)
        let mut steepest = 0.0f64;
        let mut x = -half;
        while x < 1920.0 + half {
            steepest = steepest.max((run.profile(x + 0.5) - run.profile(x - 0.5)).abs());
            x += 1.0;
        }
        assert!(steepest <= STEEPEST.tan() + 1e-6, "{steepest}");
        assert!(run.drop_at >= 0.35 * 1920.0 && run.drop_at <= 0.65 * 1920.0);
        assert!(run.gives && !run.sent);
        // a manual dock send: dock floor, dock scale, no gift
        let docked = StageScreen { dock: Some((100.0, 1800.0, 40.0)), ..screen };
        let run = start_run(&docked, true, Some(true), &mut Lcg64::new(1), 0.0);
        assert_eq!((run.floor, run.scale), (39.0, DOCK_SCALE));
        assert_eq!(run.touch, (100.0 + 0.3 * 1700.0, 1800.0 - 0.3 * 1700.0));
        assert!(!run.gives && run.sent);
    }
}
