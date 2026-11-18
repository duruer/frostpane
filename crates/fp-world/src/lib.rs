//! The desktop world: everything that lives in its own CPU-drawn window and
//! is stepped on the UI thread (~80 Hz, dt <= 0.05 s).
//!
//! Spec: snowballs 3.1 (lifecycle `run(on)`, gating on shown weather ==
//! snow and the `snowballs` setting, battery-pause restore), 4 (step order),
//! 5 (interfaces), 3.10/trees 3.14 (autosave cadence 3 s / 6 s);
//! characters 0.1 (visit/plough/shown screen lists, main screen), 8 (routing
//! between actors); snowman-fight 2.1-2.4 (scheduling hand-off); trees 3.1
//! (planting trigger and the 0.5 s x 16 ready poll); README 2.4 (data flow).
//! Decorations are stepped here too (independent of the weather) so the
//! host has one sprite list.
//!
//! The host (`frostpane` app) feeds [`WorldInput`] and applies [`WorldOutput`]:
//! sprites to `fp-layered`, bursts/targets/blade to the `SnowFeed` (done
//! here), prefs writes, card requests to `fp-ui`.
//!
//! Step order each frame while snow is shown: feed snapshot -> ledges,
//! fight, visitors (plough, Santa, yeti, train, gift), train riders, blade
//! hand-over, trees, planting poll, snow absorb, balls, targets.
//!
//! Output rules: [`WorldOutput::sprites`] is replaced on every call (it is
//! the complete list); `prefs` and `cards` are appended to.

mod screens;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use fp_characters::{Cargo, CharacterEvent, Pick, StageScreen, StepCtx, Visitors, quote};
use fp_core::clock::WallTime;
use fp_core::desktop::Layout;
use fp_core::effect::Lang;
use fp_core::feed::{Burst, SnowFeed};
use fp_core::frame::{FIRST_DT, MAX_DT};
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng, mix_seed};
use fp_core::scene::{Owner, PointerInput, Sprite, SpriteKey};
use fp_core::settings::Settings;
use fp_core::surface::ScreenSnap;
use fp_fight::{Fight, FightEvent};
use fp_snowballs::{BallEvent, Balls, Ledges};
use fp_trees::{Grove, TreeEvent};
use serde_json::{Map, Value};

/// Tray/UI commands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    SendPlough,
    SendSanta,
    SendYeti,
    SendTrain,
    SendFight(Option<fp_fight::Variant>),
    PlantTree,
    AddDecoration(fp_decor::Kind),
    RemoveAllDecorations,
    /// Take one decoration down (menu, same path as the X button).
    RemoveDecoration(u64),
    /// Station bubble answers (local train).
    TrainFetch(fp_characters::Pick),
    TrainDismiss,
}

/// UI cards the host should open (rendered by `fp-ui`).
#[derive(Clone, Debug, PartialEq)]
pub enum CardRequest {
    DressUp {
        bottom: u64,
        look: Option<fp_snowman::Look>,
    },
    /// `quip` = cap refusal line 1 or 2 (trees 3.1).
    TreeStyle {
        tree: u64,
        look: fp_trees::TreeLook,
        quip: Option<u8>,
    },
    /// Santa gift opened; `quote` = index into `fp_text::quotes(lang)`.
    Quote {
        at: fp_core::geom::Vec2,
        quote: usize,
    },
    /// Cap reached (third tree refusal, or a decoration add at the cap):
    /// show the limit snowman (snowman F9) on `screen`'s work area.
    LimitReached {
        screen: Frame,
    },
    /// Station bubble with the objects that can ride.
    TrainStation {
        at: fp_core::geom::Vec2,
        choices: Vec<(fp_characters::Pick, fp_core::geom::Vec2)>,
    },
}

pub struct WorldInput<'a> {
    pub now: f64,
    pub wall: WallTime,
    pub pointer: PointerInput,
    pub settings: &'a Settings,
    /// All monitors (work areas for cards, nearest-monitor lookups; misc 4).
    pub layout: &'a Layout,
    /// True while ShownWeather == snow.
    pub snowing: bool,
    pub main_screen: Option<Frame>,
    /// The open decoration picker in global points (inflated by 12 as an
    /// obstacle for the spot, decorations 3.3).
    pub picker: Option<Frame>,
    /// Qualifying top-level window frames in global points (decorations 3.3);
    /// only read while a decoration add is pending.
    pub windows: &'a [Frame],
}

