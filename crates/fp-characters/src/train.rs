//! The local toy train (characters 5, local only) and its carrier elf.

use crate::art::{self, ElfLook, c, poly};
use crate::util::{arc, canvas, ease, find_screen, sprite};
use crate::{Cargo, CharacterEvent, Pick, StepCtx};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::Rng;
use fp_core::scene::{HitMode, Owner, Sprite, ZBand};
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, rgba};

const CRUISE: f64 = 150.0;
const BRAKE: f64 = 140.0;
const ACCEL: f64 = 55.0;
const REST_X: f64 = 108.0;
const DECK: f64 = 24.0;
const STAND: f64 = 44.0;
const SPOT: f64 = 96.0;
const CARGO_AT: f64 = 56.0;
const ELF_SPEED: f64 = 130.0;
const VENT_FOR: f64 = 1.4;
const MARGIN: f64 = 24.0;
const AHEAD: f64 = 160.0;
const SKY: f64 = 240.0;
const ELF_W: f64 = 44.0;
const ELF_H: f64 = 60.0;
const ELF_FOOT: f64 = 3.0;

/// Train art selection (pref `trainLook`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrainLook {
    /// The default, longer festive train.
    #[default]
    Festive,
    /// The short classic train.
    Classic,
}

impl TrainLook {
    /// Pref string: `classic`, otherwise festive.
    pub fn parse(s: &str) -> Self {
        if s == "classic" { TrainLook::Classic } else { TrainLook::Festive }
    }

    pub fn raw(self) -> &'static str {
        match self {
            TrainLook::Festive => "festive",
            TrainLook::Classic => "classic",
        }
    }

    /// Train length in points.
    pub fn length(self) -> f64 {
        match self {
            TrainLook::Festive => 580.0,
            TrainLook::Classic => 436.0,
        }
    }

    fn roof(self) -> (f64, f64) {
        match self {
            TrainLook::Festive => (-75.0, 88.0),
            TrainLook::Classic => (-3.0, 84.0),
        }
    }

    /// Chimney and cylinder emitters (train-local).
    fn emitters(self) -> ((f64, f64), (f64, f64)) {
        match self {
            TrainLook::Festive => ((548.0, 106.0), (552.0, 16.0)),
            TrainLook::Classic => ((404.0, 104.0), (408.0, 16.0)),
        }
    }
}

/// A queued delivery (`trainQueue` entry).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Delivery {
    pub id: i64,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub cargo: Cargo,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Coming,
    Stopped,
    Waiting,
    Fetching,
    Boarding,
    Leaving,
    /// A delivery with nothing to unload rolls through without stopping.
    Passing,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ElfPose {
    x: f64,
    y: f64,
    out: bool,
    look: ElfLook,
}

#[derive(Clone, Copy, Debug)]
struct Fetch {
    pick: Pick,
    /// Offset of the object from the tail.
    d: f64,
}

#[derive(Clone, Debug)]
struct Run {
    screen: Frame,
    summoned: bool,
    front: f64,
    speed: f64,
    phase: Phase,
    phase_at: f64,
    travelled: f64,
    cargo: Option<Cargo>,
    placed: bool,
    fetch: Option<Fetch>,
    took: bool,
    landed: bool,
    shown: bool,
    stop_at: f64,
    end: f64,
    elf: Option<ElfPose>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Smoke,
    Ember,
    Steam,
}

#[derive(Clone, Copy, Debug)]
struct Puff {
    kind: Kind,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    ax: f64,
    ay: f64,
    age: f64,
    life: f64,
    scale: f64,
    grow: f64,
    alpha: f64,
    fade: f64,
}

/// The train.
#[derive(Clone, Debug, Default)]
pub struct Train {
    look: TrainLook,
    queue: Vec<Delivery>,
    next_id: i64,
    next_at: f64,
    summon: bool,
    started: bool,
    run: Option<Run>,
    last_screen: Option<Frame>,
    puffs: Vec<Puff>,
    acc: [f64; 3],
    events: Vec<CharacterEvent>,
    now: f64,
}

/// Fetch timeline marks `[t1, t2, t3, t4, t5]` for an object at offset `d`.
fn marks(d: f64) -> [f64; 5] {
    let t1 = (0.4f64).max((d + STAND).abs() / ELF_SPEED);
    let t2 = t1 + 0.4;
    let t3 = t2 + (0.4f64).max((d + 18.0).abs() / ELF_SPEED);
    let t4 = t3 + 0.55;
    [t1, t2, t3, t4, t4 + 0.5]
}

