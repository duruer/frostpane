//! Snowman fights.
//!
//! Spec: snowman-fight 1 (overview), 2.1 (auto schedule via DailyVisits,
//! pref `lastSnowmanFight`: the scheduler lives in `fp-characters`), 2.2
//! (manual, variants Quick/Long/Both/Backstab or random), 2.3-2.4 (step,
//! begin), 3 (story generator: moves, building blocks, variant scripts), 4
//! (runtime: ball flight, puffs, drop, crumble, flag, hat, plough panic), 5
//! (rendering, restyled), 6 (constants), 7 (edge cases). The puppet art is in
//! `fp-snowman`.
//!
//! The caller (`fp-world`) owns the scheduling, the screen pick and the
//! plough veto; this crate owns one bout at a time:
//!
//! * [`Fight::begin`] takes the fighting range as `stage` (its `x` and `w`
//!   are the range in global points) and the floor line. When `floor` is
//!   above `stage.y` (the screen bottom) the fight stands on a dock whose top
//!   is `floor + 1`, and the fighters are `clamp((floor + 1 - stage.y) *
//!   0.62, 26, 44)` tall; otherwise they are 44 tall. Use
//!   [`Fight::plough_veto`] before starting.
//! * [`Fight::step`] takes the plough front on this screen (panic, 4.7).
//! * [`Fight::sprites`] gives one panel sprite; [`Fight::paint`] draws it.
//! * [`FightEvent::Burst`] positions are global points.

mod art;
mod bout;
mod story;

pub use story::{Aim, Emote, Ending, Face, Gait, Move, React, Story, StoryParams, Style, StyleInfo, Variant, story};

use bout::Bout;
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng};
use fp_core::scene::{HitMode, Owner, Sprite, SpriteKey, ZBand};

/// Plough sight distance: a fight does not start within this far of a plough
/// run (2.4) and panics when one comes this close (4.7).
pub const PLOUGH_SIGHT: f64 = bout::PLOUGH_SIGHT;

#[derive(Clone, Debug, PartialEq)]
pub enum FightEvent {
    /// A death disturbs the snowfall here (global points, `area` = 380).
    Burst { at: Vec2, area: f64 },
    /// The bout is over (after the fade); the panel is gone.
    Finished,
}

/// The fight controller: at most one bout at a time.
#[derive(Default)]
pub struct Fight {
    bout: Option<Bout>,
    prev_variant: Option<Variant>,
    last_ending: Option<Ending>,
    hidden: bool,
    rev: u64,
}

fn other_variant(prev: Option<Variant>, rng: &mut impl Rng) -> Variant {
    let options: Vec<Variant> = Variant::ALL.iter().copied().filter(|v| Some(*v) != prev).collect();
    options[rng.below(options.len() as u64) as usize]
}

impl Fight {
    /// True when a plough run whose front is at `front` is too close to the
    /// range of `stage` (2.4: within 420 of either end).
    pub fn plough_veto(stage: Frame, front: f64) -> bool {
        front > stage.min_x() - PLOUGH_SIGHT && front < stage.max_x() + PLOUGH_SIGHT
    }

    /// Starts a bout (2.4). `variant` `None` picks a random one that differs
    /// from the previous bout. Returns false when a bout is already running
    /// or the stage is unusable. The same `seed` gives the same fight.
    pub fn begin(&mut self, variant: Option<Variant>, stage: Frame, floor: f64, seed: u64) -> bool {
        let usable =
            stage.w.is_finite() && stage.w > 0.0 && stage.x.is_finite() && stage.y.is_finite() && floor.is_finite();
        if self.bout.is_some() || !usable {
            return false;
        }
        let mut rng = Lcg64::new(seed);
        let variant = match variant {
            Some(v) => v,
            None => other_variant(self.prev_variant, &mut rng),
        };
        self.prev_variant = Some(variant);
        let height = if floor > stage.y { ((floor + 1.0 - stage.y) * 0.62).clamp(26.0, 44.0) } else { 44.0 };
        let scale = height / bout::RIG_HEIGHT;
        let (min, max) = (stage.min_x(), stage.max_x());
        let params = StoryParams { min, max, scale, avoid: self.last_ending };
        let plan = story(variant, &params, &mut rng);
        if plan.ending.is_some() {
            self.last_ending = plan.ending;
        }
        self.bout = Some(Bout::new(plan, rng, min, max, scale, floor));
        self.rev += 1;
        true
    }

