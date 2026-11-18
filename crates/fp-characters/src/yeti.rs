//! The yeti (characters 3, Q3: a pending manual send times out).

use crate::art::c;
use crate::util::{canvas, ease, find_screen, sprite};
use crate::{CharacterEvent, StageScreen, StepCtx};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::Rng;
use fp_core::scene::{HitMode, Owner, Sprite, ZBand};
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, rgba};

const DASH: f64 = 430.0;
const WALK: f64 = 300.0;
const PEEK_IN: f64 = 46.0;
const WALK_IN: f64 = 100.0;
const OUTSIDE: f64 = 60.0;
const PEEK_TIME: f64 = 0.7;
const SCOOP_TIME: f64 = 0.6;
const AIM_TIME: f64 = 0.32;
const RELOAD_TIME: f64 = 0.85;
const REACT_TIME: f64 = 0.8;
const SHOTS: u32 = 2;
const CHASE_TIME: f64 = 1.1;
const CHOPS: u32 = 3;
const WIND_UP_TIME: f64 = 0.5;
const REST_TIME: f64 = 0.7;
const GRAVITY: f64 = 1500.0;
const BALL_R: f64 = 8.0;
const SPLAT_SNOW: f64 = 160.0;
const CLEARANCE: f64 = 22.0;
const LOWEST_ARC: f64 = 240.0;
const SCALE: f64 = 0.66;
const WIN_W: f64 = 140.0 * SCALE;
const WIN_H: f64 = 112.0 * SCALE;
const FEET: f64 = 6.0 * SCALE;
/// How long a manual send waits for a target before it is forgotten (Q3).
pub(crate) const SEND_TIMEOUT: f64 = 60.0;
/// Id offset of ball sprites.
const BALL_KEY: u64 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Entering,
    Peeking,
    Walking,
    Dashing,
    Scooping,
    Aiming,
    Waiting,
    WindingUp,
    Resting,
    Reacting,
    Leaving,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Prey(u64),
    Chop(u64),
    Riding,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pose {
    Stand,
    Run,
    Surprised,
    Scoop,
    Aim,
    Threw,
    Cheer,
    Sad,
}

/// What the yeti looks like this frame.
#[derive(Clone, Copy, Debug)]
struct Look {
    pose: Pose,
    a: f64,
    stride: f64,
    axe: bool,
    stole: bool,
    way: f64,
}

/// Stack parts, base point, chop flag, riding flag.
type Parts = (Vec<(Vec2, f64)>, Vec2, bool, bool);

#[derive(Clone, Debug)]
struct Target {
    parts: Vec<(Vec2, f64)>,
    x: f64,
    on_floor: bool,
    chop: bool,
    riding: bool,
}

#[derive(Clone, Debug)]
struct Run {
    screen: Frame,
    way: f64,
    x: f64,
    bottom: f64,
    phase: Phase,
    since: f64,
    spot: f64,
    thrown: u32,
    chops: u32,
    stole: bool,
    cheer: bool,
    chasing: bool,
    from_left: bool,
    mode: Mode,
    stride: f64,
    threw_at: f64,
    speed: f64,
    prev_x: Option<f64>,
    sent: bool,
    axe: bool,
}

#[derive(Clone, Debug)]
struct Ball {
    id: u64,
    pos: Vec2,
    vel: Vec2,
    spin: f64,
    low: bool,
    screen: Frame,
}

/// Snowman stack parts as `(centre, radius)`, bottom to top.
pub(crate) type StackParts = Vec<(Vec2, f64)>;

/// Targets offered to the yeti this frame.
pub(crate) struct Targets<'a> {
    pub prey: &'a [(u64, StackParts)],
    pub doomed: Option<(u64, Vec2, f64)>,
    pub riding: Option<StackParts>,
}

/// Bottom-centre point of a stack (foot of its first part).
fn foot(parts: &[(Vec2, f64)]) -> Option<Vec2> {
    parts.first().map(|&(c, r)| Vec2::new(c.x, c.y - r))
}

/// The yeti.
#[derive(Clone, Debug)]
pub struct Yeti {
    left: f64,
    sent: Option<f64>,
    chase: bool,
    run: Option<Run>,
    balls: Vec<Ball>,
    serial: u64,
    look: Look,
    now: f64,
}

impl Default for Yeti {
    fn default() -> Self {
        Self {
            left: f64::INFINITY,
            sent: None,
            chase: false,
            run: None,
            balls: Vec::new(),
            serial: 0,
            look: Look { pose: Pose::Stand, a: 0.0, stride: 0.0, axe: false, stole: false, way: 1.0 },
            now: 0.0,
        }
    }
}

/// Floor y for a yeti or its snowball at `x` (dock ledge when over it).
fn floor(screen: &StageScreen, x: f64) -> f64 {
    match screen.dock {
        Some((a, b, top)) if x >= a - 24.0 && x <= b + 24.0 => top - 1.0,
        _ => screen.frame.min_y() + 1.0,
    }
}

impl Run {
    /// Walk toward `to`; true when within a point of it.
    fn walk(&mut self, to: f64, speed: f64, dt: f64) -> bool {
        let dx = to - self.x;
        if dx.abs() > 1.0 {
            self.way = dx.signum();
        }
        let step = (speed * dt).min(dx.abs());
        self.x += dx.signum() * step;
        self.stride += step;
        (to - self.x).abs() < 1.0
    }