fn pose(now: f64, x: f64, y: f64, out: bool, f: (bool, bool, bool, bool)) -> ElfPose {
    let (walking, carrying, waving, hopping) = f;
    ElfPose { x, y, out, look: ElfLook { t: now, walking, carrying, waving, hopping } }
}

/// Carrier elf while a delivery unloads (characters 5.3 `Act`).
fn deliver_pose(t: f64, now: f64) -> ElfPose {
    if t < 0.5 {
        pose(now, REST_X, DECK, true, (false, true, false, false))
    } else if t < 1.0 {
        let u = (t - 0.5) / 0.5;
        pose(now, ease(REST_X, -18.0, u), ease(DECK, 0.0, u) + arc(26.0, u), true, (false, true, false, true))
    } else if t < 2.3 {
        pose(now, ease(-18.0, -96.0, (t - 1.0) / 1.3), 0.0, true, (true, true, false, false))
    } else if t < 2.9 {
        let hop = t < 2.7;
        pose(now, -96.0, if hop { arc(26.0, (t - 2.3) / 0.4) } else { 0.0 }, true, (false, false, false, hop))
    } else if t < 4.1 {
        pose(now, ease(-96.0, -18.0, (t - 2.9) / 1.2), 0.0, false, (true, false, false, false))
    } else {
        let u = ((t - 4.1) / 0.6).min(1.0);
        pose(now, ease(-18.0, REST_X, u), ease(0.0, DECK, u) + arc(26.0, u), false, (false, false, false, true))
    }
}

/// Elf stepping off at the station.
fn station_pose(t: f64, now: f64) -> ElfPose {
    if t < 0.5 {
        pose(now, REST_X, DECK, true, (false, false, false, false))
    } else {
        let u = (t - 0.5) / 0.5;
        pose(now, ease(REST_X, -STAND, u), ease(DECK, 0.0, u) + arc(26.0, u), true, (false, false, false, true))
    }
}

impl Train {
    pub(crate) fn start(&mut self, now: f64, rng: &mut impl Rng) {
        self.started = true;
        self.now = now;
        self.next_at = if self.queue.is_empty() { f64::INFINITY } else { now + rng.range(20.0, 40.0) };
    }

    pub(crate) fn stop(&mut self) {
        self.started = false;
        self.summon = false;
        self.vanish();
    }

    /// Ends the run without queueing; fires `Left` for a summoned one.
    fn vanish(&mut self) {
        if self.run.take().is_some_and(|r| r.summoned) {
            self.events.push(CharacterEvent::Left);
        }
        self.puffs.clear();
    }

    pub fn look(&self) -> TrainLook {
        self.look
    }

    pub fn set_look(&mut self, look: TrainLook) {
        self.look = look;
    }

    /// Ask for a summoned run; consumed on the next step.
    pub(crate) fn summon(&mut self) {
        if self.started && self.run.is_none() {
            self.summon = true;
        }
    }

    pub fn busy(&self) -> bool {
        self.run.is_some() || self.summon
    }

