//! Visitors that cross the desktop while it snows.
//!
//! Spec: characters 0.1 (conventions, screen lists, visitor toggles),
//! 1 (Santa sleigh), 2 (snowplough, blade -> `fp_core::feed::Blade`),
//! 3 (yeti), 4 (elf rig), 5 (train, local only: see below), 6 (Sketch is in
//! `fp-paint`), 7 (persistence keys `lastSantaVisit`, `lastGift`,
//! `trainQueue`, `trainLook`), 9 (open questions: Q3 yeti timeout is added);
//! social 2.11 (gift box physics and looks; Santa daily gift + quote of the
//! day; item gift and letters dropped), social 2.13 (DailyVisits scheduler);
//! ui 3.9 QuoteCard data model (own quotes; card UI in `fp-ui`).
//!
//! Local train (Frostpane): summoned from the tray. At the station the elf
//! offers to load a standing snowman or tree (`Fetch` timeline 5.3); the
//! train leaves and returns as a delivery run on the next shown screen (or
//! the same one) and unloads the cargo there (`Coming` + placement, 5.3).
//! Deliveries are queued in `trainQueue` so a quit mid-run replays.
//!
//! Also here: [`quote`] (ui 3.9 deck selection, `quotesLeft*` prefs; the
//! quote texts are in `fp-text`) and [`groundlife`] (other-weather 2.5,
//! the settings-page vignette, because it reuses the sleigh and deer art).

mod art;
mod gift;
pub mod groundlife;
mod plough;
pub mod quote;
mod santa;
#[cfg(test)]
mod tests;
mod train;
mod util;
mod visits;
mod yeti;

pub use gift::GiftBox;
pub use plough::Plough;
pub use santa::{Block, Santa};
pub use train::{Train, TrainLook};
pub use visits::DailyVisits;
pub use yeti::Yeti;

use fp_core::clock::WallTime;
use fp_core::effect::Visitor;
use fp_core::feed::Blade;
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::Lcg64;
use fp_core::scene::{Owner, PointerInput, Sprite, SpriteKey};

/// What a character sees of one screen (characters 0.1 `Screen`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageScreen {
    pub frame: Frame,
    pub shown: bool,
    /// Taskbar ledge: `(min_x, max_x, top)`.
    pub dock: Option<(f64, f64, f64)>,
}

/// Shared per-step context.
pub struct StepCtx<'a> {
    pub now: f64,
    pub dt: f64,
    pub wall: WallTime,
    pub visit_screens: &'a [StageScreen],
    pub plough_screens: &'a [StageScreen],
    pub shown_screens: &'a [StageScreen],
    pub pointer: &'a PointerInput,
}

/// Cross-actor effects for `fp-world` to route.
#[derive(Clone, Debug, PartialEq)]
pub enum CharacterEvent {
    Burst {
        at: Vec2,
        area: f64,
        velocity: Vec2,
    },
    /// Gift dropped by Santa at a point.
    GiftDropped {
        at: Vec2,
    },
    /// Gift opened: burst 500 + quote card.
    GiftOpened {
        at: Vec2,
    },
    /// Yeti reached a snowman stack.
    Smash {
        bottom: u64,
    },
    /// Yeti chopped a tree.
    Chop {
        tree: u64,
    },
    /// The summoned train waits at the station: open the station bubble
    /// (pick a standing snowman/tree, or wave goodbye) anchored at `at`.
    TrainReady {
        at: Vec2,
    },
    /// The elf lifted the picked object: the world starts carrying it at
    /// [`Visitors::carry_point`].
    Took {
        pick: Pick,
    },
    /// The picked object landed on the flat car: the world takes it out of
    /// its owner (`take`) and hands the value back with [`Visitors::load`].
    Loaded {
        pick: Pick,
    },
    /// A delivery run reached its placement frame: the world plants/stands
    /// the cargo at `x` on `screen` (characters 5.3 table, t = 2.3 s). When
    /// the cargo holds both a tree and a snowman, the snowman goes 70 points
    /// to the left of the tree.
    Unload {
        cargo: Cargo,
        x: f64,
        screen: Frame,
    },
    /// A summoned run ended or was stopped: drop any riding object.
    Left,
    /// DailyVisits for snowmen fired: the world begins a fight
    /// (snowman-fight 2.1, pref `lastSnowmanFight`).
    FightDue,
    /// Persist visit/gift/train state.
    Dirty,
}

/// A standing object the user picked at the station.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Bottom ball id of a snowman stack (`fp-snowballs`).
    Snowman(u64),
    /// Tree id (`fp-trees`).
    Tree(u64),
}

/// What a train carries. Values are opaque JSON made by
/// `fp_snowballs::Balls::take` / `fp_trees::Grove::take`, so this crate
/// does not depend on those crates. Persisted inside `trainQueue`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cargo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snowman: Option<serde_json::Value>,
}

impl Cargo {
    /// Nothing aboard.
    pub fn is_empty(&self) -> bool {
        self.tree.is_none() && self.snowman.is_none()
    }
}