    fn go(&mut self, phase: Phase, now: f64) {
        self.phase = phase;
        self.since = now;
    }

    fn face(&mut self, t: &Target) {
        let dx = t.x - self.x;
        if dx.abs() > 20.0 {
            self.way = dx.signum();
        }
    }

    fn react(&mut self, cheer: bool, now: f64) {
        self.cheer = cheer;
        self.go(Phase::Reacting, now);
    }
}

impl Yeti {
    pub(crate) fn start(&mut self, now: f64, rng: &mut impl Rng) {
        self.left = rng.range(900.0, 2400.0);
        self.now = now;
    }

    pub(crate) fn stop(&mut self) {
        self.left = f64::INFINITY;
        self.sent = None;
        self.chase = false;
        self.run = None;
        self.balls.clear();
    }

    /// Manual send: hunts as soon as a target exists (until the timeout).
    pub fn send(&mut self, now: f64) {
        self.sent = Some(now);
    }

    /// Chase the train: hunt the carried snowman next frame.
    pub(crate) fn chase(&mut self) {
        self.chase = true;
    }

    /// No run is active.
    pub fn idle(&self) -> bool {
        self.run.is_none()
    }

    /// Stack parts, base point, chop and riding flags for a mode.
    fn parts(mode: Mode, t: &Targets<'_>) -> Option<Parts> {
        Some(match mode {
            Mode::Prey(id) => {
                let parts = t.prey.iter().find(|(i, _)| *i == id)?.1.clone();
                let base = foot(&parts)?;
                (parts, base, false, false)
            }
            Mode::Chop(id) => {
                let (_, p, r) = t.doomed.filter(|(i, _, _)| *i == id)?;
                (vec![(p, r)], p, true, false)
            }
            Mode::Riding => {
                let parts = t.riding.clone()?;
                let base = foot(&parts)?;
                (parts, base, false, true)
            }
        })
    }

    fn build(mode: Mode, t: &Targets<'_>, screen: &StageScreen) -> Option<Target> {
        let (parts, base, chop, riding) = Self::parts(mode, t)?;
        let on_floor = chop || riding || (base.y - floor(screen, base.x)).abs() <= 6.0;
        Some(Target { parts, x: base.x, on_floor, chop, riding })
    }

