//! Fight runtime (snowman-fight 4) and pose maths (5.1).

use crate::FightEvent;
use crate::story::{ALERT, Aim, Emote, Face, Gait, Move, React, SHOCK, Story, Style, jump_fe, shake_fe, walk};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng};
use std::f64::consts::PI;

/// Size of the snow burst a death publishes (6).
pub(crate) const BURST_AREA: f64 = 380.0;
/// Plough sight distance (4.7).
pub(crate) const PLOUGH_SIGHT: f64 = 420.0;
/// Rig height at scale 1.
pub(crate) const RIG_HEIGHT: f64 = 58.0;
/// Half collision width at size 1.
pub(crate) const HALF_WIDTH: f64 = 35.0;
/// Panel margin and height (2.4).
pub(crate) const PANEL_MARGIN: f64 = 60.0;
pub(crate) const PANEL_HEIGHT: f64 = 180.0;
/// Particle pool size.
pub(crate) const MAX_SPECKS: usize = 56;
const LINGER: f64 = 1.3;
const FADE: f64 = 0.6;
const EDGE: f64 = 24.0;

/// Smoothstep.
pub(crate) fn ease(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Sine arc 0 -> 1 -> 0.
pub(crate) fn arc(x: f64) -> f64 {
    (PI * x.clamp(0.0, 1.0)).sin()
}

pub(crate) fn mix(a: f64, b: f64, p: f64) -> f64 {
    a + (b - a) * p.clamp(0.0, 1.0)
}

/// How a ball ends (4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Land {
    Head,
    Wall,
    Drop,
    Plain,
}

#[derive(Clone, Debug)]
pub(crate) struct Flight {
    pub start: Vec2,
    pub through: Vec2,
    pub end: Vec2,
    pub age: f64,
    pub time: f64,
    pub target: Option<usize>,
    pub land: Land,
    /// Sprite scale factor.
    pub size: f64,
}

impl Flight {
    pub fn progress(&self) -> f64 {
        (self.age / self.time).clamp(0.0, 1.0)
    }

    /// Fraction of the flight at which the apex point sits.
    pub fn apex(&self) -> f64 {
        let d = self.end.x - self.start.x;
        if d.abs() < 1e-9 { 0.5 } else { (self.through.x - self.start.x) / d }
    }

    /// Linear in x, quadratic through the three points in y (4.1).
    pub fn pos(&self) -> Vec2 {
        let p = self.progress();
        let (a, b, c) = (self.start, self.through, self.end);
        let x = a.x + (c.x - a.x) * p;
        let dens = [(a.x - b.x) * (a.x - c.x), (b.x - a.x) * (b.x - c.x), (c.x - a.x) * (c.x - b.x)];
        let y = if dens.iter().any(|d| d.abs() < 1e-6) {
            a.y + (c.y - a.y) * p
        } else {
            a.y * (x - b.x) * (x - c.x) / dens[0]
                + b.y * (x - a.x) * (x - c.x) / dens[1]
                + c.y * (x - a.x) * (x - b.x) / dens[2]
        };
        Vec2::new(x, y)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Speck {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    pub size: f64,
    pub life: f64,
    pub total: f64,
    pub sweat: bool,
}

/// The one tossed flag (4.5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Toss {
    pub x: f64,
    pub y: f64,
    pub dir: f64,
    pub age: f64,
}

const TOSS_TIME: f64 = 0.7;

impl Toss {
    pub fn t(&self) -> f64 {
        self.age / TOSS_TIME
    }
    pub fn pos(&self) -> Vec2 {
        let t = self.t();
        Vec2::new(self.x + self.dir * 55.0 * t, self.y * (1.0 - t) + arc(t) * 55.0)
    }
}

/// One fighter.
#[derive(Clone, Debug)]
pub(crate) struct Rig {
    pub x: f64,
    pub size: f64,
    pub pace: f64,
    pub last_way: f64,
    pub moves: Vec<Move>,
    pub idx: usize,
    pub clock: f64,
    pub begun: bool,
    pub gone: bool,
    pub turned: bool,
    pub holding: bool,
    pub flag: bool,
    pub targeted: bool,
    pub hat_from: Option<f64>,
    pub pile_at: Option<f64>,
    pub worn_by: Option<usize>,
    pub walked: f64,
    pub scoop_time: f64,
    pub wind_time: f64,
    pub flair: f64,
    pub style: Style,
    pub from: f64,
    pub sweat_at: f64,
}

impl Rig {
    fn new(x: f64, way: f64, size: f64, pace: f64, moves: Vec<Move>) -> Rig {
        Rig {
            x,
            size,
            pace,
            last_way: way,
            moves,
            idx: 0,
            clock: 0.0,
            begun: false,
            gone: false,
            turned: false,
            holding: false,
            flag: false,
            targeted: false,
            hat_from: None,
            pile_at: None,
            worn_by: None,
            walked: 0.0,
            scoop_time: 0.42,
            wind_time: 0.34,
            flair: 1.0,
            style: Style::Overhand,
            from: x,
            sweat_at: -1.0,
        }
    }

    pub fn current(&self) -> Option<&Move> {
        self.moves.get(self.idx)
    }

    pub fn offstage(&self) -> bool {
        matches!(self.current(), Some(Move::Offstage(_)))
    }

    pub fn visible(&self) -> bool {
        !self.gone && !self.offstage()
    }

    fn replace_moves(&mut self, moves: Vec<Move>) {
        self.moves = moves;
        self.idx = 0;
        self.clock = 0.0;
        self.begun = false;
    }
}

/// What a fighter looks like this frame (4, 5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub x: f64,
    pub way: f64,
    pub lift: f64,
    pub lean: f64,
    pub squash: f64,
    pub wide: Option<f64>,
    pub front: f64,
    pub back: f64,
    pub face: Face,
    pub emote: Emote,
    pub bubble: f64,
    pub holding: bool,
    pub flag: bool,
    pub hat: bool,
    pub visible: bool,
}

/// One running fight.
pub(crate) struct Bout {
    pub rng: Lcg64,
    acts: Vec<[Vec<Move>; 2]>,
    act: usize,
    pub age: f64,
    pub ended_at: Option<f64>,
    panicked: bool,
    pub f: [Rig; 2],
    pub flights: Vec<Flight>,
    pub specks: Vec<Option<Speck>>,
    next_speck: usize,
    pub toss: Option<Toss>,
    pub wall_x: f64,
    pub wall_height: f64,
    pub scale: f64,
    pub floor: f64,
    pub min: f64,
    pub max: f64,
    pub panel: Frame,
}

fn cheer_pose(p: &mut Pose, c: f64, s: f64) {
    p.face = Face::Happy;
    p.lift = (14.0 * c).sin().abs() * 10.0 * s;
    arms(p, -0.6 + 0.25 * (24.0 * c).sin());
}

fn arms(p: &mut Pose, front: f64) {
    p.front = front;
    p.back = -front;
}

fn crouch_pose(p: &mut Pose) {
    p.squash = 0.5;
    p.wide = Some(1.04);
    arms(p, -0.9);
    p.face = SHOCK;
}

/// Fixed duration of the moves that run for a set time (3.1).
fn span(r: &Rig, mv: &Move) -> f64 {
    match mv {
        Move::Offstage(t)
        | Move::Wait { t, .. }
        | Move::Lie(t)
        | Move::Stagger(t)
        | Move::Cheer(t)
        | Move::Dance(t)
        | Move::Taunt(t)
        | Move::Mourn(t)
        | Move::Wave(t)
        | Move::Handshake(t)
        | Move::Hide(t)
        | Move::Peek(t) => *t,
        Move::Rise => 0.45,
        Move::Jump { .. } => 0.3,
        Move::Release(_) => 0.26,
        Move::DropBall | Move::Shove => 0.3,
        Move::GetUp | Move::Dive => 0.38,
        Move::FlagOut | Move::FlagAway => 0.25,
        Move::WindUp(_) => r.wind_time,
        _ => 0.0,
    }
}

