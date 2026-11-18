//! Story generator (snowman-fight 3): variants, moves, building blocks.

use fp_core::rng::Rng;

/// The four story variants (enum order = tray order).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Quick,
    Long,
    Both,
    Backstab,
}

impl Variant {
    pub const ALL: [Variant; 4] = [Variant::Quick, Variant::Long, Variant::Both, Variant::Backstab];
}

/// How a fighter walks (px/s at pace 1: 78, 140, 240, 105, 130).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gait {
    Waddle,
    Trot,
    Run,
    Tiptoe,
    Backward,
}

impl Gait {
    pub fn speed(self) -> f64 {
        match self {
            Gait::Waddle => 78.0,
            Gait::Trot => 140.0,
            Gait::Run => 240.0,
            Gait::Tiptoe => 105.0,
            Gait::Backward => 130.0,
        }
    }
}

/// The eight faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Normal,
    Blink,
    Happy,
    Shock,
    Dizzy,
    Angry,
    Sly,
    Sad,
}

/// The icon above a head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emote {
    None,
    Alert,
    Question,
    Heart,
}

/// Where a thrown ball goes (4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    Head,
    Over,
    Under,
    Low,
    Wide,
    Shy,
    Short,
    Wall,
}

/// How the target reacts while the ball flies (5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum React {
    Take,
    Cheer,
    Crouch,
    Duck,
    Hop,
    Lean,
    Spin,
    Watch,
    Flinch,
}

/// The five throw styles (4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Overhand,
    Sidearm,
    Lob,
    Hopping,
    Spin,
}

/// Numbers of one throw style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StyleInfo {
    pub pull_back: f64,
    pub through: f64,
    pub wind_lean: f64,
    pub follow_lean: f64,
    pub speed: f64,
    pub hand: f64,
    pub arc: f64,
}

impl Style {
    pub const ALL: [Style; 5] = [Style::Overhand, Style::Sidearm, Style::Lob, Style::Hopping, Style::Spin];

    pub fn info(self) -> StyleInfo {
        let (pull_back, through, wind_lean, follow_lean, speed, hand, arc) = match self {
            Style::Overhand => (0.8, -1.2, -0.22, 0.28, 1.0, 42.0, 0.22),
            Style::Sidearm => (1.7, -1.75, -0.14, 0.32, 1.25, 30.0, 0.08),
            Style::Lob => (2.7, -0.7, 0.12, 0.05, 0.72, 24.0, 0.5),
            Style::Hopping => (0.9, -1.2, -0.2, 0.28, 1.0, 54.0, 0.22),
            Style::Spin => (0.8, -1.3, -0.22, 0.28, 1.1, 42.0, 0.16),
        };
        StyleInfo { pull_back, through, wind_lean, follow_lean, speed, hand, arc }
    }
}

/// How the winner ends the fight (3.3 Finish).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Flee,
    Dance,
    StealHat,
    Mourn,
}

/// One scripted move (snowman-fight 3.1).
#[derive(Clone, Debug, PartialEq)]
pub enum Move {
    Offstage(f64),
    Rise,
    Walk { to: f64, gait: Gait, look: Option<Face>, sign: Emote },
    Wait { t: f64, look: Option<Face>, sign: Emote, tremble: bool },
    Jump { look: Option<Face>, sign: Emote },
    Turn(bool),
    Scoop,
    WindUp(Face),
    Release(Aim),
    Brace(React),
    DropBall,
    Shove,
    Stumble(f64),
    Lie(f64),
    GetUp,
    Crumble,
    Stagger(f64),
    Cheer(f64),
    Dance(f64),
    Taunt(f64),
    Mourn(f64),
    Wave(f64),
    Handshake(f64),
    Hide(f64),
    Peek(f64),
    Pant(f64),
    FlagOut,
    FlagAway,
    Build { at: f64, t: f64 },
    GrabHat,
    Dive,
}