    /// First hunt that can start: mode, whether it is a manual one, screen.
    fn pick<'a>(&self, c: &'a StepCtx<'_>, t: &Targets<'_>, comes: bool) -> Option<(Mode, bool, &'a StageScreen)> {
        let mut modes = Vec::new();
        if self.chase && t.riding.is_some() {
            modes.push((Mode::Riding, true));
        }
        if let Some((id, _, _)) = t.doomed {
            modes.push((Mode::Chop(id), true));
        }
        if self.sent.is_some() || (self.left <= 0.0 && comes) {
            modes.extend(t.prey.iter().map(|(id, _)| (Mode::Prey(*id), self.sent.is_some())));
        }
        modes.into_iter().find_map(|(mode, sent)| {
            let (parts, base, _, _) = Self::parts(mode, t)?;
            let at = Vec2::new(base.x, parts[0].0.y);
            let screen = c.shown_screens.iter().find(|s| s.shown && s.frame.contains(at))?;
            Some((mode, sent, screen))
        })
    }

    pub(crate) fn step(
        &mut self,
        c: &StepCtx<'_>,
        t: &Targets<'_>,
        comes: bool,
        rng: &mut impl Rng,
        out: &mut Vec<CharacterEvent>,
    ) {
        let (now, dt) = (c.now, c.dt.max(0.0));
        self.now = now;
        if self.sent.is_some_and(|at| now - at > SEND_TIMEOUT) && self.run.is_none() {
            self.sent = None;
        }
        if self.run.is_none() {
            if !t.prey.is_empty() {
                self.left -= dt;
            }
            self.begin(c, t, comes);
        }
        if t.riding.is_none() && self.run.is_none() {
            self.chase = false;
        }
        let balls_hit = self.fly(c, t, dt, out);
        let Some(mut run) = self.run.take() else { return };
        let Some(screen) = find_screen(c.shown_screens, run.screen) else {
            self.finish(&run, rng);
            return;
        };
        let target = Self::build(run.mode, t, screen);
        if balls_hit {
            if let Mode::Prey(id) = run.mode {
                out.push(CharacterEvent::Smash { bottom: id });
            }
            run.react(true, now);
        }
        if let Some(tg) = &target {
            if let Some(px) = run.prev_x.filter(|_| dt > 0.0) {
                run.speed += ((tg.x - px) / dt - run.speed) * 0.3;
            }
            run.prev_x = Some(tg.x);
            run.axe = tg.chop;
        }
        let outside = target.as_ref().is_none_or(|tg| {
            let p = tg.parts[0].0;
            !screen.frame.inset(-40.0, -40.0).contains(p)
        });
        if outside && !matches!(run.phase, Phase::Leaving | Phase::Reacting) {
            run.react(false, now);
        }
        let done = self.phases(&mut run, screen, target.as_ref(), c, dt, out);
        run.bottom += (floor(screen, run.x) - run.bottom) * (1.0 - (-14.0 * dt).exp());
        self.look = look_of(&run, now);
        if done {
            self.finish(&run, rng);
        } else {
            self.run = Some(run);
        }
    }

    fn finish(&mut self, run: &Run, rng: &mut impl Rng) {
        if !run.sent {
            self.left = rng.range(3600.0, 10800.0);
        }
    }

    fn begin(&mut self, c: &StepCtx<'_>, t: &Targets<'_>, comes: bool) {
        let Some((mode, sent, screen)) = self.pick(c, t, comes) else { return };
        let Some(target) = Self::build(mode, t, screen) else { return };
        let mid = screen.frame.mid_x();
        let from_left = if target.riding {
            true
        } else if target.on_floor {
            target.x > mid
        } else {
            target.x <= mid
        };
        let edge = if from_left { screen.frame.min_x() } else { screen.frame.max_x() };
        let way = if from_left { 1.0 } else { -1.0 };
        let x = edge - way * OUTSIDE;
        self.sent = None;
        self.chase = false;
        self.run = Some(Run {
            screen: screen.frame,
            way,
            x,
            bottom: floor(screen, x),
            phase: Phase::Entering,
            since: c.now,
            spot: x,
            thrown: 0,
            chops: 0,
            stole: false,
            cheer: false,
            chasing: false,
            from_left,
            mode,
            stride: 0.0,
            threw_at: -1e9,
            speed: 0.0,
            prev_x: None,
            sent: sent || target.chop || target.riding,
            axe: false,
        });
    }

    /// Flying snowballs. Returns whether one hit the target.
    fn fly(&mut self, c: &StepCtx<'_>, t: &Targets<'_>, dt: f64, out: &mut Vec<CharacterEvent>) -> bool {
        let mut hit = false;
        let run_mode = self.run.as_ref().map(|r| (r.mode, r.screen));
        let mut balls = std::mem::take(&mut self.balls);
        balls.retain_mut(|b| {
            b.vel.y -= GRAVITY * dt;
            b.pos = b.pos + b.vel * dt;
            b.spin -= b.vel.x * dt / BALL_R;
            let Some(screen) = find_screen(c.shown_screens, b.screen) else { return false };
            let f = screen.frame;
            if b.pos.x < f.min_x() - 30.0 || b.pos.x > f.max_x() + 30.0 {
                return false;
            }
            let target = run_mode.filter(|(_, s)| *s == b.screen).and_then(|(m, _)| Self::build(m, t, screen));
            let direct = !hit
                && target
                    .as_ref()
                    .is_some_and(|tg| !tg.riding && tg.parts.iter().any(|(p, r)| p.dist(b.pos) < r + 5.6));
            let ground = b.pos.y - BALL_R <= if b.low { f.min_y() + 1.0 } else { floor(screen, b.pos.x) };
            if direct || ground {
                out.push(CharacterEvent::Burst { at: b.pos, area: SPLAT_SNOW, velocity: b.vel });
            }
            hit |= direct;
            !(direct || ground)
        });
        self.balls = balls;
        hit
    }

    /// Phase machine. Returns true when the run is over.
    fn phases(
        &mut self,
        run: &mut Run,
        screen: &StageScreen,
        target: Option<&Target>,
        c: &StepCtx<'_>,
        dt: f64,
        out: &mut Vec<CharacterEvent>,
    ) -> bool {
        let now = c.now;
        let held = now - run.since;
        let frame = screen.frame;
        match run.phase {
            Phase::Entering => {
                let edge = if run.from_left { frame.min_x() } else { frame.max_x() };
                let way = if run.from_left { 1.0 } else { -1.0 };
                if run.walk(edge + way * PEEK_IN, DASH, dt) {
                    run.go(Phase::Peeking, now);
                }
            }
            Phase::Peeking => {
                if let Some(tg) = target.filter(|_| held >= PEEK_TIME) {
                    if tg.on_floor {
                        run.go(Phase::Dashing, now);
                    } else {
                        run.spot = run.x + run.way * WALK_IN;
                        run.go(Phase::Walking, now);
                    }
                }
            }
            Phase::Walking => {
                if run.walk(run.spot, WALK, dt) {
                    run.go(Phase::Scooping, now);
                }
            }
            Phase::Dashing => target.iter().for_each(|tg| dash(run, tg, (now, dt, held), out)),
            Phase::Scooping | Phase::Aiming | Phase::Waiting => {
                if let Some(tg) = target {
                    run.face(tg);
                    self.aim_and_throw(run, tg, c, held);
                }
            }
            Phase::WindingUp => {
                if held >= WIND_UP_TIME {
                    if let Mode::Chop(tree) = run.mode {
                        out.push(CharacterEvent::Chop { tree });
                    }
                    run.chops += 1;
                    run.threw_at = now;
                    if run.chops >= CHOPS {
                        run.react(true, now);
                    } else {
                        run.go(Phase::Resting, now);
                    }
                }
            }
            Phase::Resting => {
                if held >= REST_TIME {
                    run.go(Phase::WindingUp, now);
                }
            }
            Phase::Reacting => {
                if held >= REACT_TIME {
                    run.go(Phase::Leaving, now);
                }
            }
            Phase::Leaving => {
                let to = if run.chasing || run.x - frame.min_x() < frame.max_x() - run.x {
                    frame.min_x() - OUTSIDE
                } else {
                    frame.max_x() + OUTSIDE
                };
                return run.walk(to, if run.stole { DASH } else { WALK }, dt);
            }
        }
        false
    }

    /// Scooping, aiming, waiting and the throws.
    fn aim_and_throw(&mut self, run: &mut Run, tg: &Target, c: &StepCtx<'_>, held: f64) {
        let now = c.now;
        match run.phase {
            Phase::Scooping => {
                if held >= SCOOP_TIME {
                    run.go(Phase::Aiming, now);
                }
            }
            Phase::Aiming => {
                if held >= AIM_TIME {
                    run.thrown += 1;
                    run.threw_at = now;
                    self.throw(run, tg);
                    run.go(Phase::Waiting, now);
                }
            }
            _ => {
                if run.thrown < SHOTS {
                    if held >= RELOAD_TIME {
                        run.go(Phase::Scooping, now);
                    }
                } else if self.balls.is_empty() {
                    run.react(run.cheer, now);
                }
            }
        }
    }

    fn throw(&mut self, run: &Run, tg: &Target) {
        let hand = Vec2::new(
            run.x + run.way * 0.66 * (54.0 - 32.0 + 2.4f64.sin() * 21.0),
            run.bottom + 0.66 * (24.0 + 4.0 - 2.4f64.cos() * 21.0),
        );
        let vel = if run.thrown == 1 {
            let (top, r) = tg.parts[tg.parts.len() - 1];
            let apex = (top.y + r + 8.0 + CLEARANCE).max(hand.y + LOWEST_ARC);
            let vy = (2.0 * GRAVITY * (apex - hand.y)).sqrt();
            let t = vy / GRAVITY;
            let lead = if tg.riding { run.speed * t } else { 0.0 };
            Vec2::new((top.x + lead - hand.x) / t, vy)
        } else {
            let (mut aim, r) = tg.parts[tg.parts.len() / 2];
            let flight = if tg.riding { 1.0 } else { (aim.dist(hand) / 820.0).clamp(0.7, 1.2) };
            if tg.riding {
                aim.x += run.speed * flight - (aim.x - hand.x).signum() * (r + 8.0 + 6.0);
            }
            (aim - hand) * (1.0 / flight) + Vec2::new(0.0, GRAVITY * flight / 2.0)
        };
        self.serial += 1;
        self.balls.push(Ball { id: self.serial, pos: hand, vel, spin: 0.0, low: tg.riding, screen: run.screen });
    }

    pub(crate) fn sprites(&self, rev: u64, shown: &[StageScreen], out: &mut Vec<Sprite>) {
        if let Some(r) = &self.run {
            let alpha = find_screen(shown, r.screen).map_or(0.0, |s| if s.shown { 1.0 } else { 0.0 });
            let frame = Frame::new(r.x - WIN_W / 2.0, r.bottom - FEET, WIN_W, WIN_H);
            out.push(sprite(Owner::Character, 3, frame, (ZBand::Front, HitMode::Never), rev, alpha));
        }
        for b in &self.balls {
            let alpha = find_screen(shown, b.screen).map_or(0.0, |s| if s.shown { 1.0 } else { 0.0 });
            let frame = Frame::new(b.pos.x - BALL_R, b.pos.y - BALL_R, 2.0 * BALL_R, 2.0 * BALL_R);
            out.push(sprite(Owner::Character, BALL_KEY + b.id, frame, (ZBand::Front, HitMode::Never), rev, alpha));
        }
    }

    pub(crate) fn paint(&self, id: u64, scale: f32) -> Option<Pixmap> {
        if id == 3 {
            self.run.as_ref()?;
            return paint_yeti(&self.look, self.now, scale);
        }
        let b = self.balls.iter().find(|b| BALL_KEY + b.id == id)?;
        let mut pm = canvas(2.0 * BALL_R, 2.0 * BALL_R, scale)?;
        let mut s = Sketch::new(&mut pm, scale);
        s.circle(8.0, 8.0, 8.0, c(0xCFDDF5));
        s.circle(7.0, 9.0, 6.5, c(0xF4F8FF));
        let (sn, cs) = (b.spin.sin() as f32, b.spin.cos() as f32);
        s.circle(8.0 + 4.0 * cs, 8.0 + 4.0 * sn, 1.4, c(0xB6C8EA));
        Some(pm)
    }
}