impl Bout {
    /// Builds a bout from a story (2.4): rolls size factor and pace per
    /// fighter and loads act 0.
    pub fn new(story: Story, mut rng: Lcg64, min: f64, max: f64, scale: f64, floor: f64) -> Bout {
        let acts = story.acts;
        let [m0, m1] = acts[0].clone();
        let mk = |rng: &mut Lcg64, i: usize, moves: Vec<Move>| {
            let size = rng.range(0.9, 1.08) * scale;
            let pace = rng.range(0.9, 1.12);
            Rig::new(story.spawn[i], if i == 0 { 1.0 } else { -1.0 }, size, pace, moves)
        };
        let a = mk(&mut rng, 0, m0);
        let b = mk(&mut rng, 1, m1);
        Bout {
            rng,
            acts,
            act: 0,
            age: 0.0,
            ended_at: None,
            panicked: false,
            f: [a, b],
            flights: Vec::new(),
            specks: vec![None; MAX_SPECKS],
            next_speck: 0,
            toss: None,
            wall_x: 0.0,
            wall_height: 0.0,
            scale,
            floor,
            min,
            max,
            panel: Frame::new(min - PANEL_MARGIN, floor, max - min + 2.0 * PANEL_MARGIN, PANEL_HEIGHT),
        }
    }

    /// 0 until the end linger is over, then eases 0 -> 1 (4).
    pub fn fading(&self) -> f64 {
        self.ended_at.map_or(0.0, |e| ease((self.age - e - LINGER) / FADE))
    }

    pub fn finished(&self) -> bool {
        self.ended_at.is_some_and(|e| self.age > e + LINGER + FADE)
    }

    /// Standing fighters as `(x, half width, height)` (4).
    pub fn obstacles(&self) -> Vec<(f64, f64, f64)> {
        self.f.iter().filter(|r| r.visible()).map(|r| (r.x, HALF_WIDTH * r.size, RIG_HEIGHT * r.size)).collect()
    }

    // ---------------------------------------------------------------- step

    /// One frame: plough panic check, then advance (2.3, 4).
    pub fn step(&mut self, dt: f64, plough_x: Option<f64>, out: &mut Vec<FightEvent>) {
        if let Some(front) = plough_x
            && !self.panicked
            && self.ended_at.is_none()
            && self.f.iter().any(|r| !r.gone && r.hat_from.is_none() && (r.x - front).abs() < PLOUGH_SIGHT)
        {
            self.panic(front);
        }
        self.advance(dt, out);
    }

    fn advance(&mut self, dt: f64, out: &mut Vec<FightEvent>) {
        self.age += dt;
        self.update_specks(dt);
        if let Some(t) = &mut self.toss {
            t.age += dt;
        }
        if self.toss.is_some_and(|t| t.age >= TOSS_TIME) {
            self.toss = None;
        }
        for fl in &mut self.flights {
            fl.age += dt;
        }
        let (done, keep): (Vec<Flight>, Vec<Flight>) =
            std::mem::take(&mut self.flights).into_iter().partition(|fl| fl.age >= fl.time);
        self.flights = keep;
        for fl in done {
            self.land(&fl);
        }
        for i in 0..2 {
            self.run(i, dt, out);
        }
        if self.ended_at.is_none() && self.f.iter().all(|r| r.idx >= r.moves.len()) {
            self.act += 1;
            if let Some(next) = self.acts.get(self.act) {
                let [m0, m1] = next.clone();
                self.f[0].replace_moves(m0);
                self.f[1].replace_moves(m1);
            } else {
                self.ended_at = Some(self.age);
            }
        }
        for i in 0..2 {
            let dx = self.f[1 - i].x - self.f[i].x;
            if dx.abs() > 1.0 {
                self.f[i].last_way = dx.signum();
            }
        }
    }

    fn update_specks(&mut self, dt: f64) {
        for slot in &mut self.specks {
            if let Some(s) = slot {
                s.life -= dt;
                s.dy -= 420.0 * dt;
                s.x += s.dx * dt;
                s.y = (s.y + s.dy * dt).max(1.0);
                if s.life <= 0.0 {
                    *slot = None;
                }
            }
        }
    }

    fn land(&mut self, fl: &Flight) {
        let (x, y) = (fl.end.x, fl.end.y);
        match fl.land {
            Land::Head => self.puff(x, y, 14, 110.0),
            Land::Wall => {
                self.wall_height = if self.wall_height > 0.6 { 0.5 } else { 0.0 };
                if self.wall_height > 0.0 {
                    self.puff(x, y, 10, 80.0);
                } else {
                    self.puff(x, y, 20, 130.0);
                }
            }
            Land::Drop => self.puff(x, 1.0, 4, 40.0),
            Land::Plain => self.puff(x, y, 8, 70.0),
        }
    }

    fn push_speck(&mut self, s: Speck) {
        self.specks[self.next_speck] = Some(s);
        self.next_speck = (self.next_speck + 1) % MAX_SPECKS;
    }

    /// Upward fan of white specks (5, particles).
    pub fn puff(&mut self, x: f64, y: f64, count: usize, speed: f64) {
        for _ in 0..count {
            let angle = self.rng.range(0.15, PI - 0.15);
            let v = self.rng.range(0.3, 1.0) * speed;
            let size = self.rng.range(2.0, 4.5);
            let life = self.rng.range(0.3, 0.65);
            self.push_speck(Speck {
                x,
                y,
                dx: angle.cos() * v,
                dy: angle.sin() * v,
                size,
                life,
                total: life,
                sweat: false,
            });
        }
    }

    fn sweat(&mut self, i: usize) {
        if self.age - self.f[i].sweat_at < 0.1 {
            return;
        }
        self.f[i].sweat_at = self.age;
        let (size, x) = (self.f[i].size, self.f[i].x);
        let way = -self.pose(i).way;
        let dx = way * self.rng.range(30.0, 80.0);
        let dy = self.rng.range(50.0, 110.0);
        let sz = self.rng.range(2.0, 3.0);
        self.push_speck(Speck {
            x: x + way * 7.0 * size,
            y: 46.0 * size,
            dx,
            dy,
            size: sz,
            life: 0.35,
            total: 0.35,
            sweat: true,
        });
    }

    // --------------------------------------------------------------- moves

    fn run(&mut self, i: usize, dt: f64, out: &mut Vec<FightEvent>) {
        let Some(mv) = self.f[i].current().cloned() else {
            return;
        };
        if !self.f[i].begun {
            self.f[i].begun = true;
            self.f[i].clock = 0.0;
            self.begin_move(i, &mv);
        }
        self.f[i].clock += dt;
        let done = self.update_move(i, &mv, dt, out);
        if done {
            if mv == Move::Dive {
                self.f[i].gone = true;
            }
            let r = &mut self.f[i];
            r.idx += 1;
            r.clock = 0.0;
            r.begun = false;
        }
    }