#[derive(Default)]
pub struct WorldOutput {
    /// The complete sprite list of this call (replaced, not appended).
    pub sprites: Vec<Sprite>,
    /// Pref key -> new value, written by the host in one batch.
    pub prefs: Vec<(&'static str, Value)>,
    pub cards: Vec<CardRequest>,
    /// Snow went off this call: the host closes the dress, quote and station cards.
    pub snow_stopped: bool,
    /// Right-click on a decoration: its id and the cursor (global points).
    pub decor_menu: Option<(u64, fp_core::geom::Vec2)>,
}

/// Minimum spacing between two steps (snowballs 3.1).
const MIN_STEP: f64 = 0.0125;
/// Planting waits for the shelves: every 0.5 s, at most 16 tries (trees 3.1).
const PLANT_TRIES: u8 = 16;
const PLANT_EVERY: f64 = 0.5;
/// Narrowest dock a fight stands on (snowman-fight 2.4).
const NARROWEST_DOCK: f64 = 280.0;
/// Fight range without a dock (snowman-fight 2.4).
const BARE_HALF_RANGE: f64 = 360.0;
/// Burst area of an opened gift (characters 8).
const GIFT_BURST: f64 = 500.0;
/// A snowman placed beside a tree stands this far to its left (characters 5.3).
const SNOWMAN_BESIDE_TREE: f64 = 70.0;

const KEY_SNOWMEN: &str = "standingSnowmen";
const KEY_TREES: &str = "standingTrees";
const KEY_DECOR: &str = "decorations";
const VISIT_KEYS: [&str; 5] = ["lastSantaVisit", "lastSnowmanFight", "lastGift", "trainQueue", "trainLook"];

const OWNER_BALL: u8 = Owner::Ball as u8;
const OWNER_TREE: u8 = Owner::Tree as u8;
const OWNER_CHARACTER: u8 = Owner::Character as u8;
const OWNER_GIFT: u8 = Owner::Gift as u8;
const OWNER_DECORATION: u8 = Owner::Decoration as u8;
const OWNER_FIGHT: u8 = Owner::Fight as u8;

#[derive(Clone, Copy, Debug)]
struct Plant {
    tries: u8,
    next: f64,
}

/// An object riding the train.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rider {
    pick: Pick,
    /// A copy made for the ride (it vanishes when the train lets go), as
    /// opposed to the original that was lifted off the ground.
    boarded: bool,
}

pub struct World {
    feed: Arc<SnowFeed>,
    /// The prefs the world owns, as last read or written.
    saved: Map<String, Value>,
    rng: Lcg64,
    /// Snow is shown: balls, trees, characters and fights run.
    on: bool,
    started: bool,
    paused: bool,
    snowballs: bool,
    last: Option<f64>,
    snaps: Vec<ScreenSnap>,
    ledges: Ledges,
    balls: Balls,
    grove: Grove,
    decor: fp_decor::Decorations,
    visitors: Visitors,
    fight: Fight,
    /// A requested fight: `Some(variant)`, retried each frame until it starts.
    fight_req: Option<(Option<fp_fight::Variant>, bool)>,
    /// The running bout was requested by hand (no scheduler bookkeeping).
    fight_manual: bool,
    fight_frame: Option<Frame>,
    plant: Option<Plant>,
    adds: Vec<fp_decor::Kind>,
    decor_dirty: bool,
    decor_loaded: bool,
    decor_frames: Vec<Frame>,
    riders: Vec<Rider>,
    /// The train station bubble should be on screen.
    station_open: bool,
}

impl World {
    /// `saved` = prefs map at startup (standingSnowmen, standingTrees,
    /// decorations, lastSantaVisit, lastSnowmanFight, lastGift, trainQueue, trainLook).
    pub fn new(feed: std::sync::Arc<SnowFeed>, saved: &serde_json::Map<String, Value>, seed: u64) -> Self {
        let settings = Settings::from_map(saved).0;
        let mut visitors = Visitors::new(mix_seed(seed ^ 0x5649_5349));
        visitors.load_json(&Value::Object(saved.clone()));
        World {
            feed,
            saved: saved.clone(),
            rng: Lcg64::new(mix_seed(seed)),
            on: false,
            started: false,
            paused: false,
            snowballs: settings.snowballs,
            last: None,
            snaps: Vec::new(),
            ledges: Ledges::default(),
            balls: Balls::new(),
            grove: Grove::new(settings.max_placed),
            decor: fp_decor::Decorations::new(settings.max_placed),
            visitors,
            fight: Fight::default(),
            fight_req: None,
            fight_manual: false,
            fight_frame: None,
            plant: None,
            adds: Vec::new(),
            decor_dirty: false,
            decor_loaded: false,
            decor_frames: Vec::new(),
            riders: Vec::new(),
            station_open: false,
        }
    }