    /// The summoned train waits at the station.
    pub fn at_station(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.phase == Phase::Waiting)
    }

    pub(crate) fn fetch(&mut self, pick: Pick, x: f64, now: f64) {
        if let Some(r) = self.run.as_mut().filter(|r| r.phase == Phase::Waiting) {
            let tail = r.front - self.look.length();
            r.fetch = Some(Fetch { pick, d: x - tail });
            r.go(Phase::Fetching, now);
        }
    }

    pub(crate) fn dismiss(&mut self, now: f64) {
        if let Some(r) = self.run.as_mut().filter(|r| r.phase == Phase::Waiting) {
            r.go(Phase::Boarding, now);
        }
    }

    pub(crate) fn load(&mut self, cargo: Cargo) -> bool {
        match self.run.as_mut().filter(|r| r.summoned && r.fetch.is_some()) {
            Some(r) => {
                r.cargo = Some(cargo);
                true
            }
            None => false,
        }
    }

    pub(crate) fn take_events(&mut self) -> Vec<CharacterEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub(crate) fn queue_json(&self) -> serde_json::Value {
        serde_json::to_value(&self.queue).unwrap_or_default()
    }

    pub(crate) fn load_queue(&mut self, v: &serde_json::Value) {
        self.queue = v
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| {
                        let id = d.get("id")?.as_i64()?;
                        let from = d.get("from").and_then(|f| f.as_str()).unwrap_or("").to_string();
                        let cargo =
                            d.get("cargo").and_then(|c| serde_json::from_value(c.clone()).ok()).unwrap_or_default();
                        Some(Delivery { id, from, cargo })
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.next_id = self.queue.iter().map(|d| d.id + 1).max().unwrap_or(0);
        if self.started && self.next_at.is_infinite() && !self.queue.is_empty() {
            self.next_at = self.now;
        }
    }

    /// Where a carried snowman rides, for the yeti to chase.
    pub(crate) fn riding(&self) -> Option<Vec<(Vec2, f64)>> {
        let r = self.run.as_ref()?;
        let radii = r.cargo.as_ref()?.snowman.as_ref()?.get("radii")?.as_array()?;
        let mut y = self.carry_point()?.y;
        let x = self.carry_point()?.x;
        radii
            .iter()
            .map(|v| v.as_f64().filter(|r| r.is_finite() && *r > 0.0))
            .map(|r| {
                r.map(|r| {
                    let part = (Vec2::new(x, y + r), r);
                    y += 2.0 * r;
                    part
                })
            })
            .collect::<Option<Vec<_>>>()
            .filter(|p| !p.is_empty())
    }

    #[cfg(test)]
    pub(crate) fn test_set_cargo(&mut self, snowman: Option<serde_json::Value>) {
        if let Some(r) = self.run.as_mut() {
            r.cargo = Some(Cargo { snowman, tree: None });
        }
    }

    pub fn cargo_aboard(&self) -> Option<&Cargo> {
        let r = self.run.as_ref().filter(|r| !r.summoned && !r.placed)?;
        r.cargo.as_ref().filter(|c| !c.is_empty())
    }

    /// Where the world draws the carried or riding object.
    pub fn carry_point(&self) -> Option<Vec2> {
        let r = self.run.as_ref()?;
        let tail = r.front - self.look.length();
        let floor = r.screen.min_y();
        if !r.summoned {
            self.cargo_aboard()?;
            let e = r.elf.unwrap_or_else(|| default_pose(self.now, r.speed > 0.0));
            return Some(Vec2::new(tail + e.x, floor + e.y + 46.0));
        }
        let f = r.fetch?;
        if r.landed {
            let jitter = if r.speed > 0.0 { 0.6 * (14.0 * self.now).sin() } else { 0.0 };
            return Some(Vec2::new(tail + CARGO_AT, floor + DECK + jitter));
        }
        let t = self.now - r.phase_at;
        let m = marks(f.d);
        if t < m[0] {
            return None;
        }
        let e = r.elf?;
        let lift = ((t - m[0] - 0.2) / 0.2).clamp(0.0, 1.0);
        let setting = ease(0.0, 1.0, (t - m[3]) / 0.5);
        let x = tail + e.x + (CARGO_AT - e.x) * setting;
        Some(Vec2::new(x, floor + e.y + (4.0 + 42.0 * lift) * (1.0 - setting)))
    }

    fn begin(&mut self, c: &StepCtx<'_>) {
        let screen = if self.summon {
            c.visit_screens.iter().find(|s| s.shown)
        } else if !self.queue.is_empty() && c.now >= self.next_at {
            let shown = || c.visit_screens.iter().filter(|s| s.shown);
            shown().find(|s| Some(s.frame) != self.last_screen).or_else(|| shown().next())
        } else {
            None
        };
        let summoned = std::mem::take(&mut self.summon);
        let Some(screen) = screen else { return };
        let f = screen.frame;
        let len = self.look.length();
        let cargo = if summoned { None } else { self.queue.first().map(|d| d.cargo.clone()) };
        self.run = Some(Run {
            screen: f,
            summoned,
            front: f.min_x() - 6.0,
            speed: CRUISE,
            phase: if summoned || cargo.as_ref().is_some_and(|c| !c.is_empty()) {
                Phase::Coming
            } else {
                Phase::Passing
            },
            phase_at: c.now,
            travelled: 0.0,
            cargo,
            placed: false,
            fetch: None,
            took: false,
            landed: false,
            shown: screen.shown,
            stop_at: f.max_x() - (f.w * 0.04).max(24.0),
            end: f.max_x() + len + 20.0,
            elf: None,
        });
    }

    pub(crate) fn step(&mut self, c: &StepCtx<'_>, rng: &mut impl Rng, out: &mut Vec<CharacterEvent>) {
        out.append(&mut self.events);
        self.now = c.now;
        if self.run.is_none() {
            self.begin(c);
        }
        let Some(mut run) = self.run.take() else { return };
        let Some(screen) = find_screen(c.visit_screens, run.screen) else {
            if run.summoned {
                out.push(CharacterEvent::Left);
            }
            self.puffs.clear();
            return;
        };
        run.shown = screen.shown;
        let dt = c.dt.max(0.0);
        let now = c.now;
        let len = self.look.length();
        match run.phase {
            Phase::Coming => {
                let d = run.stop_at - run.front;
                let v = CRUISE.min((BRAKE * d.max(0.0)).sqrt());
                if d <= 0.5 || v < 2.0 {
                    run.front = run.stop_at;
                    run.speed = 0.0;
                    run.go(Phase::Stopped, now);
                } else {
                    run.speed = v;
                }
            }
            Phase::Leaving => run.speed = CRUISE.min(run.speed + ACCEL * dt),
            Phase::Passing => run.speed = CRUISE,
            _ => run.speed = 0.0,
        }
        run.front += run.speed * dt;
        run.travelled += run.speed * dt;
        let tail = run.front - len;
        let t = now - run.phase_at;
        run.elf = None;
        match run.phase {
            Phase::Stopped if run.summoned => {
                if t >= 1.0 {
                    run.go(Phase::Waiting, now);
                    out.push(CharacterEvent::TrainReady { at: Vec2::new(tail - STAND, run.screen.min_y()) });
                } else {
                    run.elf = Some(station_pose(t, now));
                }
            }
            Phase::Stopped => {
                if t >= 4.7 {
                    run.go(Phase::Leaving, now);
                } else {
                    if t >= 2.3 && !run.placed {
                        run.placed = true;
                        let at = Vec2::new(tail - SPOT, run.screen.min_y() + 20.0);
                        out.push(CharacterEvent::Burst { at, area: 220.0, velocity: Vec2::ZERO });
                        let cargo = run.cargo.clone().unwrap_or_default();
                        out.push(CharacterEvent::Unload { cargo, x: tail - SPOT, screen: run.screen });
                    }
                    run.elf = Some(deliver_pose(t, now));
                }
            }
            Phase::Waiting => {
                let waving = (t / 3.0).floor() % 2.0 == 0.0 && t % 3.0 < 1.6;
                run.elf = Some(pose(now, -STAND, 0.0, true, (false, false, waving, false)));
            }
            Phase::Fetching => self.fetching(&mut run, t, tail, out),
            Phase::Boarding => {
                if t >= 0.9 {
                    run.go(Phase::Leaving, now);
                } else {
                    let u = ((t - 0.1) / 0.6).clamp(0.0, 1.0);
                    let (x, y) = (ease(-STAND, REST_X, u), ease(0.0, DECK, u) + arc(26.0, u));
                    run.elf = Some(pose(now, x, y, u >= 1.0, (false, false, false, u > 0.0 && u < 1.0)));
                }
            }
            Phase::Coming | Phase::Leaving | Phase::Passing => {}
        }
        self.emit(&run, c, rng);
        if run.front > run.end {
            self.finish(&run, c.now, rng, out);
        } else {
            self.run = Some(run);
        }
    }

    fn fetching(&mut self, run: &mut Run, t: f64, tail: f64, out: &mut Vec<CharacterEvent>) {
        let Some(f) = run.fetch else { return };
        let now = self.now;
        let m = marks(f.d);
        let d = f.d;
        let p = |x: f64, y: f64, out: bool, fl: (bool, bool, bool, bool)| Some(pose(now, x, y, out, fl));
        run.elf = if t < m[0] {
            p(ease(-STAND, d, t / m[0]), 0.0, d < -STAND, (true, false, false, false))
        } else if t < m[1] {
            if !run.took {
                run.took = true;
                out.push(CharacterEvent::Took { pick: f.pick });
            }
            let hop = t - m[0] < 0.2;
            p(d, if hop { arc(12.0, (t - m[0]) / 0.2) } else { 0.0 }, false, (false, !hop, false, hop))
        } else if t < m[2] {
            p(ease(d, -18.0, (t - m[1]) / (m[2] - m[1])), 0.0, false, (true, true, false, false))
        } else if t < m[3] {
            let u = (t - m[2]) / (m[3] - m[2]);
            p(ease(-18.0, REST_X, u), ease(0.0, DECK, u) + arc(26.0, u), false, (false, true, false, true))
        } else if t < m[4] {
            p(REST_X, DECK, false, (false, true, false, false))
        } else {
            if !run.landed {
                run.landed = true;
                let at = Vec2::new(tail + CARGO_AT, run.screen.min_y() + DECK + 20.0);
                out.push(CharacterEvent::Burst { at, area: 220.0, velocity: Vec2::ZERO });
                out.push(CharacterEvent::Loaded { pick: f.pick });
            }
            if t >= m[4] + 0.5 {
                run.go(Phase::Leaving, now);
            }
            None
        };
    }

    fn finish(&mut self, run: &Run, now: f64, rng: &mut impl Rng, out: &mut Vec<CharacterEvent>) {
        self.puffs.clear();
        if run.summoned {
            if let Some(cargo) = run.cargo.clone().filter(|c| !c.is_empty()) {
                self.queue.push(Delivery { id: self.next_id, from: String::new(), cargo });
                self.next_id += 1;
                self.next_at = now + rng.range(20.0, 40.0);
                self.last_screen = Some(run.screen);
                out.push(CharacterEvent::Dirty);
            }
            out.push(CharacterEvent::Left);
        } else {
            if !self.queue.is_empty() {
                self.queue.remove(0);
            }
            self.next_at = now + rng.range(60.0, 120.0);
            out.push(CharacterEvent::Dirty);
        }
    }

    /// Smoke, embers and steam.
    fn emit(&mut self, run: &Run, c: &StepCtx<'_>, rng: &mut impl Rng) {
        let dt = c.dt.max(0.0).clamp(0.0, 0.05); // max first: a NaN dt becomes 0
        let moving = run.speed > 0.0;
        for p in &mut self.puffs {
            p.age += dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.vx += p.ax * dt;
            p.vy += p.ay * dt;
            p.scale += p.grow * dt;
            p.alpha -= p.fade * dt;
        }
        self.puffs.retain(|p| p.age < p.life && p.alpha > 0.0);
        let venting = matches!(run.phase, Phase::Stopped | Phase::Leaving) && c.now - run.phase_at < VENT_FOR;
        let rates = [if moving { 4.0 } else { 1.0 }, if moving { 4.0 } else { 0.0 }, if venting { 30.0 } else { 0.0 }];
        let (chimney, cylinders) = self.look.emitters();
        for (i, rate) in rates.into_iter().enumerate() {
            self.acc[i] += rate * dt;
            while self.acc[i] >= 1.0 {
                self.acc[i] -= 1.0;
                let kind = [Kind::Smoke, Kind::Ember, Kind::Steam][i];
                let puff = spawn(kind, if i == 2 { cylinders } else { chimney }, moving, rng);
                self.puffs.push(puff);
            }
        }
    }

    pub(crate) fn sprites(&self, rev: u64, out: &mut Vec<Sprite>) {
        let Some(r) = &self.run else { return };
        let alpha = if r.shown { 1.0 } else { 0.0 };
        let len = self.look.length();
        let tail = r.front - len;
        let floor = r.screen.min_y();
        let frame = Frame::new(tail - MARGIN, floor, MARGIN + len + AHEAD, SKY);
        out.push(sprite(Owner::Character, 4, frame, (ZBand::Front, HitMode::Never), rev, alpha));
        let e = r.elf.unwrap_or_else(|| default_pose(self.now, r.speed > 0.0));
        let frame = Frame::new(tail + e.x - ELF_W / 2.0, floor + e.y - ELF_FOOT, ELF_W, ELF_H);
        out.push(sprite(Owner::Character, 5, frame, (ZBand::Front, HitMode::Never), rev, alpha));
    }

    pub(crate) fn paint(&self, id: u64, scale: f32) -> Option<Pixmap> {
        let r = self.run.as_ref()?;
        let t = self.now;
        if id == 5 {
            let e = r.elf.unwrap_or_else(|| default_pose(t, r.speed > 0.0));
            let mut pm = canvas(ELF_W, ELF_H, scale)?;
            let mut s = Sketch::new(&mut pm, scale);
            s.translate((ELF_W / 2.0) as f32, ELF_FOOT as f32);
            s.scale_by(if e.out { -1.0 } else { 1.0 }, 1.0);
            art::elf(&mut s, &e.look);
            return Some(pm);
        }
        let len = self.look.length();
        let mut pm = canvas(MARGIN + len + AHEAD, SKY, scale)?;
        let mut s = Sketch::new(&mut pm, scale);
        s.translate(MARGIN as f32, 0.0);
        draw_train(&mut s, self.look, t, r.travelled);
        for p in &self.puffs {
            let a = p.alpha.clamp(0.0, 1.0);
            let (x, y) = (p.x as f32, p.y as f32);
            match p.kind {
                Kind::Ember => s.circle(x, y, (2.2 * p.scale) as f32, rgba(1.0, 0.62, 0.15, a)),
                _ => s.halo(x, y, (24.0 * p.scale) as f32, rgba(1.0, 1.0, 1.0, a * 0.8)),
            }
        }
        let (rx, ry) = self.look.roof();
        let bob = if r.speed > 0.0 { 1.2 * (14.0 * t).sin() } else { 0.0 };
        let look =
            ElfLook { t, walking: false, carrying: false, waving: (t / 4.0).floor() % 2.0 == 0.0, hopping: false };
        s.at((len / 2.0 + rx) as f32, (ry + bob) as f32, 0.0, 1.0, |s| {
            s.scale_by(-1.0, 1.0);
            art::elf(s, &look);
        });
        Some(pm)
    }
}