/// All visitors plus the gift box.
pub struct Visitors {
    rng: Lcg64,
    started: bool,
    away: Vec<Visitor>,
    often: bool,
    santa: Santa,
    plough: Plough,
    yeti: Yeti,
    train: Train,
    gift: Option<GiftBox>,
    santa_visits: DailyVisits,
    fights: DailyVisits,
    last_santa: Option<i64>,
    last_fight: Option<i64>,
    /// A bout is running (set by the world): fights are not due meanwhile.
    fight_active: bool,
    last_gift: Option<String>,
    depths: Vec<(Frame, f64)>,
    shown: Vec<StageScreen>,
    rev: u64,
}

impl Default for Visitors {
    fn default() -> Self {
        Self::new(1)
    }
}

/// `unix` seconds as the ISO UTC string persisted for visit stamps.
fn stamp(unix: Option<i64>) -> Option<String> {
    unix.map(|u| WallTime::new(u, 0).iso_utc())
}

impl Visitors {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Lcg64::new(seed),
            started: false,
            away: Vec::new(),
            often: false,
            santa: Santa::default(),
            plough: Plough::default(),
            yeti: Yeti::default(),
            train: Train::default(),
            gift: None,
            santa_visits: DailyVisits::santa(),
            fights: DailyVisits::snowmen(),
            last_santa: None,
            last_fight: None,
            fight_active: false,
            last_gift: None,
            depths: Vec::new(),
            shown: Vec::new(),
            rev: 0,
        }
    }

    /// Visitor toggles (`visitorsAway`): automatic visits skip these.
    pub fn set_away(&mut self, away: &[Visitor]) {
        self.away = away.to_vec();
    }

    /// "Frequent visitors" (`oftenVisits`): waits divided by 6.
    pub fn set_often(&mut self, often: bool) {
        self.often = often;
    }

    /// Setting `behindWindows`: the plough is ordered under the taskbar
    /// (characters 2.1 window order); otherwise it is topmost.
    pub fn set_behind_windows(&mut self, behind: bool) {
        self.plough.set_behind(behind);
    }

    /// Train art (`trainLook`).
    pub fn set_train_look(&mut self, look: TrainLook) {
        self.train.set_look(look);
    }

    /// Ground snow depth of a screen, reported by the snow field each frame
    /// (gates automatic plough runs).
    pub fn report_depth(&mut self, screen: Frame, depth: f64) {
        match self.depths.iter_mut().find(|d| d.0 == screen) {
            Some(d) => d.1 = depth,
            None => self.depths.push((screen, depth)),
        }
    }

    /// The world reports whether a bout runs (snowman-fight 2.3: due is
    /// evaluated only while none does).
    pub fn set_fight_active(&mut self, active: bool) {
        self.fight_active = active;
    }

    /// A non-manual bout began: note the visit (`lastSnowmanFight`).
    pub fn fight_came(&mut self, wall: WallTime) {
        self.last_fight = Some(wall.unix);
    }

    /// A non-manual bout ended or its screen vanished: wait again.
    pub fn fight_ended(&mut self, now: f64) {
        self.fights.wait_again(now, self.often, &mut self.rng);
    }

    fn comes(&self, v: Visitor) -> bool {
        !self.away.contains(&v)
    }

    pub fn start(&mut self, now: f64) {
        self.started = true;
        self.santa_visits.start(now, self.often, &mut self.rng);
        self.fights.start(now, self.often, &mut self.rng);
        self.plough.start(now, &mut self.rng);
        self.yeti.start(now, &mut self.rng);
        self.train.start(now, &mut self.rng);
    }

    pub fn stop(&mut self) {
        self.started = false;
        self.santa_visits.stop();
        self.fights.stop();
        self.santa.stop();
        self.plough.stop();
        self.yeti.stop();
        self.train.stop();
    }

    /// Tray "send" commands; ignore visitor toggles (characters 0.1).
    pub fn send_santa(&mut self, _now: f64) {
        self.santa.send(true);
    }

    pub fn send_plough(&mut self, _now: f64) {
        self.plough.send();
    }

    pub fn send_yeti(&mut self, now: f64) {
        self.yeti.send(now);
    }

    pub fn summon_train(&mut self, _now: f64) {
        self.train.summon();
    }

    /// Station bubble answers (characters 5.3 Waiting): fetch a standing
    /// object at world `x`, or leave empty.
    pub fn train_fetch(&mut self, pick: Pick, x: f64, now: f64) {
        self.train.fetch(pick, x, now);
    }

    pub fn train_dismiss(&mut self, now: f64) {
        self.train.dismiss(now);
    }

    /// Cargo value for the object announced by `Loaded`; when the summoned
    /// run leaves, the cargo is queued as a delivery (`trainQueue`).
    pub fn load(&mut self, cargo: Cargo) {
        let snowman = cargo.snowman.is_some();
        if self.train.load(cargo) && snowman && self.yeti.idle() {
            self.yeti.chase();
        }
    }

    /// Where the world draws the carried/riding object this frame.
    pub fn carry_point(&self) -> Option<Vec2> {
        self.train.carry_point()
    }

    /// Cargo riding on a delivery run before placement (5.3 CargoAboard).
    pub fn cargo_aboard(&self) -> Option<&Cargo> {
        self.train.cargo_aboard()
    }

    /// Current plough front on the screen with this frame (fight 4.7 panic).
    pub fn plough_front(&self, screen: Frame) -> Option<f64> {
        self.plough.front_on(screen)
    }

    /// One step. `prey` = snowman stacks (3+ balls) as the bottom-centre
    /// point of each stack; `doomed` = chop point of the tree to fell;
    /// `obstacles` = fight hop rectangles for Santa (screen, x0, x1, bottom, top).
    pub fn step(
        &mut self,
        ctx: &StepCtx<'_>,
        prey: &[(u64, Vec<(Vec2, f64)>)],
        doomed: Option<(u64, Vec2, f64)>,
        obstacles: &[Block],
        out: &mut Vec<CharacterEvent>,
    ) {
        self.rev += 1;
        self.shown = ctx.shown_screens.to_vec();
        if !self.started {
            out.extend(self.train.take_events());
            return;
        }
        let now = ctx.now;
        let today = ctx.wall.local_date().iso();
        self.santa_visits.tick(now, ctx.wall, self.last_santa, self.often, &mut self.rng);
        self.fights.tick(now, ctx.wall, self.last_fight, self.often, &mut self.rng);
        if !self.fight_active
            && self.fights.due(now, self.comes(Visitor::Snowmen))
            && ctx.visit_screens.iter().any(|s| s.shown)
        {
            out.push(CharacterEvent::FightDue);
        }

        let due = self.santa_visits.due(now, self.comes(Visitor::Santa));
        let dropping = self.gift.is_none() && self.last_gift.as_deref() != Some(today.as_str());
        let s = self.santa.step(ctx, due, (obstacles, self.plough.block()), dropping, &mut self.rng);
        if s.came {
            self.last_santa = Some(ctx.wall.unix);
            out.push(CharacterEvent::Dirty);
        }
        if s.ended_auto {
            self.santa_visits.wait_again(now, self.often, &mut self.rng);
        }
        if let Some(at) = s.drop {
            self.gift = Some(GiftBox::new(at));
            self.last_gift = Some(today);
            out.push(CharacterEvent::GiftDropped { at });
            out.push(CharacterEvent::Dirty);
        }

        let comes = self.comes(Visitor::Plough);
        self.plough.step(ctx, comes, &self.depths, &mut self.rng);
        let targets = yeti::Targets { prey, doomed, riding: self.train.riding() };
        let comes = self.comes(Visitor::Yeti);
        self.yeti.step(ctx, &targets, comes, &mut self.rng, out);
        self.train.step(ctx, &mut self.rng, out);

        if let Some(at) = self.gift.as_mut().and_then(|g| g.step(ctx)) {
            self.gift = None;
            out.push(CharacterEvent::GiftOpened { at });
        }
    }

    /// The plough blade for the snow field, if any.
    pub fn blade(&self) -> Option<(Vec2, Blade)> {
        self.plough.blade()
    }

    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        let rev = self.rev;
        if let Some(g) = self.gift.as_ref().filter(|_| self.started) {
            out.push(g.sprite(rev));
        }
        self.santa.sprites(rev, out);
        self.plough.sprites(rev, out);
        self.yeti.sprites(rev, &self.shown, out);
        self.train.sprites(rev, out);
    }

    /// Art for a sprite key (1.4, 2.3, 3.5, 4, 5.4, social 2.11 looks).
    pub fn paint(&self, key: SpriteKey, scale: f32) -> Option<fp_paint::tiny_skia::Pixmap> {
        let id = key.id();
        if key.owner_byte() == Owner::Gift as u8 {
            return self.gift.as_ref()?.paint(scale);
        }
        if key.owner_byte() != Owner::Character as u8 {
            return None;
        }
        match id {
            1 => self.santa.paint(scale),
            2 => self.plough.paint(scale),
            4 | 5 => self.train.paint(id, scale),
            _ => self.yeti.paint(id, scale),
        }
    }

    pub fn save_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        if let Some(s) = stamp(self.last_santa) {
            m.insert("lastSantaVisit".into(), s.into());
        }
        if let Some(s) = stamp(self.last_fight) {
            m.insert("lastSnowmanFight".into(), s.into());
        }
        if let Some(d) = &self.last_gift {
            m.insert("lastGift".into(), d.clone().into());
        }
        m.insert("trainQueue".into(), self.train.queue_json());
        m.insert("trainLook".into(), self.train.look().raw().into());
        serde_json::Value::Object(m)
    }

    pub fn load_json(&mut self, v: &serde_json::Value) {
        let when = |k: &str| v.get(k).and_then(|s| s.as_str()).and_then(WallTime::parse_iso_utc);
        self.last_santa = when("lastSantaVisit");
        self.last_fight = when("lastSnowmanFight");
        self.last_gift = v.get("lastGift").and_then(|s| s.as_str()).map(str::to_string);
        self.train.load_queue(v.get("trainQueue").unwrap_or(&serde_json::Value::Null));
        let look = v.get("trainLook").and_then(|s| s.as_str()).map_or(TrainLook::Festive, TrainLook::parse);
        self.train.set_look(look);
    }
}