    /// The weather turned off because of the battery rule (snowballs 3.1
    /// `PausedForBattery`): standing snowmen are kept and come back when snow
    /// resumes. Set before the step that sees the weather change.
    pub fn set_paused_for_battery(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Tree whose style card should be open (close the window when this
    /// returns `None` or another id).
    /// Whether the train station bubble should stay open (closed when the train leaves or snow stops).
    pub fn station_open(&self) -> bool {
        self.station_open
    }

    pub fn tree_card(&self) -> Option<u64> {
        self.grove.card_open()
    }

    /// Window origin of the style card of tree `id` on `work` (22 pt shadow
    /// margin included; content size is [`fp_trees::CARD_SIZE`]).
    pub fn tree_card_origin(&self, id: u64, work: Frame) -> Option<Vec2> {
        self.grove.card_origin(id, work)
    }

    /// Monitor anchor of the style card of tree `id` (trees 3.9).
    pub fn tree_card_anchor(&self, id: u64) -> Option<Vec2> {
        self.grove.card_anchor(id)
    }

    /// The user closed the tree style card window.
    pub fn close_tree_card(&mut self) {
        self.grove.close_card();
    }

    /// Final saves before the app exits (trees 3.14 Stop, snowballs 3.10).
    pub fn shutdown(&mut self, out: &mut WorldOutput) {
        if self.on {
            if let Some(v) = self.grove.stop() {
                self.put(out, KEY_TREES, v);
            }
            if let Some(v) = self.balls.save_json() {
                self.put(out, KEY_SNOWMEN, v);
            }
            self.on = false;
        }
        self.put_visitors(out);
        self.put(out, KEY_DECOR, self.decor.save_json());
    }

    pub fn command(&mut self, c: Command, now: f64) {
        match c {
            Command::SendPlough if self.on => self.visitors.send_plough(now),
            Command::SendSanta if self.on => self.visitors.send_santa(now),
            Command::SendYeti if self.on => {
                // Snowmen first; with none, the newest tree is doomed (trees 3.11).
                if self.balls.prey().is_empty() {
                    let _ = self.grove.doom_newest();
                }
                self.visitors.send_yeti(now);
            }
            Command::SendTrain if self.on => self.visitors.summon_train(now),
            Command::SendFight(variant) if self.on => self.fight_req = Some((variant, true)),
            Command::PlantTree => self.plant = Some(Plant { tries: PLANT_TRIES, next: now }),
            Command::AddDecoration(kind) => self.adds.push(kind),
            Command::RemoveAllDecorations => {
                self.decor.remove_all();
                self.decor_dirty = true;
            }
            Command::RemoveDecoration(id) => {
                self.decor.remove(id);
                self.decor_dirty = true;
            }
            Command::TrainFetch(pick) => match self.standing_picks().into_iter().find(|(p, _)| *p == pick) {
                Some((_, foot)) => self.visitors.train_fetch(pick, foot.x, now),
                None => self.visitors.train_dismiss(now),
            },
            Command::TrainDismiss => self.visitors.train_dismiss(now),
            _ => {}
        }
    }

    /// `false` once the snowman of the open dress-up card is gone, carried,
    /// stacked on another ball, or shorter than three (snowballs 3.6).
    pub fn dress_card_valid(&self, bottom: u64) -> bool {
        self.balls.dressable(bottom)
    }

    /// Dress-up / style card results.
    pub fn restyle_snowman(&mut self, bottom: u64, look: Option<fp_snowman::Look>) {
        self.balls.restyle(bottom, look);
    }

    pub fn restyle_tree(&mut self, tree: u64, look: fp_trees::TreeLook) {
        self.grove.restyle(tree, look);
    }

    /// Chop button on the tree style card (trees 3.11): the tree is doomed,
    /// its card closes and the yeti comes for it while snow runs.
    pub fn doom_tree(&mut self, tree: u64) {
        self.grove.doom(tree);
    }

    /// Live decorations (the picker counter and its cap check).
    pub fn decoration_count(&self) -> usize {
        self.decor.len()
    }

    /// True while a picker add waits for the next step (the host then
    /// supplies `WorldInput::windows`).
    pub fn has_pending_add(&self) -> bool {
        !self.adds.is_empty()
    }

    /// The decoration cap (`Settings::max_placed`).
    pub fn decoration_max(&self) -> u32 {
        self.decor.max()
    }

    /// One UI-thread step (snowballs 4). Gated internally to >= 12.5 ms.
    pub fn step(&mut self, input: &WorldInput<'_>, out: &mut WorldOutput) {
        out.sprites.clear();
        let now = input.now;
        // A broken clock (NaN/inf) or a step closer than 12.5 ms (including
        // time going backwards, dt <= 0) only reports the sprites again.
        if !now.is_finite() || self.last.is_some_and(|l| now - l < MIN_STEP) {
            self.collect(out);
            return;
        }
        let dt = self.last.map_or(FIRST_DT, |l| (now - l).min(MAX_DT));
        self.last = Some(now);
        self.sync_settings(input.settings, out);
        if !self.started || input.snowing != self.on {
            self.run(input.snowing, now, out);
        }
        self.decor_step(input, dt, out);
        if self.on {
            self.world_step(input, dt, out);
        } else {
            // Not running: the planting poll still counts its tries, so a
            // request the weather switch never answers gives up (trees 3.1).
            self.poll_plant(now, &mut Vec::new());
        }
        self.collect(out);
    }

    /// Repaint a sprite at a monitor scale.
    pub fn paint(&self, key: SpriteKey, scale: f32) -> Option<fp_paint::tiny_skia::Pixmap> {
        match key.owner_byte() {
            OWNER_BALL => self.balls.paint(key.id(), scale),
            OWNER_TREE => self.grove.paint(key.id(), scale),
            OWNER_CHARACTER | OWNER_GIFT => self.visitors.paint(key, scale),
            OWNER_DECORATION => self.decor.paint(key.id(), scale),
            OWNER_FIGHT => self.fight.paint(scale),
            _ => None,
        }
    }

    // ---- prefs ----

    /// Queue a pref write unless the value is what was last written.
    fn put(&mut self, out: &mut WorldOutput, key: &'static str, v: Value) {
        if self.saved.get(key) != Some(&v) {
            self.saved.insert(key.to_string(), v.clone());
            out.prefs.push((key, v));
        }
    }

    fn put_visitors(&mut self, out: &mut WorldOutput) {
        let v = self.visitors.save_json();
        for key in VISIT_KEYS {
            if let Some(x) = v.get(key) {
                self.put(out, key, x.clone());
            }
        }
    }

    // ---- lifecycle (snowballs 3.1) ----

    /// The `snowballs` setting; turning it off breaks every ball and forgets
    /// the saved snowmen.
    fn sync_settings(&mut self, s: &Settings, out: &mut WorldOutput) {
        self.visitors.set_away(&s.visitors_away);
        self.visitors.set_often(s.often_visits);
        self.visitors.set_behind_windows(s.behind_windows);
        self.grove.set_max(s.max_placed);
        self.decor.set_max(s.max_placed);
        if self.snowballs && !s.snowballs {
            let mut events = Vec::new();
            self.balls.break_all(&mut events);
            self.route_balls(events, out);
            self.put(out, KEY_SNOWMEN, Value::Array(Vec::new()));
        }
        self.snowballs = s.snowballs;
    }

    /// `Run(on)`: stop everything, then start it again when snow is shown.
    fn run(&mut self, on: bool, now: f64, out: &mut WorldOutput) {
        let was_on = self.on;
        self.started = true;
        self.on = on;
        self.visitors.stop();
        self.fight.stop();
        self.fight_frame = None;
        self.fight_req = None;
        self.riders.clear();
        self.station_open = false;
        if !on {
            out.snow_stopped = true;
        }
        if was_on {
            if let Some(v) = self.grove.stop() {
                self.put(out, KEY_TREES, v);
            }
            // Battery pause keeps the snowmen for the resume.
            if self.paused
                && let Some(v) = self.balls.save_json()
            {
                self.put(out, KEY_SNOWMEN, v);
            }
        }
        if !on {
            self.plant = None;
            if !self.paused {
                self.put(out, KEY_SNOWMEN, Value::Array(Vec::new()));
            }
        }
        self.balls.clear();
        self.ledges = Ledges::default();
        // The feed is drained, but the latest screen snapshot (pending, or the
        // one last read) is still the truth, so it is handed back instead of
        // waiting for the tracker's next push.
        let (pending, _) = self.feed.take();
        let known = std::mem::take(&mut self.snaps);
        let latest = pending.or((!known.is_empty()).then_some(known));
        self.feed.reset(on);
        if let Some(screens) = latest {
            self.feed.snow(screens);
        }
        if on {
            self.visitors.start(now);
            let trees = self.saved.get(KEY_TREES).cloned().unwrap_or(Value::Null);
            self.grove.restore(&trees, now);
            if self.snowballs {
                let snowmen = self.saved.get(KEY_SNOWMEN).cloned().unwrap_or(Value::Null);
                self.balls.restore(&snowmen, now);
            }
        }
    }

    // ---- decorations (independent of the weather) ----

    fn decor_step(&mut self, input: &WorldInput<'_>, dt: f64, out: &mut WorldOutput) {
        let frames = screens::monitor_frames(input.layout);
        if !self.decor_loaded {
            self.decor_loaded = true;
            self.decor.set_screens(frames.clone());
            self.decor_frames = frames;
            let saved = self.saved.get(KEY_DECOR).cloned().unwrap_or(Value::Null);
            self.decor.load_json(&saved);
        } else if frames != self.decor_frames {
            self.decor_dirty |= self.decor.set_screens(frames.clone());
            self.decor_frames = frames;
        }
        for kind in std::mem::take(&mut self.adds) {
            self.add_decoration(kind, input, out);
        }
        let changed = self.decor.step(dt, input.now, &input.pointer);
        out.decor_menu = self.decor.take_menu_request();
        if changed || self.decor_dirty {
            self.decor_dirty = false;
            self.put(out, KEY_DECOR, self.decor.save_json());
        }
    }

    /// Picker add (decorations 3.3): a free spot in the work area, or the
    /// limit snowman at the cap.
    fn add_decoration(&mut self, kind: fp_decor::Kind, input: &WorldInput<'_>, out: &mut WorldOutput) {
        let under = input.pointer.pos.and_then(|p| input.layout.at_point(p)).map(|s| s.work_points);
        let Some(work) = under.or_else(|| screens::work_area(input.layout, input.main_screen)) else { return };
        if self.decor.is_full() {
            out.cards.push(CardRequest::LimitReached { screen: work });
            return;
        }
        let mut obstacles =
            if input.windows.is_empty() { screens::window_frames(&self.snaps) } else { input.windows.to_vec() };
        obstacles.extend(input.picker.map(|p| p.inset(-12.0, -12.0)));
        let spot = self.decor.spot(kind, work, &obstacles, &mut self.rng);
        self.decor_dirty |= self.decor.add(kind, spot).is_some();
    }

    // ---- the world step (snowballs 4) ----

    fn world_step(&mut self, input: &WorldInput<'_>, dt: f64, out: &mut WorldOutput) {
        let now = input.now;
        let (snap, gathered) = self.feed.take();
        if let Some(s) = snap {
            self.snaps = s;
            self.ledges.read(&self.snaps, now);
        }
        let all = screens::stage_screens(&self.snaps);
        let (visit, plough, shown) = screens::screen_lists(&all, input.main_screen);
        for s in &self.snaps {
            if let Some(d) = self.feed.depth(s.frame.min()) {
                self.visitors.report_depth(s.frame, d);
            }
        }
        self.start_fight(&visit, input.wall, out);
        let mut fight_events = Vec::new();
        self.step_fight(dt, now, &visit, &mut fight_events);
        for e in fight_events {
            match e {
                FightEvent::Burst { at, area } => self.burst_at(at, area, Vec2::ZERO),
                FightEvent::Finished => self.end_bout(now),
            }
        }

        let prey = self.balls.prey();
        let doomed = self.grove.doomed();
        let obstacles = fight_blocks(self.fight_frame, self.fight.floor(), &self.fight.obstacles());
        let ctx = StepCtx {
            now,
            dt,
            wall: input.wall,
            visit_screens: &visit,
            plough_screens: &plough,
            shown_screens: &shown,
            pointer: &input.pointer,
        };
        let mut events = Vec::new();
        self.visitors.set_fight_active(self.fight_frame.is_some());
        self.visitors.step(&ctx, &prey, doomed, &obstacles, &mut events);
        let mut ball_events = Vec::new();
        self.route_characters(events, &mut ball_events, input.settings.language, out);
        self.update_riders();
        let blade = self.visitors.blade();
        self.feed.publish_blade(blade);
        if let Some((origin, b)) = blade {
            self.balls.shove_run(b.front, b.way, origin, 1.0, b.on_dock);
        }

        let mut tree_events = Vec::new();
        self.grove.step(dt, now, &self.snaps, &input.pointer, true, &mut tree_events);
        self.poll_plant(now, &mut tree_events);
        self.route_trees(tree_events, input.layout, out);

        let dragging = self.decor.busy();
        self.balls.absorb(&gathered, &self.ledges, now, dragging);
        self.balls.step(dt, now, &self.ledges, &input.pointer, &mut ball_events);
        self.route_balls(ball_events, out);
        let open =
            self.snowballs && !dragging && !self.balls.dragging() && self.balls.count() < fp_snowballs::MAX_BALLS;
        self.feed.publish_targets(self.balls.targets(), open);
    }

    // ---- fights (snowman-fight 2.2-2.4) ----

    /// Begin the requested fight when a screen, a dock and a plough-free
    /// range allow it; otherwise the request stays for the next frame.
    fn start_fight(&mut self, visit: &[StageScreen], wall: WallTime, out: &mut WorldOutput) {
        let Some((variant, manual)) = self.fight_req else { return };
        if self.fight.active() {
            return;
        }
        let wide = |s: &StageScreen| s.dock.is_some_and(|d| d.1 - d.0 >= NARROWEST_DOCK);
        let pick = visit.iter().find(|s| s.shown && wide(s)).or_else(|| visit.iter().find(|s| s.shown));
        let Some(s) = pick else { return };
        let (min, max, floor) = match s.dock.filter(|d| d.1 - d.0 >= NARROWEST_DOCK) {
            Some((a, b, top)) => (a, b, top - 1.0),
            None => (s.frame.mid_x() - BARE_HALF_RANGE, s.frame.mid_x() + BARE_HALF_RANGE, s.frame.min_y()),
        };
        let stage = Frame::new(min, s.frame.min_y(), max - min, s.frame.h);
        if self.visitors.plough_front(s.frame).is_some_and(|front| Fight::plough_veto(stage, front)) {
            return;
        }
        if self.fight.begin(variant, stage, floor, self.rng.next_u64()) {
            self.fight_req = None;
            self.fight_frame = Some(s.frame);
            self.fight_manual = manual;
            if !manual {
                self.visitors.fight_came(wall);
                self.put_visitors(out);
            }
        }
    }

    fn step_fight(&mut self, dt: f64, now: f64, visit: &[StageScreen], events: &mut Vec<FightEvent>) {
        let Some(frame) = self.fight_frame else { return };
        match visit.iter().find(|s| s.frame == frame) {
            Some(s) => {
                self.fight.set_shown(s.shown);
                let plough = self.visitors.plough_front(frame);
                self.fight.step(dt, plough, events);
            }
            None => {
                self.fight.stop();
                self.end_bout(now);
            }
        }
    }

    /// A bout ended (finished or its screen vanished): a non-manual one
    /// re-rolls the wait from now (snowman-fight 2.3).
    fn end_bout(&mut self, now: f64) {
        self.fight_frame = None;
        if !self.fight_manual {
            self.visitors.fight_ended(now);
        }
    }

    // ---- planting (trees 3.1) ----

    fn poll_plant(&mut self, now: f64, events: &mut Vec<TreeEvent>) {
        let Some(p) = self.plant.filter(|p| now >= p.next) else { return };
        // Ready = running and at least one shelf known.
        self.plant = if self.on && !self.snaps.is_empty() {
            let seed = self.rng.next_u64();
            self.grove.plant(now, seed, events);
            None
        } else if p.tries <= 1 {
            None
        } else {
            Some(Plant { tries: p.tries - 1, next: now + PLANT_EVERY })
        };
    }

    // ---- event routing ----

    /// Snow burst at a global point, on the screen nearest to it.
    fn burst_at(&mut self, at: Vec2, area: f64, velocity: Vec2) {
        if let Some(frame) = screens::nearest_frame(&self.snaps, at) {
            let origin = frame.min();
            self.feed.publish_burst(origin, Burst { point: at - origin, area, velocity });
        }
    }

    fn route_balls(&mut self, events: Vec<BallEvent>, out: &mut WorldOutput) {
        for e in events {
            match e {
                BallEvent::Burst { origin, burst } => self.feed.publish_burst(origin, burst),
                BallEvent::Dress { bottom } => {
                    let look = self.balls.look(bottom);
                    out.cards.push(CardRequest::DressUp { bottom, look });
                }
                BallEvent::Dirty => {
                    if let Some(v) = self.balls.save_json() {
                        self.put(out, KEY_SNOWMEN, v);
                    }
                }
            }
        }
    }

    fn route_trees(&mut self, events: Vec<TreeEvent>, layout: &Layout, out: &mut WorldOutput) {
        for e in events {
            match e {
                TreeEvent::Style { id, quip } => {
                    let look = self.grove.look(id).cloned().unwrap_or_default();
                    out.cards.push(CardRequest::TreeStyle { tree: id, look, quip });
                }
                TreeEvent::Snow { at, area, velocity } => self.burst_at(at, area, velocity),
                TreeEvent::LimitReached => {
                    if let Some(screen) = screens::primary_work(layout) {
                        out.cards.push(CardRequest::LimitReached { screen });
                    }
                }
                TreeEvent::Dirty => {
                    if let Some(v) = self.grove.save_json() {
                        self.put(out, KEY_TREES, v);
                    }
                }
            }
        }
    }

    fn route_characters(
        &mut self,
        events: Vec<CharacterEvent>,
        ball_events: &mut Vec<BallEvent>,
        language: Option<Lang>,
        out: &mut WorldOutput,
    ) {
        for e in events {
            match e {
                CharacterEvent::Burst { at, area, velocity } => self.burst_at(at, area, velocity),
                CharacterEvent::GiftDropped { .. } => {}
                CharacterEvent::GiftOpened { at } => {
                    self.burst_at(at, GIFT_BURST, Vec2::ZERO);
                    self.quote_card(at, language.unwrap_or(Lang::En), out);
                }
                CharacterEvent::Smash { bottom } => self.balls.smash(bottom, ball_events),
                CharacterEvent::Chop { tree } => self.chop(tree),
                CharacterEvent::TrainReady { at } => {
                    self.station_open = true;
                    out.cards.push(CardRequest::TrainStation { at, choices: self.standing_picks() });
                }
                CharacterEvent::Took { pick } => self.riders.push(Rider { pick, boarded: false }),
                CharacterEvent::Loaded { pick } => self.loaded(pick),
                CharacterEvent::Unload { cargo, x, screen } => self.unload(&cargo, x, screen),
                CharacterEvent::Left => {
                    self.station_open = false;
                    self.release_riders();
                }
                CharacterEvent::FightDue => {
                    if self.fight_req.is_none() {
                        self.fight_req = Some((None, false));
                    }
                }
                CharacterEvent::Dirty => self.put_visitors(out),
            }
        }
    }

    /// One yeti axe blow: the tree falls away from the yeti.
    fn chop(&mut self, tree: u64) {
        let mut sprites = Vec::new();
        self.visitors.sprites(&mut sprites);
        let yeti = SpriteKey::new(Owner::Character, 3);
        match sprites.iter().find(|s| s.key == yeti) {
            Some(s) => self.grove.chop_from(tree, s.frame.mid_x()),
            None => self.grove.chop(tree),
        }
    }

    /// Next quote of the language's shuffled deck (ui 3.9); the deck is saved.
    fn quote_card(&mut self, at: Vec2, lang: Lang, out: &mut WorldOutput) {
        let key = quote::key(lang);
        let left: Vec<i64> = self
            .saved
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_i64).collect())
            .unwrap_or_default();
        let (index, deck) = quote::next(&left, fp_text::quotes(lang).len(), &mut self.rng);
        self.put(out, key, Value::from(deck));
        out.cards.push(CardRequest::Quote { at, quote: index });
    }