    fn begin_move(&mut self, i: usize, mv: &Move) {
        let x = self.f[i].x;
        match mv {
            Move::Rise => self.puff(x, 3.0, 8, 60.0),
            Move::Scoop => self.f[i].scoop_time = self.rng.range(0.36, 0.5),
            Move::WindUp(_) => {
                self.f[i].wind_time = self.rng.range(0.26, 0.42);
                self.f[i].holding = true;
                self.f[i].style = Style::ALL[self.rng.below(5) as usize];
            }
            Move::Release(aim) => {
                self.f[i].holding = false;
                self.launch(i, *aim);
            }
            Move::Brace(_) => self.f[i].flair = self.rng.range(0.85, 1.15),
            Move::DropBall => {
                self.f[i].holding = false;
                self.drop_ball(i);
            }
            Move::Stumble(_) => {
                self.f[i].from = x;
                self.puff(x, 8.0, 8, 70.0);
            }
            Move::Turn(away) => self.f[i].turned = *away,
            Move::Build { at, .. } => {
                self.wall_x = *at;
                self.wall_height = 0.0;
            }
            Move::Crumble => self.f[i].hat_from = Some(self.age),
            Move::Dive => self.puff(x, 3.0, 10, 80.0),
            Move::FlagOut => self.f[i].flag = true,
            Move::FlagAway => {
                self.f[i].flag = false;
                let (way, size) = (self.pose(i).way, self.f[i].size);
                self.toss = Some(Toss { x: x + way * 14.0 * size, y: 40.0 * size, dir: -way, age: 0.0 });
            }
            _ => {}
        }
    }

    /// Runs one frame of the current move; true when it ended.
    fn update_move(&mut self, i: usize, mv: &Move, dt: f64, out: &mut Vec<FightEvent>) -> bool {
        let c = self.f[i].clock;
        match mv {
            Move::Walk { to, gait, .. } => {
                let dist = to - self.f[i].x;
                let step = dist.abs().min(gait.speed() * self.f[i].pace * dt);
                self.f[i].x += dist.signum() * step;
                self.f[i].walked += step;
                if *gait == Gait::Run {
                    self.sweat(i);
                }
                (to - self.f[i].x).abs() < 0.5
            }
            Move::Brace(_) => {
                let free = !self.flights.iter().any(|fl| fl.target == Some(i));
                let over = self.f[i].targeted && free;
                if over {
                    self.f[i].targeted = false;
                }
                over
            }
            Move::Scoop => {
                if c > 0.52 * self.f[i].scoop_time {
                    self.f[i].holding = true;
                }
                c >= self.f[i].scoop_time
            }
            Move::Stumble(to) => {
                self.f[i].x = mix(self.f[i].from, *to, ease(c / 0.38));
                c >= 0.42
            }
            Move::Pant(t) => {
                self.sweat(i);
                c >= *t
            }
            Move::Build { t, .. } => {
                self.wall_height = (c / t).clamp(0.0, 1.0);
                if c % 0.25 < dt {
                    self.puff(self.wall_x, 4.0, 3, 50.0);
                }
                c >= *t
            }
            Move::GrabHat => {
                if c > 0.25
                    && let Some(j) = (0..2).find(|j| self.f[*j].pile_at.is_some())
                {
                    self.f[j].worn_by = Some(i);
                }
                c >= 0.5
            }
            Move::Crumble => {
                let dead = c >= 0.5;
                if dead {
                    self.die(i, out);
                }
                dead
            }
            _ => c >= span(&self.f[i], mv),
        }
    }

    fn die(&mut self, i: usize, out: &mut Vec<FightEvent>) {
        let r = &mut self.f[i];
        r.pile_at = Some(self.age);
        r.gone = true;
        r.holding = false;
        r.flag = false;
        let (x, size) = (r.x, r.size);
        self.puff(x, 22.0 * size, 26, 150.0);
        out.push(FightEvent::Burst { at: Vec2::new(x, self.floor + 24.0 * size), area: BURST_AREA });
    }

    // ------------------------------------------------------------- throws

    fn launch(&mut self, i: usize, aim: Aim) {
        let (me, op) = (&self.f[i], &self.f[1 - i]);
        let dir = if op.x >= me.x { 1.0 } else { -1.0 };
        let style = me.style.info();
        let (size, osz, ox, mx) = (me.size, op.size, op.x, me.x);
        let start = Vec2::new(mx + dir * 14.0 * size, style.hand * size);
        let mid = |x: f64| (start.x + x) / 2.0;
        let dx = (ox - start.x).abs();
        let r = &mut self.rng;
        let mut target = Some(1 - i);
        let (through, end, land) = match aim {
            Aim::Head => {
                let end = Vec2::new(ox - dir * 7.0 * osz, 40.0 * osz);
                (Vec2::new(mid(end.x), 40.0 * size + dx * style.arc), end, Land::Head)
            }
            Aim::Over => {
                let end = Vec2::new(ox + dir * r.range(35.0, 75.0), 0.0);
                (Vec2::new(ox, r.range(44.0, 54.0) * osz), end, Land::Plain)
            }
            Aim::Under => {
                let end = Vec2::new(ox + dir * r.range(24.0, 50.0), 0.0);
                (Vec2::new(ox, r.range(8.0, 13.0) * osz), end, Land::Plain)
            }
            Aim::Low => {
                let end = Vec2::new(ox + dir * r.range(40.0, 70.0), 0.0);
                (Vec2::new(ox, r.range(34.0, 40.0) * osz), end, Land::Plain)
            }
            Aim::Wide => {
                let end = Vec2::new(ox + dir * r.range(70.0, 110.0), 0.0);
                (Vec2::new(ox, r.range(76.0, 92.0) * osz), end, Land::Plain)
            }
            Aim::Shy => {
                let end = Vec2::new(ox - dir * r.range(16.0, 30.0), 0.0);
                (Vec2::new(mid(end.x), 42.0 * size + dx * style.arc * 0.7), end, Land::Plain)
            }
            Aim::Short => {
                target = None;
                let end = Vec2::new(mx + dir * (ox - mx).abs() * 0.5, 0.0);
                (Vec2::new(mid(end.x), 42.0 * size + 16.0), end, Land::Plain)
            }
            Aim::Wall => {
                let end = Vec2::new(self.wall_x, 20.0 * self.scale);
                (Vec2::new(mid(end.x), 40.0 * size + (self.wall_x - start.x).abs() * 0.2), end, Land::Wall)
            }
        };
        let time = ((end.x - start.x).abs() / (430.0 * style.speed * r.range(0.92, 1.08))).max(0.05);
        let sprite = self.scale * r.range(0.9, 1.2);
        self.flights.push(Flight { start, through, end, age: 0.0, time, target, land, size: sprite });
        if let Some(t) = target {
            self.f[t].targeted = true;
        }
    }

    fn drop_ball(&mut self, i: usize) {
        let (way, size, x) = (self.pose(i).way, self.f[i].size, self.f[i].x);
        let start = Vec2::new(x + way * 15.0 * size, 24.0 * size);
        let through = Vec2::new(start.x + way * 3.0, start.y - 8.0 * size);
        let end = Vec2::new(start.x + way * 8.0, 0.0);
        self.flights.push(Flight {
            start,
            through,
            end,
            age: 0.0,
            time: 0.24,
            target: None,
            land: Land::Drop,
            // size factor 1 (not randomised) on the bout scale (4.3)
            size: self.scale,
        });
    }

    // -------------------------------------------------------------- panic

    fn panic(&mut self, front: f64) {
        self.panicked = true;
        self.act = self.acts.len();
        self.wall_height = 0.0;
        for i in 0..2 {
            if self.f[i].gone || self.f[i].hat_from.is_some() {
                continue;
            }
            if self.f[i].offstage() {
                self.f[i].gone = true;
                self.f[i].replace_moves(Vec::new());
                continue;
            }
            if self.f[i].current() == Some(&Move::Dive) {
                continue;
            }
            let dir = if self.f[i].x >= front { 1.0 } else { -1.0 };
            let mut list = Vec::new();
            if matches!(self.f[i].current(), Some(Move::Stumble(_) | Move::Lie(_))) {
                list.push(Move::GetUp);
            }
            if self.f[i].flag {
                self.toss = Some(Toss { x: self.f[i].x, y: 40.0 * self.scale, dir: -dir, age: 0.0 });
            }
            let edge = if dir > 0.0 { self.max - EDGE } else { self.min + EDGE };
            let freeze = self.rng.range(0.05, 0.3);
            list.push(jump_fe(SHOCK, ALERT));
            list.push(shake_fe(freeze, SHOCK, ALERT));
            list.push(walk(edge, Gait::Run, Some(SHOCK), ALERT));
            list.push(Move::Dive);
            let r = &mut self.f[i];
            r.turned = false;
            r.flag = false;
            r.holding = false;
            r.targeted = false;
            r.replace_moves(list);
        }
    }