fn wait(t: f64) -> Move {
    Move::Wait { t, look: None, sign: Emote::None, tremble: false }
}
fn wait_f(t: f64, f: Face) -> Move {
    Move::Wait { t, look: Some(f), sign: Emote::None, tremble: false }
}
fn wait_e(t: f64, e: Emote) -> Move {
    Move::Wait { t, look: None, sign: e, tremble: false }
}
fn wait_fe(t: f64, f: Face, e: Emote) -> Move {
    Move::Wait { t, look: Some(f), sign: e, tremble: false }
}
fn shake(t: f64, f: Face) -> Move {
    Move::Wait { t, look: Some(f), sign: Emote::None, tremble: true }
}
pub(crate) fn shake_fe(t: f64, f: Face, e: Emote) -> Move {
    Move::Wait { t, look: Some(f), sign: e, tremble: true }
}
fn jump_f(f: Face) -> Move {
    Move::Jump { look: Some(f), sign: Emote::None }
}
pub(crate) fn jump_fe(f: Face, e: Emote) -> Move {
    Move::Jump { look: Some(f), sign: e }
}
pub(crate) fn walk(to: f64, gait: Gait, look: Option<Face>, sign: Emote) -> Move {
    Move::Walk { to, gait, look, sign }
}
/// Shorthand for the usual `Walk` with a face and no icon.
fn walk_f(to: f64, gait: Gait, f: Face) -> Move {
    walk(to, gait, Some(f), Emote::None)
}
pub(crate) const SHOCK: Face = Face::Shock;
pub(crate) const ALERT: Emote = Emote::Alert;

/// Inputs of the generator (everything the stage decides).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StoryParams {
    /// Fighting range in screen x (global points).
    pub min: f64,
    pub max: f64,
    /// Bout scale (fighter height / 58).
    pub scale: f64,
    /// Ending the previous fight used (not repeated back to back).
    pub avoid: Option<Ending>,
}

/// A generated story: acts of per-fighter move lists plus the stand points.
#[derive(Clone, Debug, PartialEq)]
pub struct Story {
    pub acts: Vec<[Vec<Move>; 2]>,
    pub spawn: [f64; 2],
    pub stand: [f64; 2],
    /// Set by Quick and Long only (3.4).
    pub ending: Option<Ending>,
}

struct Script<'r, R: Rng> {
    rng: &'r mut R,
    acts: Vec<[Vec<Move>; 2]>,
    at: [f64; 2],
    min: f64,
    max: f64,
    scale: f64,
    avoid: Option<Ending>,
    ending: Option<Ending>,
}

/// Generate a story (3) for one of the variants.
pub fn story(variant: Variant, p: &StoryParams, rng: &mut impl Rng) -> Story {
    let (min, max) = (p.min, p.max);
    let width = max - min;
    let gap = (width * 0.55).min(rng.range(170.0, 230.0));
    let slack = width / 2.0 - gap / 2.0 - 44.0;
    let centre = (min + max) / 2.0 + if slack > 0.0 { rng.range(-slack, slack) } else { 0.0 };
    let at = [centre - gap / 2.0, centre + gap / 2.0];
    let spawn = [(at[0] - rng.range(90.0, 170.0)).max(min + 24.0), (at[1] + rng.range(90.0, 170.0)).min(max - 24.0)];
    let mut s = Script { rng, acts: Vec::new(), at, min, max, scale: p.scale, avoid: p.avoid, ending: None };
    s.arrive();
    let a = s.rng.below(2) as usize;
    match variant {
        Variant::Quick => s.quick(a),
        Variant::Long => s.long(a),
        Variant::Both => s.both(a),
        Variant::Backstab => s.backstab(a),
    }
    Story { acts: s.acts, spawn, stand: at, ending: s.ending }
}