    // ---- train cargo hand-over (characters 5.3, 8) ----

    /// Standing snowmen and trees that can ride, with their foot points.
    fn standing_picks(&self) -> Vec<(Pick, Vec2)> {
        let mut picks: Vec<(Pick, Vec2)> =
            self.balls.standing_snowmen().into_iter().map(|(id, foot)| (Pick::Snowman(id), foot)).collect();
        picks.extend(self.grove.standing().into_iter().map(|(id, foot)| (Pick::Tree(id), foot)));
        picks
    }

    fn carry(&mut self, pick: Pick, at: Option<Vec2>) {
        match (pick, at) {
            (Pick::Snowman(id), at) => self.balls.carry(id, at),
            (Pick::Tree(id), Some(p)) => self.grove.carry(Some(id), Some(p)),
            (Pick::Tree(_), None) => self.grove.carry(None, None),
        }
    }

    fn vanish(&mut self, pick: Pick) {
        match pick {
            Pick::Snowman(id) => self.balls.vanish(id),
            Pick::Tree(id) => self.grove.vanish(id),
        }
    }

    /// The picked object landed on the car: take it out of its owner, hand
    /// the cargo to the train and keep a copy riding until the train leaves.
    fn loaded(&mut self, pick: Pick) {
        self.riders.retain(|r| r.pick != pick);
        let mut cargo = Cargo::default();
        match pick {
            Pick::Snowman(id) => cargo.snowman = self.balls.take(id),
            Pick::Tree(id) => cargo.tree = self.grove.take(id),
        }
        if cargo.is_empty() {
            return;
        }
        self.visitors.load(cargo.clone());
        if let Some(p) = self.visitors.carry_point() {
            self.board(&cargo, p);
        }
    }