/// Dashing toward the target: chop range, snowman steal or chase.
fn dash(run: &mut Run, tg: &Target, (now, dt, held): (f64, f64, f64), out: &mut Vec<CharacterEvent>) {
    let near = (tg.x - run.x).abs() <= tg.parts[0].1 + 12.0;
    if tg.chop {
        if near {
            run.face(tg);
            run.go(Phase::WindingUp, now);
        } else {
            run.walk(tg.x, DASH, dt);
        }
    } else if !tg.on_floor {
        run.go(Phase::Scooping, now);
    } else if tg.riding {
        run.chasing = true;
        if held > CHASE_TIME || near {
            run.go(Phase::Scooping, now);
        } else {
            run.walk(tg.x, DASH, dt);
        }
    } else if near {
        run.stole = true;
        if let Mode::Prey(id) = run.mode {
            out.push(CharacterEvent::Smash { bottom: id });
        }
        run.react(true, now);
    } else {
        run.walk(tg.x, DASH, dt);
    }
}

fn look_of(r: &Run, now: f64) -> Look {
    let since = now - r.since;
    let (pose, a) = if now - r.threw_at < 0.3 {
        (Pose::Threw, now - r.threw_at)
    } else {
        match r.phase {
            Phase::Peeking => (Pose::Surprised, since),
            Phase::Entering | Phase::Dashing | Phase::Walking | Phase::Leaving => (Pose::Run, 0.0),
            Phase::Scooping => (Pose::Scoop, since / SCOOP_TIME),
            Phase::Aiming => (Pose::Aim, since / AIM_TIME),
            Phase::Waiting => (Pose::Stand, 0.0),
            Phase::WindingUp => (Pose::Aim, since / WIND_UP_TIME),
            Phase::Resting => (Pose::Sad, 0.0),
            Phase::Reacting => (if r.cheer { Pose::Cheer } else { Pose::Sad }, 0.0),
        }
    };
    Look { pose, a, stride: r.stride, axe: r.axe, stole: r.stole, way: r.way }
}