impl Run {
    fn go(&mut self, phase: Phase, now: f64) {
        self.phase = phase;
        self.phase_at = now;
    }
}

/// Elf riding at rest on the deck.
fn default_pose(now: f64, moving: bool) -> ElfPose {
    let y = DECK + if moving { 1.2 * (14.0 * now).sin() } else { 0.0 };
    let waving = moving && (now / 3.0).floor() as i64 % 3 == 0;
    pose(now, REST_X, y, true, (false, false, waving, false))
}

fn spawn(kind: Kind, at: (f64, f64), moving: bool, rng: &mut impl Rng) -> Puff {
    use std::f64::consts::FRAC_PI_2;
    let mut p =
        |speed: (f64, f64), dir: (f64, f64), life: (f64, f64), scale: (f64, f64), rest: (f64, f64, f64, f64, f64)| {
            let v = rng.range(speed.0, speed.1);
            let a = rng.range(dir.0, dir.1);
            let (grow, alpha, fade, ax, ay) = rest;
            Puff {
                kind,
                x: at.0 + rng.range(-4.0, 4.0),
                y: at.1,
                vx: v * a.cos(),
                vy: v * a.sin(),
                ax,
                ay,
                age: 0.0,
                life: rng.range(life.0, life.1),
                scale: rng.range(scale.0, scale.1),
                grow,
                alpha,
                fade,
            }
        };
    match kind {
        Kind::Smoke => p(
            (20.0, 48.0),
            (FRAC_PI_2 - 0.225, FRAC_PI_2 + 0.225),
            (2.2, 3.4),
            (0.23, 0.47),
            (0.32, 0.85, 0.33, if moving { -34.0 } else { -6.0 }, 6.0),
        ),
        Kind::Ember => {
            p((30.0, 80.0), (FRAC_PI_2 - 0.2, FRAC_PI_2 + 0.2), (0.6, 1.2), (0.5, 1.3), (0.0, 1.0, 1.1, -40.0, -30.0))
        }
        Kind::Steam => p((40.0, 80.0), (-0.025, 0.325), (0.8, 1.4), (0.1, 0.26), (0.5, 0.8, 0.85, 0.0, 40.0)),
    }
}