    /// Create the carried copies of a cargo at `p`.
    fn board(&mut self, cargo: &Cargo, p: Vec2) {
        if let Some(id) = cargo.snowman.as_ref().and_then(|v| self.balls.board(v, p)) {
            self.riders.push(Rider { pick: Pick::Snowman(id), boarded: true });
        }
        if let Some(id) = cargo.tree.as_ref().and_then(|v| self.grove.board(v, p)) {
            self.riders.push(Rider { pick: Pick::Tree(id), boarded: true });
        }
    }

    /// Keep the riders at the train's carry point; without one, copies vanish
    /// and lifted originals are let go (they fall).
    fn update_riders(&mut self) {
        let point = self.visitors.carry_point();
        if self.riders.is_empty()
            && let (Some(p), Some(cargo)) = (point, self.visitors.cargo_aboard().cloned())
        {
            self.board(&cargo, p);
        }
        for r in std::mem::take(&mut self.riders) {
            match (point, r.boarded) {
                (Some(p), _) => {
                    self.carry(r.pick, Some(p));
                    self.riders.push(r);
                }
                (None, true) => self.vanish(r.pick),
                (None, false) => self.carry(r.pick, None),
            }
        }
    }

    fn release_riders(&mut self) {
        for r in std::mem::take(&mut self.riders) {
            if r.boarded {
                self.vanish(r.pick);
            } else {
                self.carry(r.pick, None);
            }
        }
    }