/// Overshoot pop-in (`0..1` input).
fn pop(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    1.0 + 2.7 * (x - 1.0).powi(3) + 1.7 * (x - 1.0).powi(2)
}

/// Arm angles, lean, trunk squash, hop and carried snowball scale.
struct Stance {
    front: f64,
    back: f64,
    lean: f64,
    squash: f64,
    hop: f64,
    ball: f64,
}

fn stance(l: &Look, t: f64) -> Stance {
    let a = l.a;
    let mut s =
        Stance { front: 0.18, back: -0.18 + 0.04 * (2.0 * t).sin(), lean: 0.0, squash: 1.0, hop: 0.0, ball: 0.0 };
    match l.pose {
        Pose::Stand => s.squash = 1.0 + 0.02 * (3.0 * t).sin(),
        Pose::Run => {
            let k = l.stride / 13.0;
            s.front = 0.8 + 0.7 * k.sin();
            s.back = -0.8 + 0.7 * k.sin();
            s.lean = -0.14 + 0.05 * (k / 2.0).sin();
            s.hop = k.sin().abs() * 2.5;
        }
        Pose::Surprised => {
            (s.front, s.back) = (2.4, -2.4);
            s.hop = if a < 0.3 { (a * 10.5).sin().abs() * 8.0 } else { 0.0 };
        }
        Pose::Scoop => {
            (s.front, s.back, s.squash, s.ball) = (0.85, -0.85, 0.86, a.clamp(0.0, 1.0));
        }
        Pose::Aim => {
            (s.front, s.back, s.lean, s.ball) = (0.85 + 2.6 * ease(0.0, 1.0, a), -0.5, 0.12 * ease(0.0, 1.0, a), 1.0);
        }
        Pose::Threw => {
            (s.front, s.back) = (3.45 - 2.55 * ease(0.0, 1.0, a / 0.14), -0.5);
            s.lean = -0.18 * (1.0 - ease(0.0, 1.0, a / 0.3));
        }
        Pose::Cheer => {
            s.front = 2.7 + 0.25 * (16.0 * t).sin();
            s.back = -2.7 + 0.25 * (16.0 * t).sin();
            s.hop = 9.0 * (9.0 * t).sin().abs();
        }
        Pose::Sad => {
            (s.front, s.back, s.squash) = (0.05, -0.05, 0.95);
            s.lean = (15.0 * t).sin() * 0.07;
        }
    }
    s
}