fn wheel(s: &mut Sketch, x: f32, y: f32, r: f32, travelled: f64) {
    let angle = -(travelled / f64::from(r)) as f32;
    s.at(x, y, angle, 1.0, |s| {
        s.circle(0.0, 0.0, r, c(0x33333D));
        s.circle(0.0, 0.0, r - 2.0, c(0xCC212B));
        for k in 0..6 {
            let a = k as f32 * std::f32::consts::TAU / 6.0;
            s.line(0.0, 0.0, (r - 2.0) * a.cos(), (r - 2.0) * a.sin(), c(0xF2BD40), if r > 10.0 { 2.0 } else { 1.4 });
        }
        s.circle(0.0, 0.0, r * 0.25, c(0xF2BD40));
    });
}

/// Bulb rows `(x0, x1, y)`.
fn rows(look: TrainLook) -> (&'static [(f32, f32, f32)], f32, f32) {
    match look {
        TrainLook::Festive => (
            &[
                (8.0, 136.0, 30.0),
                (148.0, 292.0, 89.0),
                (152.0, 288.0, 15.0),
                (298.0, 442.0, 89.0),
                (302.0, 438.0, 15.0),
                (448.0, 508.0, 97.0),
                (452.0, 500.0, 15.0),
                (506.0, 572.0, 77.0),
            ],
            10.0,
            5.0,
        ),
        TrainLook::Classic => (&[(150.0, 270.0, 85.0), (280.0, 338.0, 97.0)], 12.0, 6.0),
    }
}