    /// A delivery run reached its placement frame (characters 5.3).
    fn unload(&mut self, cargo: &Cargo, x: f64, screen: Frame) {
        let both = cargo.tree.is_some() && cargo.snowman.is_some();
        if let Some(v) = &cargo.snowman {
            let at = if both { x - SNOWMAN_BESIDE_TREE } else { x };
            self.balls.stand(v, at, screen, &self.ledges);
        }
        if let Some(v) = &cargo.tree {
            self.grove.plant_cargo(v, x, screen);
        }
    }

    // ---- output ----

    fn collect(&self, out: &mut WorldOutput) {
        self.decor.sprites(&mut out.sprites);
        if self.on {
            self.grove.sprites(&mut out.sprites);
            self.balls.sprites(&mut out.sprites);
            self.fight.sprites(&mut out.sprites);
            self.visitors.sprites(&mut out.sprites);
        }
    }
}

/// Fighters as Santa hop rectangles on the bout's own screen and floor.
fn fight_blocks(frame: Option<Frame>, floor: Option<f64>, fighters: &[(f64, f64, f64)]) -> Vec<fp_characters::Block> {
    let (Some(frame), Some(floor)) = (frame, floor) else { return Vec::new() };
    fighters.iter().map(|&(x, half, h)| (frame, x - half, x + half, floor, floor + h)).collect()
}