    /// One frame (2.3). `plough_x` = current plough front on this screen
    /// (4.7 panic). Pushes bursts, and [`FightEvent::Finished`] once.
    pub fn step(&mut self, dt: f64, plough_x: Option<f64>, out: &mut Vec<FightEvent>) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.1) } else { 0.0 };
        let Some(bout) = &mut self.bout else {
            return;
        };
        bout.step(dt, plough_x, out);
        self.rev += 1;
        if bout.finished() {
            self.bout = None;
            out.push(FightEvent::Finished);
        }
    }

    pub fn active(&self) -> bool {
        self.bout.is_some()
    }

    /// Ends the bout at once (screen vanished, effect off); no event.
    pub fn stop(&mut self) {
        self.bout = None;
    }

    /// The panel is invisible (opacity 0) while the screen is not shown (7).
    pub fn set_shown(&mut self, shown: bool) {
        self.hidden = !shown;
        self.rev += 1;
    }

    /// Floor line of the running bout (global points).
    pub fn floor(&self) -> Option<f64> {
        self.bout.as_ref().map(|b| b.floor)
    }

    /// Fighters as hop obstacles for Santa (characters 8): `(x, half width,
    /// height)` above the floor, for each visible fighter.
    pub fn obstacles(&self) -> Vec<(f64, f64, f64)> {
        match &self.bout {
            Some(b) => b.obstacles(),
            None => Vec::new(),
        }
    }

    /// The panel sprite (`range - 60 .. range + 60`, 180 high), click-through,
    /// repainted every step. Nothing while no bout runs.
    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        if let Some(b) = &self.bout {
            out.push(Sprite {
                key: SpriteKey::new(Owner::Fight, 0),
                frame: b.panel,
                z: ZBand::Front,
                hit: HitMode::Never,
                rev: self.rev,
                opacity: if self.hidden { 0.0 } else { 1.0 },
            });
        }
    }

    /// Paints the panel into a premultiplied pixmap of
    /// `ceil(w * scale) x ceil(h * scale)`; `None` when no bout runs.
    pub fn paint(&self, scale: f32) -> Option<fp_paint::tiny_skia::Pixmap> {
        art::paint_panel(self.bout.as_ref()?, scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 1.0 / 60.0;

    fn stage() -> Frame {
        Frame::new(100.0, 0.0, 1000.0, 1080.0)
    }

    fn run(f: &mut Fight, plough: Option<f64>) -> Vec<FightEvent> {
        let mut out = Vec::new();
        for _ in 0..8000 {
            f.step(DT, plough, &mut out);
            if !f.active() {
                break;
            }
        }
        out
    }

    #[test]
    fn begin_runs_and_finishes() {
        let mut f = Fight::default();
        assert!(!f.active());
        assert!(f.floor().is_none());
        assert!(f.obstacles().is_empty());
        assert!(f.paint(1.0).is_none());
        let mut sp = Vec::new();
        f.sprites(&mut sp);
        assert!(sp.is_empty());
        f.step(DT, None, &mut Vec::new()); // no bout: nothing
        assert!(f.begin(Some(Variant::Quick), stage(), 40.0, 7));
        assert!(f.active());
        assert_eq!(f.floor(), Some(40.0));
        assert!(!f.begin(Some(Variant::Long), stage(), 40.0, 7), "one bout at a time");
        f.sprites(&mut sp);
        assert_eq!(sp.len(), 1);
        let s = sp[0];
        assert_eq!(s.key, SpriteKey::new(Owner::Fight, 0));
        assert_eq!(s.frame, Frame::new(40.0, 40.0, 1120.0, 180.0));
        assert_eq!((s.z, s.hit, s.opacity), (ZBand::Front, HitMode::Never, 1.0));
        let events = run(&mut f, None);
        assert!(!f.active());
        assert_eq!(events.last(), Some(&FightEvent::Finished));
        assert_eq!(events.iter().filter(|e| **e == FightEvent::Finished).count(), 1);
        assert!(events.iter().any(|e| matches!(e, FightEvent::Burst { area, .. } if *area == 380.0)));
        // can start again
        assert!(f.begin(None, stage(), 40.0, 8));
        f.stop();
        assert!(!f.active());
    }

    #[test]
    fn same_seed_same_fight() {
        let play = |seed| {
            let mut f = Fight::default();
            assert!(f.begin(Some(Variant::Long), stage(), 40.0, seed));
            run(&mut f, None)
        };
        assert_eq!(play(3), play(3));
        assert_ne!(play(3), play(4));
    }

    #[test]
    fn height_and_scale_rules() {
        // dock: (floor + 1 - screen bottom) * 0.62 clamped 26..44
        for (floor, expect) in [(59.0, 60.0 * 0.62), (10.0, 26.0), (100.0, 44.0_f64)] {
            let mut f = Fight::default();
            assert!(f.begin(Some(Variant::Both), Frame::new(0.0, 0.0, 800.0, 600.0), floor, 1));
            let size = f.bout.as_ref().unwrap().scale * 58.0;
            assert!((size - expect.clamp(26.0, 44.0)).abs() < 1e-9, "{size} vs {expect}");
        }
        // no dock: floor == stage.y
        let mut f = Fight::default();
        assert!(f.begin(Some(Variant::Both), Frame::new(0.0, 0.0, 800.0, 600.0), 0.0, 1));
        assert!((f.bout.as_ref().unwrap().scale - 44.0 / 58.0).abs() < 1e-12);
    }

    #[test]
    fn unusable_stages_are_refused() {
        let mut f = Fight::default();
        for st in [
            Frame::new(0.0, 0.0, 0.0, 10.0),
            Frame::new(0.0, 0.0, -5.0, 10.0),
            Frame::new(0.0, 0.0, f64::NAN, 10.0),
            Frame::new(f64::INFINITY, 0.0, 100.0, 10.0),
            Frame::new(0.0, f64::NAN, 100.0, 10.0),
        ] {
            assert!(!f.begin(None, st, 0.0, 1));
        }
        assert!(!f.begin(None, stage(), f64::NAN, 1));
        assert!(!f.active());
    }

    #[test]
    fn random_variant_never_repeats_the_previous() {
        let mut f = Fight::default();
        let mut prev = None;
        for seed in 0..40 {
            assert!(f.begin(None, stage(), 0.0, seed));
            let v = f.prev_variant;
            assert_ne!(v, prev);
            prev = v;
            f.stop();
        }
        // an explicit variant updates the memory too
        assert!(f.begin(Some(Variant::Backstab), stage(), 0.0, 1));
        assert_eq!(f.prev_variant, Some(Variant::Backstab));
    }

    #[test]
    fn endings_do_not_repeat_back_to_back() {
        let mut f = Fight::default();
        let mut prev = None;
        for seed in 0..40 {
            assert!(f.begin(Some(Variant::Quick), stage(), 0.0, seed));
            assert_ne!(f.last_ending, prev);
            prev = f.last_ending;
            f.stop();
        }
        // Both / Backstab leave the remembered ending alone
        assert!(f.begin(Some(Variant::Both), stage(), 0.0, 1));
        assert_eq!(f.last_ending, prev);
        f.stop();
        assert!(f.begin(Some(Variant::Backstab), stage(), 0.0, 1));
        assert_eq!(f.last_ending, prev);
    }

    #[test]
    fn plough_veto_window() {
        let st = Frame::new(100.0, 0.0, 1000.0, 10.0);
        assert!(Fight::plough_veto(st, 600.0));
        assert!(Fight::plough_veto(st, -319.0));
        assert!(!Fight::plough_veto(st, -320.0));
        assert!(Fight::plough_veto(st, 1519.0));
        assert!(!Fight::plough_veto(st, 1520.0));
        assert_eq!(PLOUGH_SIGHT, 420.0);
    }

    #[test]
    fn plough_makes_them_flee_and_end_early() {
        let mut calm = Fight::default();
        assert!(calm.begin(Some(Variant::Long), stage(), 0.0, 11));
        let calm_events = run(&mut calm, None);
        let mut f = Fight::default();
        assert!(f.begin(Some(Variant::Long), stage(), 0.0, 11));
        let mut out = Vec::new();
        for _ in 0..300 {
            f.step(DT, Some(-10_000.0), &mut out);
        }
        // the plough far away does nothing; a close one panics
        let mut frames = 0;
        while f.active() && frames < 8000 {
            f.step(DT, Some(600.0), &mut out);
            frames += 1;
        }
        assert!(!f.active());
        assert!(frames < 400, "panic ends the fight quickly ({frames})");
        assert_eq!(out.last(), Some(&FightEvent::Finished));
        assert!(calm_events.contains(&FightEvent::Finished));
    }

    #[test]
    fn hidden_screen_gives_opacity_zero_and_non_finite_dt_is_safe() {
        let mut f = Fight::default();
        assert!(f.begin(Some(Variant::Both), stage(), 0.0, 2));
        f.set_shown(false);
        let mut sp = Vec::new();
        f.sprites(&mut sp);
        assert_eq!(sp[0].opacity, 0.0);
        f.set_shown(true);
        sp.clear();
        f.sprites(&mut sp);
        assert_eq!(sp[0].opacity, 1.0);
        let rev = sp[0].rev;
        let mut out = Vec::new();
        f.step(f64::NAN, None, &mut out);
        f.step(-1.0, None, &mut out);
        f.step(5.0, None, &mut out); // clamped to 0.1
        assert_eq!(f.bout.as_ref().unwrap().age, 0.1);
        sp.clear();
        f.sprites(&mut sp);
        assert!(sp[0].rev > rev);
    }

    #[test]
    fn obstacles_follow_the_fighters() {
        let mut f = Fight::default();
        assert!(f.begin(Some(Variant::Both), stage(), 0.0, 9));
        let mut out = Vec::new();
        let mut max_seen = 0;
        for _ in 0..600 {
            f.step(DT, None, &mut out);
            max_seen = max_seen.max(f.obstacles().len());
        }
        assert_eq!(max_seen, 2);
    }

    #[test]
    fn paint_covers_the_whole_fight() {
        let mut f = Fight::default();
        assert!(f.begin(Some(Variant::Backstab), stage(), 40.0, 5));
        let mut out = Vec::new();
        let mut inked = 0;
        for n in 0..1200 {
            f.step(DT, None, &mut out);
            if n % 30 == 0
                && let Some(px) = f.paint(1.0)
            {
                assert_eq!((px.width(), px.height()), (1120, 180));
                if px.pixels().iter().any(|p| p.alpha() > 0) {
                    inked += 1;
                }
            }
            if !f.active() {
                break;
            }
        }
        assert!(inked > 10);
        let mut f = Fight::default();
        assert!(f.begin(Some(Variant::Quick), stage(), 0.0, 5));
        let px = f.paint(1.5).unwrap();
        assert_eq!((px.width(), px.height()), (1680, 270));
        assert!(f.paint(0.0).is_none());
        assert!(f.paint(f32::NAN).is_none());
    }
}