/// Cars, engine and wheel layout of one train look.
#[derive(Clone, Copy)]
struct Consist {
    /// `(x, width, colour)` per coach.
    coaches: &'static [(f32, f32, u32)],
    /// Engine `(x, width, chimney x, lamp x)`.
    engine: (f32, f32, f32, f32),
    wheels: &'static [f32],
    drivers: [f32; 2],
    pilot: f32,
    couplings: &'static [f32],
}

const FESTIVE: Consist = Consist {
    coaches: &[(150.0, 140.0, 0xCC212B), (300.0, 140.0, 0x21804C)],
    engine: (450.0, 130.0, 548.0, 574.0),
    wheels: &[26.0, 50.0, 98.0, 122.0, 170.0, 196.0, 240.0, 266.0, 312.0, 338.0, 382.0, 408.0, 452.0],
    drivers: [486.0, 522.0],
    pilot: 560.0,
    couplings: &[140.0, 290.0, 440.0],
};

const CLASSIC: Consist = Consist {
    coaches: &[(150.0, 120.0, 0x21804C)],
    engine: (280.0, 156.0, 404.0, 428.0),
    wheels: &[26.0, 50.0, 98.0, 122.0, 174.0, 200.0, 238.0, 262.0, 302.0],
    drivers: [336.0, 372.0],
    pilot: 414.0,
    couplings: &[140.0, 270.0],
};