impl<R: Rng> Script<'_, R> {
    fn act(&mut self, a: Vec<Move>, b: Vec<Move>) {
        self.acts.push([a, b]);
    }
    /// Puts `mine` on `who` and `theirs` on the other fighter.
    fn duo(&mut self, who: usize, mine: Vec<Move>, theirs: Vec<Move>) {
        if who == 0 {
            self.act(mine, theirs);
        } else {
            self.act(theirs, mine);
        }
    }
    fn both_do(&mut self, list: Vec<Move>) {
        self.act(list.clone(), list);
    }
    fn toward(&self, i: usize) -> f64 {
        if self.at[1 - i] > self.at[i] { 1.0 } else { -1.0 }
    }
    fn fit(&self, x: f64) -> f64 {
        x.min(self.max - 24.0).max(self.min + 24.0)
    }

    fn arrive(&mut self) {
        let late = self.rng.below(2) as usize;
        let delay = self.rng.range(0.3, 1.0);
        let r = self.rng.below(4) as usize;
        let list = |i: usize, s: &Self| {
            let wait = if i == late { delay } else { 0.0 };
            let gait = if r == i { Gait::Trot } else { Gait::Waddle };
            vec![Move::Offstage(wait), Move::Rise, walk(s.at[i], gait, None, Emote::None)]
        };
        let (a, b) = (list(0, self), list(1, self));
        self.act(a, b);
    }

    fn notice(&mut self, a: usize) {
        if self.rng.coin() {
            self.both_do(vec![jump_fe(SHOCK, ALERT), wait(0.45)]);
        } else {
            self.duo(
                a,
                vec![jump_fe(SHOCK, ALERT), wait_f(0.4, Face::Angry)],
                vec![wait_e(0.55, Emote::Question), jump_fe(SHOCK, ALERT)],
            );
        }
    }

    fn miss(&mut self) -> (Aim, React) {
        const MISSES: [(Aim, React); 6] = [
            (Aim::Over, React::Duck),
            (Aim::Over, React::Spin),
            (Aim::Under, React::Hop),
            (Aim::Low, React::Lean),
            (Aim::Wide, React::Watch),
            (Aim::Shy, React::Flinch),
        ];
        MISSES[self.rng.below(6) as usize]
    }

    fn mood(&mut self) -> Face {
        [Face::Happy, Face::Angry, Face::Sly][self.rng.below(3) as usize]
    }

    fn dodged(&mut self, a: usize) {
        let (aim, react) = self.miss();
        let mood = self.mood();
        self.duo(a, vec![Move::Scoop, Move::WindUp(mood), Move::Release(aim)], vec![Move::Brace(react)]);
        match self.rng.below(4) {
            0 => self.duo(a, vec![shake(0.4, Face::Angry)], vec![Move::Cheer(0.5)]),
            1 => self.duo(a, vec![wait_fe(0.45, Face::Sad, Emote::Question)], vec![jump_f(Face::Happy)]),
            2 => self.duo(a, vec![jump_f(Face::Angry)], vec![wait_f(0.35, Face::Sly)]),
            _ => {}
        }
    }

    fn volley(&mut self, a: usize) {
        if self.rng.below(4) == 0 {
            self.duo(
                a,
                vec![
                    Move::Scoop,
                    Move::WindUp(Face::Angry),
                    Move::Release(Aim::Short),
                    wait_fe(0.55, Face::Sad, Emote::Question),
                ],
                vec![wait(1.1), Move::Cheer(0.7)],
            );
        } else {
            self.dodged(a);
        }
        self.dodged(1 - a);
    }

    fn shove(&mut self, a: usize) {
        let b = 1 - a;
        let dir = self.toward(a);
        let home = self.at[a];
        let goal = self.at[b] - dir * 30.0 * self.scale;
        self.duo(
            a,
            vec![walk_f(goal, Gait::Trot, Face::Angry)],
            vec![wait_e(0.5, Emote::Question), wait_fe(0.2, SHOCK, ALERT)],
        );
        let stumble = self.fit(self.at[b] + dir * 46.0);
        self.duo(a, vec![Move::Shove], vec![wait(0.14), Move::Stumble(stumble)]);
        self.duo(a, vec![Move::Cheer(0.8)], vec![Move::Lie(1.0)]);
        self.duo(b, vec![Move::GetUp, jump_f(Face::Angry)], vec![wait_fe(0.6, SHOCK, ALERT)]);
        self.duo(a, vec![walk_f(home, Gait::Backward, SHOCK)], vec![Move::Scoop]);
        let (aim, react) = self.miss();
        self.duo(b, vec![Move::WindUp(Face::Angry), Move::Release(aim)], vec![Move::Brace(react)]);
        self.at[b] = stumble;
    }

    fn hide(&mut self, a: usize) {
        let dir = self.toward(a);
        let wall = self.at[a] + dir * 26.0 * self.scale;
        let mood = self.mood();
        self.duo(
            a,
            vec![jump_fe(SHOCK, ALERT), Move::Build { at: wall, t: 0.9 }, Move::Hide(0.5)],
            vec![wait_e(0.9, Emote::Question), wait_f(0.5, Face::Angry)],
        );
        self.duo(
            1 - a,
            vec![Move::Scoop, Move::WindUp(mood), Move::Release(Aim::Wall)],
            vec![Move::Brace(React::Crouch)],
        );
        self.duo(a, vec![Move::Peek(0.95)], vec![shake(0.95, Face::Angry)]);
        self.duo(
            1 - a,
            vec![Move::Scoop, Move::WindUp(Face::Angry), Move::Release(Aim::Wall)],
            vec![Move::Brace(React::Crouch)],
        );
        self.duo(a, vec![wait_fe(0.5, SHOCK, ALERT)], vec![Move::Cheer(0.5)]);
    }

    fn chase(&mut self, a: usize) {
        let b = 1 - a;
        let dir = self.toward(a);
        let (home_a, home_b) = (self.at[a], self.at[b]);
        let far = self.fit(home_b + dir * 120.0);
        self.duo(a, vec![jump_f(Face::Angry)], vec![wait_fe(0.35, SHOCK, ALERT)]);
        self.duo(
            a,
            vec![walk_f(far - dir * 58.0, Gait::Run, Face::Angry)],
            vec![walk(far, Gait::Run, Some(SHOCK), ALERT)],
        );
        self.duo(a, vec![wait(0.3), wait_fe(0.35, SHOCK, ALERT)], vec![wait(0.3), jump_f(Face::Angry)]);
        self.duo(a, vec![walk(home_a, Gait::Run, Some(SHOCK), ALERT)], vec![walk_f(home_b, Gait::Run, Face::Angry)]);
    }

    fn taunt(&mut self, a: usize) {
        self.duo(
            a,
            vec![Move::Turn(true), Move::Taunt(1.2)],
            vec![wait_e(0.6, Emote::Question), shake(0.65, Face::Angry)],
        );
        self.duo(
            1 - a,
            vec![Move::Scoop, Move::WindUp(Face::Angry), Move::Release(Aim::Under)],
            vec![Move::Brace(React::Hop), Move::Turn(false), wait_fe(0.4, SHOCK, ALERT)],
        );
    }

    fn kill(&mut self, a: usize) {
        self.duo(
            a,
            vec![Move::Scoop, Move::WindUp(Face::Angry), Move::Release(Aim::Head)],
            vec![Move::Brace(React::Take)],
        );
        self.finish(a);
    }

    fn finish(&mut self, w: usize) {
        let l = 1 - w;
        let to = if self.at[l] < self.at[w] { self.max - 24.0 } else { self.min + 24.0 };
        let to2 = if self.at[w] - self.min < self.max - self.at[w] { self.min + 24.0 } else { self.max - 24.0 };
        let all = [Ending::Flee, Ending::Dance, Ending::StealHat, Ending::Mourn];
        let options: Vec<Ending> = all.iter().copied().filter(|e| Some(*e) != self.avoid).collect();
        let ending = options[self.rng.below(options.len() as u64) as usize];
        self.ending = Some(ending);
        let side = if self.at[w] < self.at[l] { -1.0 } else { 1.0 };
        let mut list = match ending {
            Ending::Flee => {
                vec![Move::Cheer(0.5), shake_fe(0.75, SHOCK, ALERT), walk(to, Gait::Run, Some(SHOCK), ALERT)]
            }
            Ending::Dance => vec![Move::Dance(1.4), walk_f(to2, Gait::Trot, Face::Happy)],
            Ending::StealHat => vec![
                Move::Cheer(0.5),
                wait_f(0.55, Face::Sly),
                walk_f(self.at[l] + side * 15.0 * self.scale, Gait::Waddle, Face::Sly),
                Move::GrabHat,
                Move::Cheer(0.5),
                walk_f(to2, Gait::Trot, Face::Happy),
            ],
            Ending::Mourn => {
                vec![wait_fe(0.8, SHOCK, ALERT), Move::Mourn(1.5), walk_f(to2, Gait::Trot, Face::Sad)]
            }
        };
        list.push(Move::Dive);
        self.duo(w, list, vec![Move::Crumble]);
    }

    fn quick(&mut self, a: usize) {
        if self.rng.coin() {
            self.both_do(vec![wait_f(1.0, Face::Angry)]);
            self.duo(
                a,
                vec![Move::Scoop, Move::WindUp(Face::Angry), Move::Release(Aim::Head)],
                vec![Move::Scoop, Move::Brace(React::Take)],
            );
            self.finish(a);
        } else {
            self.duo(a, vec![wait_f(0.5, Face::Sly)], vec![Move::Handshake(0.7)]);
            self.kill(a);
        }
    }

    fn event(&mut self, which: usize, a: usize) {
        match which {
            0 => self.volley(a),
            1 => self.shove(a),
            2 => self.hide(a),
            3 => self.chase(a),
            _ => self.taunt(a),
        }
    }

    fn long(&mut self, a: usize) {
        self.notice(a);
        let mut order = [0usize, 1, 2, 3, 4];
        for i in (1..order.len()).rev() {
            order.swap(i, self.rng.below(i as u64 + 1) as usize);
        }
        let count = if self.rng.below(3) == 0 { 3 } else { 2 };
        let mut used = order[..count].to_vec();
        if !used.contains(&0) {
            used[0] = 0;
        }
        let mut actor = a;
        for which in used {
            self.event(which, actor);
            if self.rng.coin() {
                actor = 1 - actor;
            }
        }
        if self.rng.below(3) == 0 {
            self.both_do(vec![Move::Pant(1.1)]);
        }
        let w = self.rng.below(2) as usize;
        self.kill(w);
    }

    fn both(&mut self, a: usize) {
        self.notice(a);
        if self.rng.coin() {
            self.dodged(a);
        }
        let shot = |end: Move| {
            vec![Move::Scoop, Move::WindUp(Face::Angry), Move::Release(Aim::Head), Move::Brace(React::Take), end]
        };
        if self.rng.coin() {
            self.both_do(vec![wait_f(0.8, Face::Angry)]);
            self.both_do(shot(Move::Crumble));
        } else {
            self.duo(
                a,
                vec![
                    Move::Scoop,
                    Move::WindUp(Face::Angry),
                    Move::Release(Aim::Head),
                    Move::Brace(React::Cheer),
                    Move::Crumble,
                ],
                vec![
                    Move::Brace(React::Take),
                    Move::Stagger(0.8),
                    Move::WindUp(Face::Dizzy),
                    Move::Release(Aim::Head),
                    Move::Crumble,
                ],
            );
        }
    }

    fn backstab(&mut self, a: usize) {
        let b = 1 - a;
        let dir = self.toward(a);
        self.notice(a);
        self.duo(
            a,
            vec![Move::Scoop, wait_f(0.35, Face::Angry), Move::DropBall, Move::FlagOut, Move::Wave(1.5)],
            vec![Move::Scoop, wait_f(0.6, Face::Angry), wait_e(1.5, Emote::Question)],
        );
        self.duo(
            b,
            vec![Move::DropBall, Move::Jump { look: Some(Face::Happy), sign: Emote::Heart }],
            vec![Move::Wave(0.75)],
        );
        let mid = (self.at[0] + self.at[1]) / 2.0;
        let (ga, gb) = (mid - dir * 15.0 * self.scale, mid + dir * 15.0 * self.scale);
        self.duo(a, vec![walk_f(ga, Gait::Waddle, Face::Sad)], vec![walk_f(gb, Gait::Waddle, Face::Happy)]);
        self.at[a] = ga;
        self.at[b] = gb;
        self.both_do(vec![Move::Handshake(1.3)]);
        let away = self.fit(self.at[b] + dir * 72.0);
        self.duo(
            b,
            vec![Move::Turn(true), walk_f(away, Gait::Waddle, Face::Happy), Move::Brace(React::Take)],
            vec![
                wait_f(0.55, Face::Sly),
                Move::FlagAway,
                wait_f(0.2, Face::Sly),
                Move::Scoop,
                Move::WindUp(Face::Sly),
                Move::Release(Aim::Head),
            ],
        );
        self.at[b] = away;
        let edge = if self.at[a] - self.min < self.max - self.at[a] { self.min + 24.0 } else { self.max - 24.0 };
        self.duo(
            a,
            vec![Move::Cheer(0.6), wait_f(0.5, Face::Sly), walk_f(edge, Gait::Tiptoe, Face::Sly), Move::Dive],
            vec![Move::Crumble],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::rng::Lcg64;

    fn params() -> StoryParams {
        StoryParams { min: 100.0, max: 1100.0, scale: 0.76, avoid: None }
    }

    fn all_moves(s: &Story) -> Vec<Move> {
        s.acts.iter().flat_map(|a| a.iter().flatten().cloned()).collect()
    }

    #[test]
    fn gait_speeds() {
        let speeds: Vec<f64> =
            [Gait::Waddle, Gait::Trot, Gait::Run, Gait::Tiptoe, Gait::Backward].iter().map(|g| g.speed()).collect();
        assert_eq!(speeds, vec![78.0, 140.0, 240.0, 105.0, 130.0]);
    }

    #[test]
    fn style_numbers() {
        let o = Style::Overhand.info();
        assert_eq!((o.pull_back, o.through, o.hand, o.arc), (0.8, -1.2, 42.0, 0.22));
        let infos: Vec<StyleInfo> = Style::ALL.iter().map(|s| s.info()).collect();
        assert_eq!(infos[2].speed, 0.72);
        assert_eq!(infos[3].hand, 54.0);
        assert_eq!(infos[1].follow_lean, 0.32);
        assert_eq!(infos[4].wind_lean, -0.22);
    }

    #[test]
    fn arrive_act_zero() {
        for seed in 0..40 {
            let s = story(Variant::Quick, &params(), &mut Lcg64::new(seed));
            let act = &s.acts[0];
            for (i, list) in act.iter().enumerate() {
                assert_eq!(list.len(), 3);
                assert!(matches!(list[0], Move::Offstage(_)));
                assert_eq!(list[1], Move::Rise);
                assert!(matches!(list[2], Move::Walk { to, .. } if to == s.stand[i]));
            }
            let delays: Vec<f64> =
                act.iter().filter_map(|l| if let Move::Offstage(t) = l[0] { Some(t) } else { None }).collect();
            assert_eq!(delays.len(), 2);
            assert!(delays.contains(&0.0));
            assert!(delays.iter().all(|d| *d == 0.0 || (0.3..1.0).contains(d)));
            let trots = act.iter().filter(|l| matches!(l[2], Move::Walk { gait: Gait::Trot, .. })).count();
            assert!(trots <= 1);
        }
    }

    #[test]
    fn stand_points() {
        for seed in 0..60 {
            let s = story(Variant::Both, &params(), &mut Lcg64::new(seed));
            let gap = s.stand[1] - s.stand[0];
            assert!((170.0..230.0).contains(&gap), "gap {gap}");
            let centre = (s.stand[0] + s.stand[1]) / 2.0;
            assert!((centre - 600.0).abs() <= 500.0 - gap / 2.0 - 44.0 + 1e-9);
            assert!(s.spawn[0] >= 124.0 && s.spawn[1] <= 1076.0);
            assert!(s.spawn[0] <= s.stand[0] && s.spawn[1] >= s.stand[1]);
        }
        // narrow range: gap = 0.55 width, no slack
        let narrow = StoryParams { min: 0.0, max: 200.0, scale: 0.76, avoid: None };
        let s = story(Variant::Quick, &narrow, &mut Lcg64::new(1));
        assert!((s.stand[1] - s.stand[0] - 110.0).abs() < 1e-9);
        assert!((s.stand[0] + s.stand[1] - 200.0).abs() <= 2.0);
        assert!(s.spawn[0] >= 24.0 && s.spawn[1] <= 176.0);
    }

    #[test]
    fn quick_and_long_record_an_ending() {
        for seed in 0..40 {
            for v in [Variant::Quick, Variant::Long] {
                let s = story(v, &params(), &mut Lcg64::new(seed));
                assert!(s.ending.is_some());
                let last = s.acts.last().unwrap();
                assert!(last.iter().any(|l| l == &vec![Move::Crumble]));
                assert!(last.iter().any(|l| l.last() == Some(&Move::Dive)));
            }
            for v in [Variant::Both, Variant::Backstab] {
                let s = story(v, &params(), &mut Lcg64::new(seed));
                assert_eq!(s.ending, None);
            }
        }
    }

    #[test]
    fn ending_avoidance_and_variety() {
        let mut seen = std::collections::BTreeSet::new();
        for avoid in [Ending::Flee, Ending::Dance, Ending::StealHat, Ending::Mourn] {
            for seed in 0..60 {
                let p = StoryParams { avoid: Some(avoid), ..params() };
                let s = story(Variant::Quick, &p, &mut Lcg64::new(seed));
                assert_ne!(s.ending, Some(avoid));
                seen.insert(format!("{:?}", s.ending));
            }
        }
        assert_eq!(seen.len(), 4);
    }

    #[test]
    fn finish_lists() {
        // every ending produces its own winner list ending in Dive
        let mut got = std::collections::BTreeMap::new();
        for seed in 0..120 {
            let s = story(Variant::Quick, &params(), &mut Lcg64::new(seed));
            let last = s.acts.last().unwrap();
            let win = last.iter().find(|l| l.last() == Some(&Move::Dive)).unwrap();
            got.insert(format!("{:?}", s.ending.unwrap()), win.clone());
        }
        assert_eq!(got.len(), 4);
        assert!(matches!(got["Dance"][0], Move::Dance(t) if t == 1.4));
        assert!(got["StealHat"].contains(&Move::GrabHat));
        assert!(got["Mourn"].contains(&Move::Mourn(1.5)));
        assert!(matches!(got["Flee"][2], Move::Walk { gait: Gait::Run, .. }));
    }

    #[test]
    fn long_has_volley_first_and_two_or_three_events() {
        for seed in 0..80 {
            let s = story(Variant::Long, &params(), &mut Lcg64::new(seed));
            // act 0 arrive, then notice (1 act), then volley: Scoop/WindUp/Release or Dodged
            assert!(s.acts.len() >= 4);
            let moves = all_moves(&s);
            assert!(moves.iter().any(|m| matches!(m, Move::Release(_))));
        }
    }

    #[test]
    fn variants_contain_their_signature_moves() {
        let mut shove = false;
        let mut hide = false;
        let mut chase = false;
        let mut taunt = false;
        let mut pant = false;
        for seed in 0..200 {
            let m = all_moves(&story(Variant::Long, &params(), &mut Lcg64::new(seed)));
            shove |= m.contains(&Move::Shove);
            hide |= m.iter().any(|x| matches!(x, Move::Build { .. }));
            chase |= m.iter().any(|x| matches!(x, Move::Walk { gait: Gait::Run, look: Some(Face::Angry), .. }));
            taunt |= m.contains(&Move::Turn(true));
            pant |= m.contains(&Move::Pant(1.1));
        }
        assert!(shove && hide && chase && taunt && pant);
        let mut handshake = false;
        let mut dizzy = false;
        let mut lens = std::collections::BTreeSet::new();
        let mut mutual = false;
        for seed in 0..80 {
            let s = story(Variant::Both, &params(), &mut Lcg64::new(seed));
            let m = all_moves(&s);
            handshake |= m.contains(&Move::Handshake(0.7));
            dizzy |= m.contains(&Move::WindUp(Face::Dizzy));
            lens.insert(s.acts.len());
            mutual |= s.acts.last().unwrap()[0] == s.acts.last().unwrap()[1];
            assert_eq!(m.iter().filter(|x| **x == Move::Crumble).count(), 2);
        }
        assert!(dizzy && lens.len() >= 3 && mutual && !handshake);
        let s = story(Variant::Backstab, &params(), &mut Lcg64::new(3));
        let m = all_moves(&s);
        assert!(m.contains(&Move::FlagAway) && m.contains(&Move::FlagOut) && m.contains(&Move::Dive));
        assert_eq!(m.iter().filter(|x| **x == Move::Handshake(1.3)).count(), 2);
        assert_eq!(m.iter().filter(|x| **x == Move::Crumble).count(), 1);
        let mut quick_hs = false;
        for seed in 0..40 {
            let m = all_moves(&story(Variant::Quick, &params(), &mut Lcg64::new(seed)));
            quick_hs |= m.contains(&Move::Handshake(0.7));
        }
        assert!(quick_hs);
    }

    #[test]
    fn backstab_bookkeeping_and_positions() {
        for seed in 0..40 {
            let p = params();
            let s = story(Variant::Backstab, &p, &mut Lcg64::new(seed));
            for m in all_moves(&s) {
                if let Move::Walk { to, .. } = m {
                    assert!(to >= p.min + 24.0 - 1e-9 && to <= p.max - 24.0 + 1e-9);
                }
            }
        }
    }

    #[test]
    fn shove_stays_in_range() {
        let p = StoryParams { min: 0.0, max: 320.0, scale: 0.76, avoid: None };
        for seed in 0..200 {
            let s = story(Variant::Long, &p, &mut Lcg64::new(seed));
            for m in all_moves(&s) {
                if let Move::Stumble(to) = m {
                    assert!((24.0..=296.0).contains(&to));
                }
            }
        }
    }
}