    // --------------------------------------------------------------- pose

    pub fn pose(&self, i: usize) -> Pose {
        let f = &self.f[i];
        let phase = self.age + 0.37 * i as f64;
        let s = f.size;
        let mut p = Pose {
            x: f.x,
            way: if f.turned { -f.last_way } else { f.last_way },
            lift: 0.0,
            lean: 0.0,
            squash: 1.0 + 0.015 * (3.0 * phase).sin(),
            wide: None,
            front: -2.15,
            back: 2.15,
            face: if phase % 2.7 < 0.11 { Face::Blink } else { Face::Normal },
            emote: Emote::None,
            bubble: 0.0,
            holding: f.holding,
            flag: f.flag,
            hat: f.hat_from.is_none(),
            visible: f.visible(),
        };
        let mv = match f.current() {
            Some(m) if f.begun || *m == Move::Rise => m,
            _ => return p,
        };
        let c = f.clock;
        match mv {
            Move::Offstage(_) | Move::Turn(_) => {}
            Move::Rise => {
                p.lift = -70.0 * s * (1.0 - ease(c / 0.45)) + arc(c / 0.45) * 6.0 * s;
            }
            Move::Walk { to, gait, look, sign } => {
                if (to - f.x).abs() > 0.5 {
                    p.way = (to - f.x).signum();
                    if let Some(l) = look {
                        p.face = *l;
                    }
                    p.emote = *sign;
                    let t = phase;
                    match gait {
                        Gait::Waddle | Gait::Trot | Gait::Backward => {
                            let a = f.walked / (if *gait == Gait::Waddle { 8.0 } else { 10.0 } * s);
                            p.lift = a.sin().abs() * 3.5 * s;
                            p.lean = a.sin() * 0.14;
                            p.front = -2.15 + 0.35 * a.sin();
                            p.back = 2.15 + 0.35 * a.sin();
                            if *gait == Gait::Backward {
                                p.lean -= 0.12;
                                p.way = -p.way;
                            }
                        }
                        Gait::Run => {
                            let a = f.walked / (7.0 * s);
                            p.lift = a.sin().abs() * 6.0 * s;
                            p.lean = 0.2 + 0.14 * a.sin();
                            arms(&mut p, -0.45 + 0.45 * (26.0 * t).sin());
                        }
                        Gait::Tiptoe => {
                            let a = f.walked / (11.0 * s);
                            p.lift = a.sin().abs() * 5.0 * s;
                            p.lean = -0.05;
                            arms(&mut p, -0.9);
                        }
                    }
                }
            }
            Move::Wait { look, sign, tremble, .. } => {
                if let Some(l) = look {
                    p.face = *l;
                }
                p.emote = *sign;
                if *tremble {
                    p.lean = 0.03 * (55.0 * c).sin();
                }
            }
            Move::Jump { look, sign } => {
                if let Some(l) = look {
                    p.face = *l;
                }
                p.emote = *sign;
                p.lift = arc(c / 0.3) * 11.0 * s;
            }
            Move::Scoop => {
                let third = f.scoop_time / 3.0;
                let k = ease(c / third) * (1.0 - ease((c - 2.0 * third) / third));
                p.lean = 0.55 * k;
                p.squash = 1.0 - 0.08 * k;
                p.front = mix(-2.15, -2.9, k);
            }
            Move::WindUp(look) => {
                let st = f.style.info();
                let k = ease(c / f.wind_time);
                p.face = *look;
                p.lean = st.wind_lean * k;
                p.front = mix(-2.15, st.pull_back, k);
                match f.style {
                    Style::Hopping => p.squash = 1.0 - 0.14 * k,
                    Style::Lob => p.squash = 1.0 - 0.06 * k,
                    Style::Spin if k < 0.85 && (19.0 * k).sin() < 0.0 => p.way = -p.way,
                    _ => {}
                }
            }
            Move::Release(_) => {
                let st = f.style.info();
                if c < 0.12 {
                    let k = c / 0.12;
                    p.lean = mix(st.wind_lean, st.follow_lean, k);
                    p.front = mix(st.pull_back, st.through, k);
                } else {
                    let k = (c - 0.12) / 0.14;
                    p.lean = mix(st.follow_lean, 0.0, k);
                    p.front = mix(st.through, -2.15, k);
                }
                if f.style == Style::Hopping {
                    p.lift = arc(c / 0.26) * 13.0 * s;
                }
                p.face = Face::Happy;
            }
            Move::Brace(r) => self.brace_pose(&mut p, i, *r, phase),
            Move::DropBall => {
                let k = c / 0.12;
                p.front = mix(-2.15, -1.5, k);
                p.back = mix(2.15, 1.5, k);
                p.face = Face::Normal;
            }
            Move::Shove => {
                if c < 0.12 {
                    p.lean = mix(0.0, -0.2, c / 0.12);
                    arms(&mut p, mix(-2.15, 0.4, c / 0.12));
                } else {
                    p.lean = mix(-0.2, 0.42, (c - 0.12) / 0.08);
                    arms(&mut p, -1.5);
                }
                p.face = Face::Angry;
            }
            Move::Stumble(_) => {
                p.lean = -1.5 * ease(c / 0.3);
                p.lift = p.lean.sin().abs() * 11.0 * s + arc(c / 0.42) * 14.0 * s;
                arms(&mut p, -0.8 + 0.5 * (40.0 * c).sin());
                p.face = SHOCK;
            }
            Move::Lie(_) => {
                p.lean = -1.5;
                p.lift = 11.0 * s;
                arms(&mut p, -1.2 + 0.2 * (18.0 * c).sin());
                p.face = Face::Dizzy;
            }
            Move::GetUp => {
                p.lean = -1.5 * (1.0 - ease(c / 0.38));
                p.lift = p.lean.sin().abs() * 11.0 * s + arc(c / 0.38) * 4.0 * s;
                p.face = Face::Dizzy;
            }
            Move::Crumble => {
                p.face = Face::Dizzy;
                p.lean = -0.28 * (1.0 - ease(c / 0.2)) + 0.2 * (38.0 * c).sin() * (1.0 - c / 0.5);
                p.squash = 1.0 - 0.22 * (1.0 - ease(c / 0.16));
                arms(&mut p, -0.9 + 0.5 * (40.0 * c).sin());
            }
            Move::Stagger(_) => {
                p.face = Face::Dizzy;
                p.lean = 0.24 * (9.0 * c).sin();
                p.lift = (9.0 * c).sin().abs() * 2.0 * s;
            }
            Move::Cheer(_) => cheer_pose(&mut p, c, s),
            Move::Dance(_) => {
                p.way = if (11.0 * c).sin() >= 0.0 { 1.0 } else { -1.0 };
                p.lift = (11.0 * c).sin().abs() * 9.0 * s;
                arms(&mut p, -0.5 + 0.3 * (22.0 * c).sin());
                p.face = Face::Happy;
            }
            Move::Taunt(_) => {
                p.lean = 0.2 * (20.0 * c).sin();
                p.lift = (10.0 * c).sin().abs() * 2.0 * s;
                p.squash = 1.0 + 0.05 * (20.0 * c).sin();
                p.face = Face::Happy;
                arms(&mut p, -0.6);
            }
            Move::Mourn(_) => {
                p.lean = 0.24;
                p.face = Face::Sad;
                arms(&mut p, -2.6);
            }
            Move::Pant(_) => {
                p.lean = 0.24 + 0.06 * (14.0 * c).sin();
                p.squash = 1.0 + 0.03 * (14.0 * c).sin();
                p.face = Face::Sad;
                arms(&mut p, -2.7);
            }
            Move::FlagOut => {
                p.front = mix(-2.15, -0.5, c / 0.25);
                p.face = Face::Sad;
            }
            Move::FlagAway => {
                p.front = mix(-0.5, 0.8, c / 0.25);
                p.face = Face::Sly;
            }
            Move::Wave(_) => {
                p.front = -0.5 + 0.35 * (9.0 * c).sin();
                p.lift = (4.5 * c).sin().abs() * 2.0 * s;
                p.face = Face::Sad;
                p.bubble = if c < 0.1 { mix(0.0, 1.25, c / 0.1) } else { mix(1.25, 1.0, (c - 0.1) / 0.1) };
            }
            Move::Handshake(_) => {
                p.front = -1.35 + 0.2 * (14.0 * c).sin();
                p.lift = (14.0 * c).sin().abs() * 1.5 * s;
                p.face = Face::Happy;
                p.emote = Emote::Heart;
            }
            Move::Build { .. } => {
                p.lean = (9.0 * c).sin().abs() * 0.45;
                p.front = -2.75;
                p.face = SHOCK;
            }
            Move::Hide(_) => crouch_pose(&mut p),
            Move::Peek(_) => {
                let k = ease(c / 0.2) * (1.0 - ease((c - 0.75) / 0.2));
                p.squash = mix(0.5, 0.92, k);
                p.wide = Some(1.04);
                p.lean = 0.08 * (16.0 * c).sin() * k;
                p.face = Face::Happy;
            }
            Move::GrabHat => {
                p.lean = arc(c / 0.5) * 0.55;
                p.front = -2.8;
                p.face = Face::Sly;
            }
            Move::Dive => {
                let u = c / 0.38;
                p.lift = arc(2.0 * u) * 14.0 * s - 70.0 * s * (2.0 * u - 1.0).clamp(0.0, 1.0);
                p.face = SHOCK;
                arms(&mut p, -0.5);
            }
        }
        p
    }