fn paint_yeti(l: &Look, t: f64, scale: f32) -> Option<Pixmap> {
    let mut pm = canvas(WIN_W, WIN_H, scale)?;
    let mut s = Sketch::new(&mut pm, scale * SCALE as f32);
    let st = stance(l, t);
    s.translate(70.0, 6.0);
    s.scale_by(l.way as f32, 1.0);
    s.rotate(st.lean as f32);
    s.translate(0.0, st.hop as f32);
    let fur = c(0xF5FAFF);
    let outline = c(0x809ED6);
    let mitt = c(0x8FB8FF);
    let k = l.stride / 13.0;
    let swing = if l.pose == Pose::Run { 3.0 * k.sin() as f32 } else { 0.0 };
    for sign in [-1.0f32, 1.0] {
        let lift = if l.pose == Pose::Run { (sign as f64 * k.sin()).max(0.0) as f32 * 5.0 } else { 0.0 };
        s.ellipse(sign * 13.0 + sign * swing - 8.0, lift - 1.0, 16.0, 9.0, mitt);
    }
    s.save();
    s.scale_by(1.0, st.squash as f32);
    // fur body with tufts
    for n in 0..22 {
        let ang = n as f32 * std::f32::consts::TAU / 22.0;
        let r = if n % 2 == 0 { 3.8 } else { 3.1 };
        s.circle(23.5 * ang.cos(), 28.0 + 22.0 * ang.sin(), r, fur);
    }
    for x in [-14.0f32, 0.0, 14.0] {
        s.circle(x, 8.0, 5.0, fur);
    }
    s.ellipse(-23.5, 6.0, 47.0, 44.0, fur);
    s.ring(-23.5, 6.0, 47.0, 44.0, outline, 2.2);
    s.ellipse(-4.0, 22.0, 26.0, 20.0, c(0xC2DEFF));
    s.ellipse(0.0, 24.0, 18.0, 14.0, c(0xA1C7FF));
    s.circle(14.0, 26.0, 3.2, rgba(1.0, 0.56, 0.68, 0.85));
    let ink = c(0x171C3D);
    match l.pose {
        Pose::Cheer => {
            s.polyline(&[(3.0, 33.0), (6.0, 36.0), (9.0, 33.0)], ink, 1.6);
            s.polyline(&[(13.0, 33.0), (16.0, 36.0), (19.0, 33.0)], ink, 1.6);
        }
        Pose::Sad => {
            s.line(2.0, 37.0, 9.0, 35.0, ink, 1.4);
            s.line(19.0, 37.0, 13.0, 35.0, ink, 1.4);
            s.circle(7.0, 31.0, 1.8, ink);
            s.circle(16.0, 31.0, 1.8, ink);
            s.circle(17.5, 27.0, 1.2, c(0x7FB5FF));
        }
        _ => {
            let blink = if t.rem_euclid(2.8) < 0.11 { 0.12 } else { 1.0 };
            for x in [6.0f32, 16.0] {
                s.at(x, 32.0, 0.0, 1.0, |s| {
                    s.save();
                    s.scale_by(1.0, blink);
                    s.circle(0.0, 0.0, 2.4, ink);
                    s.circle(0.7, 0.8, 0.8, c(0xFFFFFF));
                    s.restore();
                });
            }
        }
    }
    s.polyline(&[(9.0, 28.0), (12.0, 29.5), (13.0, 27.0)], c(0x29335E), 1.4);
    s.restore();
    // arms (shoulders front and back)
    for (sx, angle, front) in [(-20.0f32, st.back, false), (22.0, st.front, true)] {
        s.at(sx, 30.0, angle as f32, 1.0, |s| {
            s.line(0.0, 0.0, 0.0, -21.0, outline, 9.0);
            s.line(0.0, 0.0, 0.0, -21.0, fur, 6.6);
            s.circle(0.0, -21.0, 5.5, mitt);
            if front {
                if l.axe {
                    s.line(0.0, -16.0, 0.0, -48.0, c(0x80542E), 3.0);
                    s.polygon(&[(0.0, -44.0), (11.0, -40.0), (11.0, -52.0), (0.0, -50.0)], c(0xB8C4D6));
                    s.line(11.0, -40.0, 11.0, -52.0, c(0xEDF2FA), 1.4);
                } else if l.stole {
                    s.polygon(&[(-3.0, -22.0), (3.0, -22.0), (0.0, -42.0)], c(0xF5801F));
                    s.circle(-2.0, -22.0, 2.5, c(0x4DAD52));
                    s.circle(2.0, -22.0, 2.5, c(0x4DAD52));
                } else if st.ball > 0.0 {
                    s.circle(0.0, -27.0, (11.0 * st.ball) as f32, c(0xE6EEFF));
                }
            }
        });
    }
    if l.pose == Pose::Surprised {
        let p = pop(l.a / 0.16) as f32;
        s.at(4.0, 62.0, 0.0, p.max(0.01), |s| {
            s.line(0.0, 0.0, 0.0, 12.0, c(0xFFFFFF), 8.0);
            s.line(0.0, -7.0, 0.0, -7.0, c(0xFFFFFF), 8.0);
            s.line(0.0, 0.0, 0.0, 12.0, c(0xF54538), 4.5);
            s.circle(0.0, -7.0, 2.6, c(0xF54538));
        });
    }
    Some(pm)
}

#[cfg(test)]
impl Yeti {
    /// Test hook: a bare run.
    fn test_run() -> Run {
        Run {
            screen: Frame::new(0.0, 0.0, 1920.0, 1080.0),
            way: 1.0,
            x: 100.0,
            bottom: 39.0,
            phase: Phase::Waiting,
            since: 0.0,
            spot: 0.0,
            thrown: 0,
            chops: 0,
            stole: false,
            cheer: false,
            chasing: false,
            from_left: true,
            mode: Mode::Prey(1),
            stride: 0.0,
            threw_at: -1e9,
            speed: 0.0,
            prev_x: None,
            sent: false,
            axe: false,
        }
    }