fn draw_train(s: &mut Sketch, look: TrainLook, t: f64, travelled: f64) {
    let festive = look == TrainLook::Festive;
    let Consist { coaches, engine, wheels, drivers, pilot, couplings } = if festive { FESTIVE } else { CLASSIC };
    // headlamp beam first, behind everything
    let flicker = (0.3 + 0.08 * (7.3 * t).sin() + 0.05 * (19.0 * t).sin()) as f32;
    let (ex, ew, stack_x, lamp_x) = engine;
    let beam = poly(&[(lamp_x, 47.0), (lamp_x + 150.0, 0.0), (lamp_x + 150.0, 90.0), (lamp_x, 53.0)]);
    s.across(
        &beam,
        &[rgba(1.0, 0.95, 0.7, f64::from(flicker + 0.25)), rgba(1.0, 0.95, 0.7, 0.0)],
        None,
        (lamp_x, 50.0),
        (lamp_x + 150.0, 50.0),
    );
    // flat car
    s.rect(0.0, 12.0, 140.0, 9.0, 1.5, c(0x8C5C33));
    for x in [20.0f32, 44.0, 68.0, 92.0, 116.0] {
        s.line(x, 12.0, x, 21.0, c(0x664021), 1.0);
    }
    s.rect(0.0, 21.0, 140.0, 3.0, 1.0, c(0xCC212B));
    s.rect(0.0, 24.0, 140.0, 2.0, 0.0, c(0xFFFFFF));
    // coaches
    for &(x, w, tone) in coaches {
        s.rect(x, 14.0, w, 62.0, 6.0, c(tone));
        s.rect(x, 14.0, w, 8.0, 3.0, rgba(0.0, 0.0, 0.0, 0.25));
        s.rect(x - 4.0, 76.0, w + 8.0, 6.0, 2.0, c(0xF2BD40));
        s.line(x, 52.0, x + w, 52.0, c(0xF2BD40), 1.6);
        let mut wx = x + 12.0;
        while wx + 20.0 <= x + w - 8.0 {
            s.rect(wx, 56.0, 18.0, 16.0, 3.0, c(0xFFE68C));
            wx += 28.0;
        }
    }
    // engine
    s.rect(ex, 22.0, ew - 40.0, 44.0, 14.0, c(0xCC212B));
    s.rect(ex + 40.0, 22.0, 6.0, 44.0, 0.0, c(0xF2BD40));
    s.rect(ex + 76.0, 22.0, 6.0, 44.0, 0.0, c(0xF2BD40));
    s.rect(ex - 6.0, 22.0, 50.0, 66.0, 5.0, c(0xCC212B));
    s.rect(ex - 8.0, 86.0, 54.0, 6.0, 2.0, c(0x33333D));
    s.rect(ex + 6.0, 56.0, 28.0, 22.0, 3.0, c(0xFFE68C));
    s.rect(stack_x - 8.0, 64.0, 16.0, 40.0, 3.0, c(0x33333D));
    s.rect(stack_x - 12.0, 100.0, 24.0, 6.0, 3.0, c(0x33333D));
    s.circle(lamp_x, 50.0, 6.0, c(0xFFF2B3));
    s.circle(lamp_x + 4.0, 34.0, 11.0, c(0x21804C));
    s.circle(lamp_x + 4.0, 34.0, 6.0, c(0xCC212B));
    s.polygon(&[(lamp_x - 10.0, 22.0), (lamp_x + 14.0, 22.0), (lamp_x + 14.0, 4.0)], c(0x33333D));
    if festive {
        s.rect(444.0, 26.0, 22.0, 18.0, 3.0, c(0x1C1C22));
        let glow = (0.7 + 0.3 * (11.0 * t).sin().abs()) as f32;
        s.halo(455.0, 35.0, 35.0, rgba(1.0, 0.6, 0.2, f64::from(0.75 * glow)));
        for (i, (w, h, tone)) in
            [(14.0f32, 16.0f32, 0xFF7319u32), (11.0, 12.0, 0xFFB833), (8.0, 8.0, 0xFFF299)].into_iter().enumerate()
        {
            let fi = i as f64;
            let sy = (0.75 + 0.3 * (t * (13.0 + 4.0 * fi) + fi).sin().abs()) as f32;
            let sx = (0.9 + 0.2 * (9.0 * t + 2.0 * fi).sin()) as f32;
            s.at(455.0, 28.0, 0.0, 1.0, |s| {
                s.scale_by(sx, sy);
                s.polygon(&[(-w / 2.0, 0.0), (w / 2.0, 0.0), (0.0, h)], rgba_of(tone, 0.95));
            });
        }
    }
    for x in couplings {
        s.line(*x, 22.0, x + 10.0, 22.0, c(0x33333D), 3.0);
    }
    // wheels and rod
    for x in wheels {
        wheel(s, *x, 8.0, 8.0, travelled);
    }
    wheel(s, pilot, 7.0, 7.0, travelled);
    for x in drivers {
        wheel(s, x, 14.0, 14.0, travelled);
    }
    let theta = -(travelled / 14.0);
    let (ox, oy) = (7.0 * theta.cos() as f32, 7.0 * theta.sin() as f32);
    s.line(drivers[0] + ox, 14.0 + oy, drivers[1] + ox, 14.0 + oy, c(0xBDC4D1), 4.0);
    // lights
    let (lines, step, first) = rows(look);
    let group = (t * 1.6).floor() as i64 % 2;
    let mut k = 0usize;
    for &(x0, x1, y) in lines {
        let mut x = x0 + first;
        while x <= x1 {
            if (k % 2) as i64 == group {
                let tone = if festive { 0xFFE699 } else { [0xFF404D, 0xFFD933, 0x4DBFFF, 0x66E673][k % 4] };
                s.halo(x, y, if festive { 3.4 } else { 3.6 }, rgba_of(tone, 0.35));
                s.circle(x, y, 1.9, c(if festive { 0xFFEDB2 } else { tone }));
            }
            k += 1;
            x += step;
        }
    }
}

fn rgba_of(rgb: u32, a: f64) -> fp_paint::tiny_skia::Color {
    rgba(f64::from(rgb >> 16 & 255) / 255.0, f64::from(rgb >> 8 & 255) / 255.0, f64::from(rgb & 255) / 255.0, a)
}