    fn brace_pose(&self, p: &mut Pose, i: usize, react: React, t: f64) {
        let f = &self.f[i];
        let (c, s, flair) = (f.clock, f.size, f.flair);
        let fl = self.flights.iter().find(|fl| fl.target == Some(i));
        let (pr, q) = fl.map_or((0.0, 0.5), |fl| (fl.progress(), fl.apex()));
        let has = fl.is_some();
        let k = ease((pr - q + 0.4) / 0.2) * (1.0 - ease((pr - q - 0.05) / 0.15));
        match react {
            React::Cheer => {
                cheer_pose(p, c, s);
                if has && pr > 0.55 {
                    p.face = SHOCK;
                }
            }
            React::Crouch => crouch_pose(p),
            _ => {
                p.lean = 0.07 * (9.0 * t).sin();
                p.lift = (9.0 * t).sin().abs() * 1.6 * s;
                if has {
                    self.react_pose(p, react, (pr, q, k), flair, s);
                }
            }
        }
    }

    fn react_pose(&self, p: &mut Pose, react: React, (pr, q, k): (f64, f64, f64), flair: f64, s: f64) {
        let shock = |p: &mut Pose| {
            p.face = SHOCK;
            p.lean = 0.0;
            p.lift = 0.0;
        };
        match react {
            React::Take | React::Cheer | React::Crouch => {
                if pr > 0.6 {
                    p.face = SHOCK;
                }
            }
            React::Duck => {
                shock(p);
                p.squash = 1.0 - 0.42 * k * flair;
                arms(p, mix(-2.15, -0.6, k));
            }
            React::Hop => {
                shock(p);
                p.lift = arc((pr - q + 0.34) / 0.68) * 38.0 * s * flair;
                arms(p, -1.4);
            }
            React::Lean => {
                shock(p);
                p.lean = -0.78 * k * flair;
                arms(p, mix(-2.15, -1.1, k));
            }
            React::Spin => {
                shock(p);
                if k > 0.05 && (46.0 * pr).sin() < 0.0 {
                    p.way = -p.way;
                }
                p.squash = 1.0 - 0.22 * k * flair;
                p.lift = 5.0 * s * k;
            }
            React::Watch => {
                shock(p);
                p.lean = -0.2 * flair * ease(pr / 0.5);
                p.face = if pr > 0.75 { Face::Happy } else { Face::Normal };
                if pr > 0.45 {
                    p.emote = Emote::Question;
                }
            }
            React::Flinch => {
                shock(p);
                let j = ease((pr - 0.55) / 0.2);
                p.squash = 1.0 - 0.14 * j * flair;
                p.lean = -0.14 * j;
                arms(p, mix(-2.15, -0.5, j));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::{StoryParams, Variant, story};

    pub(crate) const DT: f64 = 1.0 / 60.0;

    pub(crate) fn make(acts: Vec<[Vec<Move>; 2]>, spawn: [f64; 2]) -> Bout {
        let story = Story { acts, spawn, stand: spawn, ending: None };
        Bout::new(story, Lcg64::new(5), 0.0, 1000.0, 0.76, 0.0)
    }

    pub(crate) fn run_all(b: &mut Bout, limit: usize) -> Vec<FightEvent> {
        let mut out = Vec::new();
        for _ in 0..limit {
            b.step(DT, None, &mut out);
            b.pose(0);
            b.pose(1);
            if b.finished() {
                break;
            }
        }
        out
    }

    fn rest(b: &mut Bout, frames: usize) {
        let mut out = Vec::new();
        for _ in 0..frames {
            b.step(DT, None, &mut out);
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
        assert_eq!(ease(0.5), 0.5);
        assert!((arc(0.5) - 1.0).abs() < 1e-12);
        assert_eq!(mix(0.0, 10.0, 2.0), 10.0);
    }

    #[test]
    fn full_stories_finish_and_poses_are_finite() {
        for (n, v) in Variant::ALL.iter().enumerate() {
            for seed in 0..60u64 {
                let mut rng = Lcg64::new(seed * 7 + n as u64);
                let p = StoryParams { min: 100.0, max: 1100.0, scale: 0.76, avoid: None };
                let st = story(*v, &p, &mut rng);
                let mut b = Bout::new(st, rng, 100.0, 1100.0, 0.76, 0.0);
                let mut out = Vec::new();
                let mut frames = 0;
                while !b.finished() {
                    b.step(DT, None, &mut out);
                    for i in 0..2 {
                        let p = b.pose(i);
                        assert!(p.lift.is_finite() && p.lean.is_finite() && p.squash.is_finite());
                        assert!(p.front.is_finite() && p.back.is_finite());
                    }
                    for fl in &b.flights {
                        assert!(fl.pos().y.is_finite());
                    }
                    frames += 1;
                    assert!(frames < 6000, "{v:?} seed {seed} never finished");
                }
                let bursts = out.iter().filter(|e| matches!(e, FightEvent::Burst { .. })).count();
                let expect = match v {
                    Variant::Both => 2,
                    _ => 1,
                };
                assert_eq!(bursts, expect, "{v:?} seed {seed}");
                assert!(b.f.iter().all(|r| r.x >= 100.0 && r.x <= 1100.0));
            }
        }
    }

    #[test]
    fn move_timing_one_move_per_frame() {
        // Wait(0.1) at 60 fps: begins frame 1 (clock reset then += dt), ends when clock >= 0.1
        let w = Move::Wait { t: 0.1, look: None, sign: Emote::None, tremble: false };
        let mut b = make(vec![[vec![w.clone(), w.clone()], vec![]]], [100.0, 200.0]);
        let mut out = Vec::new();
        let mut ends = Vec::new();
        for n in 0..30 {
            let before = b.f[0].idx;
            b.step(DT, None, &mut out);
            if b.f[0].idx != before {
                ends.push(n);
            }
        }
        assert_eq!(ends.len(), 2);
        // 0.1 s = 6 frames (float: clock reaches 0.1 on the 6th or 7th add)
        assert!((5..=6).contains(&ends[0]));
        assert!(ends[1] - ends[0] >= 6);
        assert!(b.ended_at.is_some());
    }

    #[test]
    fn walk_moves_at_gait_speed_and_counts_odometer() {
        let mv = Move::Walk { to: 400.0, gait: Gait::Trot, look: None, sign: Emote::None };
        let mut b = make(vec![[vec![mv], vec![]]], [100.0, 900.0]);
        let pace = b.f[0].pace;
        let mut out = Vec::new();
        b.step(DT, None, &mut out);
        assert!((b.f[0].x - (100.0 + 140.0 * pace * DT)).abs() < 1e-9);
        assert!((b.f[0].walked - 140.0 * pace * DT).abs() < 1e-9);
        run_all(&mut b, 600);
        assert!((b.f[0].x - 400.0).abs() < 0.5);
        assert!(b.f[0].walked > 299.0 && b.f[0].walked < 301.0);
        // already there: ends at once
        let mv = Move::Walk { to: 100.2, gait: Gait::Waddle, look: None, sign: Emote::None };
        let mut b = make(vec![[vec![mv], vec![]]], [100.0, 900.0]);
        rest(&mut b, 1);
        assert_eq!(b.f[0].idx, 1);
    }

    #[test]
    fn run_and_pant_sweat_rate_limited() {
        let mv = Move::Walk { to: 900.0, gait: Gait::Run, look: None, sign: Emote::None };
        let mut b = make(vec![[vec![mv], vec![Move::Pant(1.0)]]], [100.0, 800.0]);
        rest(&mut b, 60);
        let sweat = b.specks.iter().flatten().filter(|s| s.sweat).count();
        // two fighters, one drop per 0.1 s each for ~0.35 s of life -> a few, never one per frame
        assert!((4..=10).contains(&sweat), "{sweat}");
    }

    #[test]
    fn scoop_windup_release_launch_a_head_shot() {
        let acts = vec![[
            vec![Move::Scoop, Move::WindUp(Face::Angry), Move::Release(Aim::Head)],
            vec![Move::Brace(React::Take), Move::Crumble],
        ]];
        let mut b = make(acts, [100.0, 300.0]);
        let mut out = Vec::new();
        let mut saw_holding = false;
        let mut launched = false;
        for _ in 0..400 {
            b.step(DT, None, &mut out);
            saw_holding |= b.f[0].holding;
            if !b.flights.is_empty() {
                launched = true;
                assert_eq!(b.flights[0].target, Some(1));
                assert!(b.f[1].targeted);
                assert!(!b.f[0].holding);
            }
            if b.finished() {
                break;
            }
        }
        assert!(saw_holding && launched);
        assert!(b.f[1].pile_at.is_some());
        assert!(b.f[1].hat_from.is_some());
        assert_eq!(out.iter().filter(|e| matches!(e, FightEvent::Burst { .. })).count(), 1);
        let FightEvent::Burst { at, area } = out[0].clone() else { panic!() };
        assert_eq!(area, BURST_AREA);
        assert!((at.x - 300.0).abs() < 1.0);
        assert!((at.y - 24.0 * b.f[1].size).abs() < 1e-9);
    }

    #[test]
    fn flight_geometry_per_aim() {
        for aim in [Aim::Head, Aim::Over, Aim::Under, Aim::Low, Aim::Wide, Aim::Shy, Aim::Short, Aim::Wall] {
            for (a, b_) in [(100.0, 400.0), (400.0, 100.0)] {
                let mut b = make(vec![[vec![Move::Release(aim)], vec![]]], [a, b_]);
                b.wall_x = 250.0;
                b.step(DT, None, &mut Vec::new());
                let fl = b.flights[0].clone();
                let dir = if b_ > a { 1.0 } else { -1.0 };
                assert!((fl.start.x - (a + dir * 14.0 * b.f[0].size)).abs() < 1e-9);
                assert_eq!(fl.start.y, 42.0 * b.f[0].size);
                assert_eq!(fl.target.is_some(), aim != Aim::Short);
                assert!(fl.time >= 0.05);
                assert!((fl.pos().x - fl.start.x).abs() < 1e-9 && (fl.pos().y - fl.start.y).abs() < 1e-6);
                let mut done = fl.clone();
                done.age = done.time;
                assert!((done.pos().x - fl.end.x).abs() < 1e-9);
                assert!((done.pos().y - fl.end.y).abs() < 1e-6);
                let mut mid = fl.clone();
                mid.age = fl.time * 0.5;
                assert!(mid.pos().y.is_finite());
                if aim == Aim::Head {
                    assert!((fl.end.x - (b_ - dir * 7.0 * b.f[1].size)).abs() < 1e-9);
                    assert_eq!(fl.end.y, 40.0 * b.f[1].size);
                }
                if aim == Aim::Wall {
                    assert_eq!(fl.end, Vec2::new(250.0, 20.0 * 0.76));
                }
                if aim == Aim::Short {
                    assert!((fl.end.x - (a + dir * 150.0)).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn degenerate_flight_uses_linear_height() {
        let fl = Flight {
            start: Vec2::new(10.0, 5.0),
            through: Vec2::new(10.0, 9.0),
            end: Vec2::new(10.0, 0.0),
            age: 0.5,
            time: 1.0,
            target: None,
            land: Land::Plain,
            size: 1.0,
        };
        assert_eq!(fl.apex(), 0.5);
        assert!((fl.pos().y - 2.5).abs() < 1e-12);
        let tilted = Flight { through: Vec2::new(14.0, 9.0), end: Vec2::new(20.0, 0.0), ..fl.clone() };
        assert!((tilted.apex() - 0.4).abs() < 1e-12);
    }

    #[test]
    fn landing_puffs_and_wall_cycle() {
        let mut b = make(vec![[vec![Move::Build { at: 500.0, t: 0.9 }], vec![]]], [400.0, 700.0]);
        rest(&mut b, 70);
        assert_eq!(b.wall_height, 1.0);
        assert!(b.specks.iter().flatten().count() > 0);
        // two wall hits: full -> half -> destroyed
        let land = |b: &mut Bout, l: Land| {
            let fl = Flight {
                start: Vec2::new(0.0, 0.0),
                through: Vec2::new(1.0, 1.0),
                end: Vec2::new(500.0, 20.0),
                age: 1.0,
                time: 1.0,
                target: None,
                land: l,
                size: 1.0,
            };
            b.land(&fl);
        };
        land(&mut b, Land::Wall);
        assert_eq!(b.wall_height, 0.5);
        land(&mut b, Land::Wall);
        assert_eq!(b.wall_height, 0.0);
        let n = b.specks.iter().flatten().count();
        land(&mut b, Land::Drop);
        land(&mut b, Land::Head);
        land(&mut b, Land::Plain);
        assert!(b.specks.iter().flatten().count() >= n);
    }

    #[test]
    fn speck_pool_is_a_ring_and_dies() {
        let mut b = make(vec![[vec![wait_long()], vec![]]], [100.0, 300.0]);
        b.puff(10.0, 5.0, 100, 100.0);
        assert_eq!(b.specks.iter().flatten().count(), MAX_SPECKS);
        for _ in 0..60 {
            b.update_specks(DT);
        }
        assert_eq!(b.specks.iter().flatten().count(), 0);
        b.puff(10.0, 5.0, 1, 100.0);
        let s = b.specks.iter().flatten().next().unwrap();
        assert!(s.size >= 2.0 && s.size <= 4.5 && s.life >= 0.3 && s.life <= 0.65 && s.dy > 0.0);
        b.update_specks(DT);
        let s = b.specks.iter().flatten().next().unwrap();
        assert!(s.y >= 1.0);
    }

    fn wait_long() -> Move {
        Move::Wait { t: 50.0, look: None, sign: Emote::None, tremble: false }
    }

    #[test]
    fn brace_waits_then_ends_and_resets_target() {
        let acts = vec![[vec![wait(0.5), Move::Release(Aim::Under)], vec![Move::Brace(React::Hop)]]];
        let mut b = make(acts, [100.0, 300.0]);
        let mut out = Vec::new();
        for _ in 0..20 {
            b.step(DT, None, &mut out);
        }
        assert_eq!(b.f[1].idx, 0, "brace waits before the throw");
        assert!(!b.f[1].targeted);
        for _ in 0..200 {
            b.step(DT, None, &mut out);
        }
        assert_eq!(b.f[1].idx, 1);
        assert!(!b.f[1].targeted);
    }

    fn wait(t: f64) -> Move {
        Move::Wait { t, look: None, sign: Emote::None, tremble: false }
    }

    #[test]
    fn brace_after_landing_ends_on_first_frame() {
        let acts = vec![[vec![Move::Release(Aim::Under)], vec![wait(2.0), Move::Brace(React::Take)]]];
        let mut b = make(acts, [100.0, 130.0]);
        let mut out = Vec::new();
        let (mut began, mut ended) = (None, None);
        for n in 0..200 {
            b.step(DT, None, &mut out);
            if b.f[1].idx == 1 && began.is_none() {
                began = Some(n);
            }
            if b.f[1].idx == 2 && ended.is_none() {
                ended = Some(n);
            }
        }
        // the ball landed long before the brace began: it ends on its first frame
        assert!(b.flights.is_empty());
        assert!(!b.f[1].targeted);
        assert!(ended.unwrap() - began.unwrap() <= 1);
    }

    #[test]
    fn drop_ball_flight() {
        let mut b = make(vec![[vec![Move::DropBall], vec![]]], [100.0, 300.0]);
        b.f[0].holding = true;
        rest(&mut b, 1);
        assert!(!b.f[0].holding);
        let fl = &b.flights[0];
        assert_eq!(fl.time, 0.24);
        assert_eq!(fl.size, 0.76, "bout scale, not randomised");
        assert_eq!(fl.target, None);
        assert_eq!(fl.land, Land::Drop);
        assert!((fl.start.x - (100.0 + 15.0 * b.f[0].size)).abs() < 1e-9);
        assert_eq!(fl.end.y, 0.0);
        rest(&mut b, 30);
        assert!(b.flights.is_empty());
    }

    #[test]
    fn stumble_slides_and_shove_lie_getup() {
        let acts = vec![[vec![Move::Shove], vec![Move::Stumble(180.0), Move::Lie(0.2), Move::GetUp]]];
        let mut b = make(acts, [100.0, 140.0]);
        let mut out = Vec::new();
        let mut min_lean: f64 = 0.0;
        for _ in 0..100 {
            b.step(DT, None, &mut out);
            min_lean = min_lean.min(b.pose(1).lean);
            b.pose(0);
        }
        assert!((b.f[1].x - 180.0).abs() < 1e-9);
        assert!(min_lean < -1.4);
    }

    #[test]
    fn grab_hat_marks_the_mound() {
        let acts = vec![[vec![wait(1.0), Move::GrabHat], vec![Move::Crumble]]];
        let mut b = make(acts, [100.0, 140.0]);
        rest(&mut b, 150);
        assert_eq!(b.f[1].worn_by, Some(0));
        // no mound: nobody wears it
        let mut b = make(vec![[vec![Move::GrabHat], vec![wait(1.0)]]], [100.0, 140.0]);
        rest(&mut b, 60);
        assert_eq!(b.f[0].worn_by, None);
        assert_eq!(b.f[1].worn_by, None);
    }

    #[test]
    fn flag_toss_lasts_point_seven() {
        let acts = vec![[vec![Move::FlagOut, Move::FlagAway], vec![]]];
        let mut b = make(acts, [100.0, 300.0]);
        let mut out = Vec::new();
        b.step(DT, None, &mut out);
        assert!(b.f[0].flag);
        let mut tossed = 0;
        for _ in 0..120 {
            b.step(DT, None, &mut out);
            if let Some(t) = b.toss {
                tossed += 1;
                assert!(!b.f[0].flag);
                assert_eq!(t.dir, -1.0);
                assert!(t.pos().y >= 0.0);
            }
        }
        assert!((40..=44).contains(&tossed), "{tossed}");
        assert!(b.toss.is_none());
    }

    #[test]
    fn panic_scatters_fighters_once() {
        // a: mid flag, b: stumbling; plough arrives close
        let acts = vec![
            [vec![Move::FlagOut, wait(3.0)], vec![Move::Stumble(120.0), Move::Lie(5.0)]],
            [vec![wait(10.0)], vec![wait(10.0)]],
        ];
        let mut b = make(acts, [100.0, 140.0]);
        b.f[0].flag = false;
        let mut out = Vec::new();
        for _ in 0..20 {
            b.step(DT, None, &mut out);
        }
        assert!(b.f[0].flag);
        b.wall_height = 1.0;
        b.step(DT, Some(1000.0), &mut out); // far: nothing
        assert!(!b.panicked);
        b.step(DT, Some(500.0), &mut out); // 420 away exactly from 100? (400 -> inside)
        assert!(b.panicked);
        assert!(b.toss.is_some());
        assert_eq!(b.wall_height, 0.0);
        assert_eq!(b.f[0].moves.len(), 4);
        assert!(matches!(b.f[0].moves[0], Move::Jump { .. }));
        assert_eq!(b.f[0].moves[3], Move::Dive);
        // b was lying/stumbling: GetUp prefix
        assert_eq!(b.f[1].moves[0], Move::GetUp);
        // both run away from the plough (to the left edge)
        assert!(matches!(b.f[0].moves[2], Move::Walk { to, .. } if to == 24.0));
        // once only
        let before = b.f[0].moves.clone();
        b.step(DT, Some(120.0), &mut out);
        assert_eq!(b.f[0].moves.len(), before.len());
        for _ in 0..600 {
            b.step(DT, Some(500.0), &mut out);
            if b.finished() {
                break;
            }
        }
        assert!(b.finished());
        assert!(b.f.iter().all(|r| r.gone));
    }

    #[test]
    fn panic_edge_cases() {
        // offstage -> gone at once; dive untouched; crumbling untouched; right side flees right
        let acts = vec![[vec![Move::Offstage(5.0)], vec![Move::Dive]]];
        let mut b = make(acts, [100.0, 900.0]);
        let mut out = Vec::new();
        b.step(DT, None, &mut out);
        b.step(DT, Some(500.0), &mut out);
        assert!(b.panicked);
        assert!(b.f[0].gone && b.f[0].moves.is_empty());
        assert_eq!(b.f[1].moves, vec![Move::Dive]);
        // dying fighter keeps its list
        let acts = vec![[vec![Move::Crumble], vec![wait(9.0)]]];
        let mut b = make(acts, [400.0, 600.0]);
        b.step(DT, None, &mut out);
        let keep = b.f[0].moves.clone();
        b.step(DT, Some(500.0), &mut out);
        assert_eq!(b.f[0].moves, keep);
        assert!(matches!(b.f[1].moves[2], Move::Walk { to, .. } if to == 976.0));
        // already-gone fighter is skipped, and an exhausted list still flees
        let mut b = make(vec![[vec![], vec![]]], [400.0, 900.0]);
        b.f[0].gone = true;
        b.f[1].replace_moves(vec![]);
        b.panic(500.0);
        assert!(b.f[0].moves.is_empty());
        assert_eq!(b.f[1].moves.len(), 4);
        // too late: ended bouts never panic
        let mut b = make(vec![[vec![], vec![]]], [400.0, 900.0]);
        b.step(DT, None, &mut out);
        assert!(b.ended_at.is_some());
        b.step(DT, Some(400.0), &mut out);
        assert!(!b.panicked);
    }

    #[test]
    fn end_linger_fade_finish() {
        let mut b = make(vec![[vec![], vec![]]], [100.0, 300.0]);
        let mut out = Vec::new();
        b.step(DT, None, &mut out);
        let e = b.ended_at.unwrap();
        assert_eq!(b.fading(), 0.0);
        assert!(!b.finished());
        while b.age < e + 1.45 {
            b.step(DT, None, &mut out);
        }
        assert!(b.fading() > 0.0 && b.fading() < 1.0);
        while !b.finished() {
            b.step(DT, None, &mut out);
        }
        assert!(b.age > e + 1.9 && b.age < e + 1.9 + 2.0 * DT);
        assert_eq!(b.fading(), 1.0);
    }

    #[test]
    fn obstacles_are_visible_fighters() {
        let mut b = make(vec![[vec![Move::Offstage(1.0)], vec![wait(1.0)]]], [100.0, 300.0]);
        assert_eq!(b.obstacles().len(), 1);
        let (x, half, h) = b.obstacles()[0];
        assert_eq!(x, 300.0);
        assert!((half - 35.0 * b.f[1].size).abs() < 1e-9);
        assert!((h - 58.0 * b.f[1].size).abs() < 1e-9);
        b.f[1].gone = true;
        assert!(b.obstacles().is_empty());
    }

    #[test]
    fn facing_rules() {
        let mut b = make(vec![[vec![wait(2.0)], vec![wait(2.0)]]], [100.0, 300.0]);
        rest(&mut b, 2);
        assert_eq!(b.pose(0).way, 1.0);
        assert_eq!(b.pose(1).way, -1.0);
        b.f[0].turned = true;
        assert_eq!(b.pose(0).way, -1.0);
        // degenerate gap keeps the last direction
        b.f[0].x = 300.0;
        b.f[0].turned = false;
        rest(&mut b, 1);
        assert_eq!(b.pose(0).way, 1.0);
        // opponent on the left flips it
        b.f[0].x = 500.0;
        rest(&mut b, 1);
        assert_eq!(b.pose(0).way, -1.0);
    }

    #[test]
    fn idle_blink_and_squash() {
        let mut b = make(vec![[vec![], vec![]]], [100.0, 300.0]);
        b.age = 0.05;
        assert_eq!(b.pose(0).face, Face::Blink);
        b.age = 1.0;
        let p = b.pose(0);
        assert_eq!(p.face, Face::Normal);
        assert!((p.squash - (1.0 + 0.015 * 3.0f64.sin())).abs() < 1e-12);
        assert!(p.hat && p.visible && !p.holding && !p.flag);
    }

    #[test]
    fn pose_of_each_gait() {
        for gait in [Gait::Waddle, Gait::Trot, Gait::Run, Gait::Tiptoe, Gait::Backward] {
            let mv = Move::Walk { to: 800.0, gait, look: Some(Face::Angry), sign: Emote::Alert };
            let mut b = make(vec![[vec![mv], vec![]]], [100.0, 900.0]);
            rest(&mut b, 20);
            let p = b.pose(0);
            assert_eq!(p.face, Face::Angry);
            assert_eq!(p.emote, Emote::Alert);
            assert_eq!(p.way, if gait == Gait::Backward { -1.0 } else { 1.0 });
            assert!(p.lift > 0.0 || gait == Gait::Run);
        }
    }

    #[test]
    fn dance_flips_and_rise_comes_from_below() {
        let mut b = make(vec![[vec![Move::Rise, Move::Dance(2.0)], vec![wait(3.0)]]], [100.0, 300.0]);
        assert!(b.pose(0).lift < -40.0, "unbegun rise starts underground");
        let mut ways = std::collections::BTreeSet::new();
        for _ in 0..100 {
            b.step(DT, None, &mut Vec::new());
            ways.insert(b.pose(0).way as i32);
        }
        assert_eq!(ways.len(), 2);
    }

    #[test]
    fn wave_bubble_and_handshake_heart() {
        let mut b = make(vec![[vec![Move::Wave(1.0), Move::Handshake(1.0)], vec![]]], [100.0, 300.0]);
        let mut bubbles = Vec::new();
        for _ in 0..12 {
            b.step(DT, None, &mut Vec::new());
            bubbles.push(b.pose(0).bubble);
        }
        assert!(bubbles.iter().cloned().fold(0.0, f64::max) > 1.2);
        assert!((bubbles[11] - 1.0).abs() < 0.2);
        rest(&mut b, 70);
        assert_eq!(b.pose(0).emote, Emote::Heart);
    }

    #[test]
    fn spin_windup_flips_facing_sometimes() {
        let mut flipped = false;
        for style in Style::ALL {
            let mut b = make(vec![[vec![Move::WindUp(Face::Angry)], vec![]]], [100.0, 300.0]);
            b.step(DT, None, &mut Vec::new());
            b.f[0].style = style;
            b.f[0].wind_time = 0.4;
            for _ in 0..20 {
                b.step(DT, None, &mut Vec::new());
                flipped |= b.pose(0).way < 0.0;
                let p = b.pose(0);
                assert_eq!(p.face, Face::Angry);
            }
        }
        assert!(flipped);
    }

    #[test]
    fn every_reaction_has_a_pose() {
        for react in [
            React::Take,
            React::Cheer,
            React::Crouch,
            React::Duck,
            React::Hop,
            React::Lean,
            React::Spin,
            React::Watch,
            React::Flinch,
        ] {
            let acts = vec![[vec![Move::Release(Aim::Head)], vec![Move::Brace(react)]]];
            let mut b = make(acts, [100.0, 400.0]);
            for _ in 0..120 {
                b.step(DT, None, &mut Vec::new());
                let p = b.pose(1);
                assert!(p.lift.is_finite() && p.squash.is_finite() && p.lean.is_finite());
                if react == React::Watch && b.flights.first().is_some_and(|f| f.progress() > 0.5) {
                    assert_eq!(p.emote, Emote::Question);
                }
            }
        }
    }

    #[test]
    fn new_bout_rolls_sizes_and_paces() {
        for seed in 0..20 {
            let b = Bout::new(
                Story { acts: vec![[vec![], vec![]]], spawn: [1.0, 2.0], stand: [1.0, 2.0], ending: None },
                Lcg64::new(seed),
                0.0,
                500.0,
                0.5,
                7.0,
            );
            for r in &b.f {
                assert!(r.size >= 0.45 && r.size <= 0.54);
                assert!(r.pace >= 0.9 && r.pace <= 1.12);
            }
            assert_eq!(b.f[0].last_way, 1.0);
            assert_eq!(b.f[1].last_way, -1.0);
            assert_eq!(b.panel, Frame::new(-60.0, 7.0, 620.0, 180.0));
            assert_eq!((b.f[0].scoop_time, b.f[0].wind_time, b.f[0].flair), (0.42, 0.34, 1.0));
        }
    }
}