    pub(crate) fn test_look(&mut self, pose_index: usize, a: f64, axe: bool, stole: bool) {
        let poses =
            [Pose::Stand, Pose::Run, Pose::Surprised, Pose::Scoop, Pose::Aim, Pose::Threw, Pose::Cheer, Pose::Sad];
        self.look = Look { pose: poses[pose_index], a, stride: 20.0, axe, stole, way: -1.0 };
        self.run.get_or_insert_with(Self::test_run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::clock::WallTime;
    use fp_core::rng::Lcg64;
    use fp_core::scene::PointerInput;

    struct Bench {
        y: Yeti,
        now: f64,
        scr: Vec<StageScreen>,
        rng: Lcg64,
        out: Vec<CharacterEvent>,
    }

    impl Bench {
        fn new() -> Self {
            let scr = vec![StageScreen {
                frame: Frame::new(0.0, 0.0, 1920.0, 1080.0),
                shown: true,
                dock: Some((0.0, 1920.0, 40.0)),
            }];
            Self { y: Yeti::default(), now: 0.0, scr, rng: Lcg64::new(5), out: Vec::new() }
        }

        fn step(&mut self, dt: f64, t: &Targets<'_>) {
            self.now += dt;
            let ptr = PointerInput::default();
            let ctx = StepCtx {
                now: self.now,
                dt,
                wall: WallTime::default(),
                visit_screens: &self.scr,
                plough_screens: &self.scr,
                shown_screens: &self.scr,
                pointer: &ptr,
            };
            self.y.step(&ctx, t, true, &mut self.rng, &mut self.out);
        }

        fn phase(&self) -> Option<Phase> {
            self.y.run.as_ref().map(|r| r.phase)
        }
    }

    fn mk_ctx<'a>(scr: &'a [StageScreen], ptr: &'a PointerInput, dt: f64) -> StepCtx<'a> {
        StepCtx {
            now: 1.0,
            dt,
            wall: WallTime::default(),
            visit_screens: scr,
            plough_screens: scr,
            shown_screens: scr,
            pointer: ptr,
        }
    }

    fn stack_of(base: Vec2, radii: &[f64]) -> StackParts {
        let mut y = base.y;
        radii
            .iter()
            .map(|&r| {
                let part = (Vec2::new(base.x, y + r), r);
                y += 2.0 * r;
                part
            })
            .collect()
    }

    fn std3(x: f64, y: f64) -> StackParts {
        stack_of(Vec2::new(x, y), &[26.0, 19.0, 13.0])
    }

    fn none() -> Targets<'static> {
        Targets { prey: &[], doomed: None, riding: None }
    }

    #[test]
    fn chop_starts_at_the_tree_reach_radius_plus_twelve() {
        // Big tree: reach 60 > the old fixed 30, so the wind-up begins from
        // |dx| in (42, 72].
        let tree = Some((4u64, Vec2::new(900.0, 150.0), 60.0));
        let mut b = Bench::new();
        let mut dx = None;
        for _ in 0..6000 {
            b.step(1.0 / 60.0, &Targets { prey: &[], doomed: tree, riding: None });
            if b.phase() == Some(Phase::WindingUp) {
                dx = b.y.run.as_ref().map(|r| (900.0 - r.x).abs());
                break;
            }
        }
        let dx = dx.expect("never wound up");
        assert!(dx > 42.0 && dx <= 72.0, "dx = {dx}");
    }

    #[test]
    fn dash_aborts_when_the_snowman_leaves_the_floor() {
        let mut b = Bench::new();
        b.y.send(0.0);
        let floor = [(1u64, std3(500.0, 39.0))];
        let ledge = [(1u64, std3(500.0, 400.0))];
        while b.phase() != Some(Phase::Dashing) {
            b.step(1.0 / 60.0, &Targets { prey: &floor, doomed: None, riding: None });
        }
        b.step(1.0 / 60.0, &Targets { prey: &ledge, doomed: None, riding: None });
        assert_eq!(b.phase(), Some(Phase::Scooping));
    }

    #[test]
    fn riding_chase_gives_up_or_catches_up() {
        for (x0, speed) in [(1500.0, 0.5), (300.0, 0.0)] {
            let mut b = Bench::new();
            b.y.chase();
            let mut x = x0;
            let mut chasing = false;
            let mut painted = false;
            for _ in 0..3000 {
                x += speed;
                let at = Vec2::new(x, 24.0);
                b.step(1.0 / 60.0, &Targets { prey: &[], doomed: None, riding: Some(std3(at.x, at.y)) });
                chasing |= b.y.run.as_ref().is_some_and(|r| r.chasing);
                if !painted && b.phase() == Some(Phase::Aiming) {
                    painted = b.y.paint(3, 1.0).is_some();
                }
                if chasing && b.y.idle() && b.y.balls.is_empty() {
                    break;
                }
            }
            assert!(chasing && b.y.idle());
            assert!(painted);
        }
    }

    #[test]
    fn balls_fly_off_land_or_hit() {
        let mut b = Bench::new();
        b.y.run = Some(Yeti::test_run());
        let frame = b.scr[0].frame;
        let ball = |x: f64, y: f64, low: bool, screen: Frame| Ball {
            id: 0,
            pos: Vec2::new(x, y),
            vel: Vec2::ZERO,
            spin: 0.0,
            low,
            screen,
        };
        b.y.balls = vec![
            ball(-100.0, 500.0, false, frame),
            ball(1000.0, 500.0, false, Frame::new(9.0, 9.0, 9.0, 9.0)),
            ball(1000.0, 45.0, false, frame),
            ball(1100.0, 5.0, true, frame),
            ball(1500.0, 300.0, false, frame),
        ];
        let prey = [(1u64, std3(1500.0, 200.0))];
        let t = Targets { prey: &prey, doomed: None, riding: None };
        let mut out = Vec::new();
        let ptr = PointerInput::default();
        let ctx = mk_ctx(&b.scr, &ptr, 1.0 / 60.0);
        let hit = b.y.fly(&ctx, &t, 1.0 / 60.0, &mut out);
        assert!(hit);
        assert_eq!(out.len(), 3, "two ground splats and one hit");
        assert!(b.y.balls.is_empty());
        // a riding target is never hit directly
        let riding = Targets { prey: &[], doomed: None, riding: Some(std3(1500.0, 200.0)) };
        b.y.run.as_mut().unwrap().mode = Mode::Riding;
        b.y.balls = vec![ball(1500.0, 300.0, false, frame)];
        let ctx = mk_ctx(&b.scr, &ptr, 1.0 / 60.0);
        assert!(!b.y.fly(&ctx, &riding, 1.0 / 60.0, &mut out));
        assert_eq!(b.y.balls.len(), 1);
    }

    #[test]
    fn automatic_hunt_ends_and_waits_again() {
        let mut b = Bench::new();
        b.y.start(0.0, &mut Lcg64::new(2));
        let prey = [(3u64, std3(800.0, 39.0))];
        let t = Targets { prey: &prey, doomed: None, riding: None };
        b.step(3000.0, &t);
        let mut started = false;
        for _ in 0..3000 {
            b.step(1.0 / 60.0, &t);
            started |= b.y.run.is_some();
            if started && b.y.idle() {
                break;
            }
        }
        assert!(b.y.idle() && b.y.left >= 3600.0 - 3000.0);
        b.step(1.0 / 60.0, &none());
    }

    #[test]
    fn real_radii_drive_reach_apex_aim_and_hit() {
        // four balls, none of the default radii
        let radii = [60.0, 45.0, 35.0, 25.0];
        let parts = stack_of(Vec2::new(900.0, 39.0), &radii);
        let prey = [(7u64, parts.clone())];
        let t = Targets { prey: &prey, doomed: None, riding: None };
        let mut b = Bench::new();
        b.y.send(0.0);
        while b.phase() != Some(Phase::Dashing) {
            b.step(1.0 / 60.0, &t);
        }
        let screen = b.scr[0];
        let tg = Yeti::build(Mode::Prey(7), &t, &screen).expect("target");
        assert_eq!(tg.parts, parts);
        assert_eq!(tg.x, 900.0);
        // dash reach is the real bottom radius + 12, not the assumed 26 + 12
        let run = b.y.run.as_mut().unwrap();
        run.x = 900.0 - 71.0;
        let mut out = Vec::new();
        dash(run, &tg, (1.0, 0.0, 0.0), &mut out);
        assert!(out.contains(&CharacterEvent::Smash { bottom: 7 }), "within 60 + 12");
        // lob apex: top.y + top.r + 30
        let mut y = Yeti::default();
        let mut run = Yeti::test_run();
        run.thrown = 1;
        run.x = 100.0;
        y.throw(&run, &tg);
        let hand = y.balls[0].pos;
        let top = parts[3];
        let apex = (top.0.y + top.1 + 30.0).max(hand.y + LOWEST_ARC);
        let vy = y.balls[0].vel.y;
        assert!((hand.y + vy * vy / (2.0 * GRAVITY) - apex).abs() < 1e-6);
        assert!(apex > hand.y + LOWEST_ARC, "apex follows the real top");
        // throw 2 aims at parts[len / 2]
        run.thrown = 2;
        y.balls.clear();
        y.throw(&run, &tg);
        let (aim, _) = parts[2];
        let flight = (aim.dist(hand) / 820.0).clamp(0.7, 1.2);
        let want = (aim - hand) * (1.0 / flight) + Vec2::new(0.0, GRAVITY * flight / 2.0);
        assert!(y.balls[0].vel.dist(want) < 1e-9);
        // hit test uses every real part: inside the 4th ball, outside all
        let ptr = PointerInput::default();
        let ctx = mk_ctx(&b.scr, &ptr, 1.0 / 60.0);
        let ball = |at: Vec2| Ball { id: 0, pos: at, vel: Vec2::ZERO, spin: 0.0, low: false, screen: screen.frame };
        let mut hits = Vec::new();
        for at in [Vec2::new(900.0, parts[3].0.y + 25.0 + 5.0), Vec2::new(900.0, parts[3].0.y + 25.0 + 6.5)] {
            let mut yy = b.y.clone();
            yy.run.as_mut().unwrap().mode = Mode::Prey(7);
            yy.balls = vec![ball(at)];
            let mut o = Vec::new();
            hits.push(yy.fly(&ctx, &t, 0.0, &mut o));
        }
        assert_eq!(hits, [true, false], "top radius 25 + 5.6, not the fake 13");
    }

    #[test]
    fn every_pose_paints() {
        let mut y = Yeti::default();
        assert!(y.paint(3, 1.0).is_none());
        for pose in 0..8 {
            for (axe, stole) in [(false, false), (true, false), (false, true)] {
                for a in [0.0, 0.05, 0.2, 0.9] {
                    y.test_look(pose, a, axe, stole);
                    let pm = y.paint(3, 1.5).expect("art");
                    assert_eq!(pm.width(), (WIN_W * 1.5).ceil() as u32);
                }
            }
        }
        y.now = 0.05;
        y.test_look(0, 0.0, false, false);
        assert!(y.paint(3, 1.0).is_some(), "blink frame");
        assert!(y.paint(99, 1.0).is_none());
        assert!(pop(2.0) == 1.0 && pop(0.0) < 0.0 + 1.0);
    }
}
