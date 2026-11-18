//! The app state and its wiring: everything that is not the eframe loop.
//!
//! One [`Core`] lives on the UI thread. The driver timer (about 64 Hz, a
//! hidden Win32 window of `fp-win`) calls [`Core::tick`], so the tracker, the
//! world and the object windows keep running while the main window is
//! hidden. The eframe loop calls [`Core::logic`] and [`Core::ui`].

use crate::args::Args;
use crate::factory;
use crate::outside::{self, OutsideWatch};
use crate::react::{Reaction, react};
use crate::sources::{Event, Sources, UpdateOutcome};
use crate::wallpaper::{self, Dirs, Wallpaper};
use fp_core::clock::{Clock, WallTime};
use fp_core::desktop::{Layout, MonitorInfo};
use fp_core::effect::{EffectKind, Lang};
use fp_core::feed::SnowFeed;
use fp_core::geom::PixelRect;
use fp_core::geom::{Frame, Vec2};
use fp_core::prefs::Prefs;
use fp_core::rng::{Lcg64, mix_seed};
use fp_core::scene::{Owner, PointerInput, SpriteKey};
use fp_core::settings::{Changes, Settings, Weathers};
use fp_core::surface::ScreenSnap;
use fp_layered::LayeredHost;
use fp_net::Worker;
use fp_overlay::OverlayHost;
use fp_sources::season;
use fp_sources::update;
use fp_tracker::{HookEvent, PollInput, Tracker, select_screens};
use fp_ui::eframe::egui;
use fp_ui::model::DisplayInfo;
use fp_ui::{CardAction, CardCtx, Cards, HostInfo, Shell, Tray, TrayAction, UiAction, UpdateLine};
use fp_win::windows_impl::{HookSet, SystemWatcher, Win};
use fp_win::{Platform, Shell as Os, SleepState, SystemClock, SystemEvent};
use fp_world::{CardRequest, Command, World, WorldInput, WorldOutput};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, atomic::AtomicBool, atomic::Ordering};
use std::time::{Duration, Instant};

const KEY_UPDATE_ASKED: &str = "updateAskedAt";
const KEY_SEEN_CHANGELOG: &str = "seenChangelog";
/// Power state is re-read every 5 s (desktop-native 3.7).
const POWER_EVERY_S: f64 = 5.0;
/// Overlay and object windows are re-raised once a second (desktop-native 3.4).
const KEEP_EVERY_S: f64 = 1.0;
/// Taskbar shelves are re-read every second on a background thread (desktop-native 3.5.2, 3.5.6).
const SHELVES_EVERY_S: f64 = 1.0;
/// Hover poll when the cursor is not near a sprite (every 100 ms).
const HOVER_EVERY_S: f64 = 0.1;
/// A quit that the window loop has not completed after this long is forced.
const FORCE_EXIT_S: f64 = 3.0;
/// egui pass cadence while the main window or a card shows.
const UI_REPAINT: Duration = Duration::from_millis(100);
/// egui pass cadence while nothing of egui is on screen. Everything that
/// changes then (tray, second launch, system events, cards opening) asks for
/// a pass itself; this is only the safety net for `--seconds` without the
/// driver timer. Each pass costs a few milliseconds, so 10 Hz while hidden
/// was most of the idle CPU.
const HIDDEN_REPAINT: Duration = Duration::from_secs(1);

/// How soon the next egui pass is needed.
fn repaint_every(visible: bool, cards_open: bool) -> Duration {
    if visible || cards_open { UI_REPAINT } else { HIDDEN_REPAINT }
}

/// Everything `Core::new` needs from `main`.
pub struct Config {
    pub args: Args,
    pub data_dir: PathBuf,
    pub prefs: Prefs,
    pub open_request: Arc<AtomicBool>,
    /// Open the decoration picker shortly after start (smoke runs).
    pub demo_cards: bool,
}

/// Events from the system watcher, drained by the next tick.
type SystemQueue = Rc<RefCell<Vec<(SystemEvent, Option<bool>)>>>;

/// What the overlay was last told, to push only differences.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pushed {
    wind: Option<fp_core::wind::Wind>,
    keep_clear: Option<bool>,
    behind: Option<bool>,
    share: Option<f64>,
    lightning: Option<bool>,
}

/// `(month, south)` for a season pick or a forecast mapping.
pub fn season_env(settings: &Settings, os: &dyn Os, tz_id: &str, wall: WallTime) -> (u32, bool) {
    let city = settings.city.as_ref().map(|c| c.latitude);
    let region = if city.is_none() { os.region_latitude() } else { None };
    (wall.local_date().month, season::is_south(city, tz_id, region))
}

pub struct Core {
    ctx: egui::Context,
    args: Args,
    started: Instant,
    platform: Win,
    data_dir: PathBuf,
    prefs: Prefs,
    weathers: Weathers,
    lang: Lang,
    clock: SystemClock,
    tray: Option<Tray>,
    shell: Shell,
    cards: Cards,
    overlay: Option<OverlayHost>,
    overlay_failed: bool,
    layered: LayeredHost,
    feed: Arc<SnowFeed>,
    world: World,
    out: WorldOutput,
    tracker: Tracker,
    monitors: Vec<MonitorInfo>,
    layout: Layout,
    main_frame: Option<Frame>,
    sources: Sources,
    wallpaper: Wallpaper,
    rng: Lcg64,
    seed: u64,
    hooks_rx: Receiver<HookEvent>,
    /// Generated wallpaper packs are being drawn; the count arrives when done.
    packs_rx: Option<Receiver<usize>>,
    hooks: Option<HookSet>,
    system: SystemQueue,
    watcher: Option<SystemWatcher>,
    sleep: SleepState,
    open_request: Arc<AtomicBool>,
    visible: bool,
    /// Frames seen so far, up to [`STARTUP_HIDE_FRAMES`].
    startup_frames: u8,
    minimized: bool,
    /// Last seen window focus, for the activation edge.
    focused: bool,
    quitting: Option<f64>,
    shown: Option<EffectKind>,
    pushed: Pushed,
    reduce_motion: bool,
    /// A rebuild is queued; runs once per tick (desktop-native 3.6 debounce).
    rebuild_pending: bool,
    saving: bool,
    corner: f64,
    force_poll: bool,
    next_power: f64,
    shelves: Shelves,
    next_keep: f64,
    next_hover: f64,
    next_daynight: f64,
    cursor: Option<(i32, i32)>,
    outside: OutsideWatch,
    hot: Option<SpriteKey>,
    demo_at: Option<f64>,
    deadline: Option<Instant>,
    /// Last overlay statistics, kept after the overlay is dropped.
    last_panels: usize,
    last_frames: u64,
    pub ticks: u64,
}

/// Display list for the settings screen picker. A monitor without a
/// friendly name gets fp-win's English "Screen N" fallback; that one is
/// shown in the UI language instead.
/// Frames during which a quiet start keeps re-hiding the main window.
const STARTUP_HIDE_FRAMES: u8 = 8;

/// Applies the result of a launch-at-login change (core-app 2.10): a failed
/// enable shows the note; only a user block opens Windows Startup Apps.
fn login_result<O: Os>(os: &O, shell: &mut Shell, on: bool, ok: bool) {
    if on && !ok {
        let blocked = os.login_blocked();
        shell.login_failed(blocked);
        if blocked {
            os.show_startup_apps();
        }
    } else {
        shell.login_state(os.login_is_on());
    }
}

/// Tray pointer decision (core-app 2.6/2.7): the pointer opens only when the
/// icon rect lies inside the taskbar window; the result is true only when it
/// opened and Windows has the icon promoted (otherwise the overflow warning
/// must show).
fn tray_pointer<O: Os>(os: &O, rect: Option<[f64; 4]>, open: impl FnOnce([f64; 4]) -> bool) -> bool {
    let promoted = os.tray_icon_promoted();
    let Some(rect) = rect else { return false };
    let px = PixelRect::new(
        rect[0].round() as i32,
        rect[1].round() as i32,
        (rect[0] + rect[2]).round() as i32,
        (rect[1] + rect[3]).round() as i32,
    );
    if os.tray_icon_spot(px).is_none() {
        return false;
    }
    open(rect) && promoted
}

/// Re-reads the OS login state on window activation while the blocked note
/// shows (ui 3.5 Hooks).
fn recheck_login<O: Os>(os: &O, shell: &mut Shell, activated: bool) {
    if activated && shell.login_recheck_wanted() {
        shell.login_state(os.login_is_on());
    }
}

/// Desktop-native 3.5 step 5: every tracker poll hands its per-screen
/// snapshot to the snow watcher, so the world reads its ledges (dock, ground,
/// windows) from it (snowballs 3.4, 4). Without it the world has no ledges
/// and new balls fall through the dock and off the screen.
/// `FROSTPANE_LOG=debug` prints the snapshot (target `frostpane::ledges`).
/// Closes the station bubble once the world no longer has a train waiting, and
/// the dress, quote and station cards when snow went off.
fn sync_station(cards: &mut Cards, world_station_open: bool, snow_stopped: bool) {
    if snow_stopped {
        cards.close_snow_cards();
    } else if cards.station_open() && !world_station_open {
        cards.close_station();
    }
}

/// Work area for a tree style card (trees 3.9): the monitor nearest the
/// tree's ground-strip anchor, else the primary monitor's.
fn tree_card_work(layout: &Layout, anchor: Option<fp_core::geom::Vec2>) -> Option<fp_core::geom::Frame> {
    match anchor {
        Some(p) => fp_ui::cardmodel::work_at(layout, p),
        None => layout.primary().map(|s| s.work_points),
    }
}

/// Close the tree and dress-up cards whose subject the world dropped
/// (snowballs 3.6 KeepDressCard, trees card).
fn close_stale_cards(cards: &mut Cards, world: &World) {
    if cards.tree_open().is_some() && cards.tree_open() != world.tree_card() {
        cards.close_tree();
    }
    if let Some(b) = cards.dress_open()
        && !world.dress_card_valid(b)
    {
        cards.close_dress();
    }
    sync_picker_counts(cards, world);
}

/// Keep the open decoration picker's count and cap live (decorations 3.1):
/// adds, remove-all, a decoration's own remove button and a cap change all
/// show up on the next step.
fn sync_picker_counts(cards: &mut Cards, world: &World) {
    if cards.picker_open() {
        cards.set_decor_counts(world.decoration_count(), world.decoration_max() as usize);
    }
}

/// A day-and-night step stopped the cycle: the UI must not show it running.
fn tell_cycle_step(shell: &mut Shell, action: &fp_sources::daynight::Action) {
    if matches!(action, fp_sources::daynight::Action::Stop) {
        shell.cycle_stopped();
    }
}

/// Report a Wallpapers-tab action to the UI. A failed turn-on also stops
/// the old cycle (wallpaper.rs), so its ring must go too.
fn tell_wallpaper_run(shell: &mut Shell, a: &fp_ui::wallpapers::WallAction, ok: bool, now: f64) {
    shell.wallpaper_result(Some(a.picked_id()), ok, now);
    if !ok && matches!(a, fp_ui::wallpapers::WallAction::TurnOn { .. }) {
        shell.cycle_stopped();
    }
}

/// Right-click menu of one decoration (decorations 3.6): a fresh one-item
/// popup at the cursor; choosing it takes the decoration down.
fn decor_menu_command(platform: &dyn Platform, lang: Lang, cursor: Option<(i32, i32)>, id: u64) -> Option<Command> {
    decor_menu_with(|label, at| platform.context_menu(label, at), lang, cursor, id)
}

fn decor_menu_with(
    show: impl FnOnce(&str, (i32, i32)) -> bool,
    lang: Lang,
    cursor: Option<(i32, i32)>,
    id: u64,
) -> Option<Command> {
    show(fp_text::t(lang, fp_text::Key::DecorTakeDown), cursor.unwrap_or((0, 0)))
        .then_some(Command::RemoveDecoration(id))
}

fn publish_screens(feed: &SnowFeed, screens: &[ScreenSnap]) {
    if log::log_enabled!(target: "frostpane::ledges", log::Level::Debug) {
        for s in screens {
            log::debug!(target: "frostpane::ledges", "screen {:?} shown {} ground {}", s.frame, s.shown, s.ground);
            for f in &s.surfaces {
                log::debug!(target: "frostpane::ledges", "  {:?} {:?} blockers {:?}", f.id, f.frame, f.blockers);
            }
        }
    }
    feed.snow(screens.to_vec());
}

/// Qualifying top-level windows as global-point frames (decorations 3.3):
/// the tracker filter, then DWM bounds converted from pixels.
fn window_obstacles(layout: &Layout, windows: &[fp_tracker::WindowInfo]) -> Vec<Frame> {
    windows
        .iter()
        .filter_map(fp_tracker::filter)
        .map(|l| {
            let b = l.bounds;
            let a = layout.to_points(f64::from(b.left), f64::from(b.bottom));
            let c = layout.to_points(f64::from(b.right), f64::from(b.top));
            Frame::new(a.x.min(c.x), a.y.min(c.y), (c.x - a.x).abs(), (c.y - a.y).abs())
        })
        .collect()
}

/// Taskbar shelves, read on a background thread once a second. The list
/// survives display rebuilds (desktop-native 3.6); a poll that starts a read
/// keeps using the previous list (3.5.2).
struct Shelves {
    list: Vec<PixelRect>,
    next: f64,
    rx: Option<Receiver<Vec<PixelRect>>>,
}

impl Shelves {
    fn new() -> Self {
        Self { list: Vec::new(), next: -10.0, rx: None }
    }

    /// Take a finished read, then start a new one when due.
    fn step(&mut self, now: f64, read: impl FnOnce() -> Vec<PixelRect> + Send + 'static) {
        if let Some(rx) = &self.rx
            && let Ok(v) = rx.try_recv()
        {
            self.list = v;
            self.rx = None;
        }
        if now >= self.next && self.rx.is_none() {
            self.next = now + SHELVES_EVERY_S;
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(read());
            });
            self.rx = Some(rx);
        }
    }
}

/// True once the background pack drawing has finished (receiver is dropped).
fn packs_done(rx: &mut Option<Receiver<usize>>) -> bool {
    let Some(r) = rx else { return false };
    match r.try_recv() {
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
        _ => {
            *rx = None;
            true
        }
    }
}

/// What one system event asks the core to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SystemActions {
    pause: bool,
    wake: bool,
    resume: bool,
    rebuild: bool,
    theme: bool,
    power: bool,
}

/// Maps one event to actions. Only a real `Resume` restarts the match and
/// season timers (weather-sources 2.3/2.5); unlock or display-on only wakes.
fn system_actions(sleep: &mut SleepState, e: SystemEvent) -> SystemActions {
    let mut a = SystemActions::default();
    match sleep.apply(e) {
        Some(true) => a.pause = true,
        Some(false) => a.wake = true,
        None => {}
    }
    match e {
        SystemEvent::Resume => a.resume = true,
        SystemEvent::DisplaysChanged => a.rebuild = true,
        SystemEvent::ThemeChanged => a.theme = true,
        SystemEvent::PowerChanged => a.power = true,
        _ => {}
    }
    a
}

/// Debounce: queued display events and a DPI change give one rebuild.
fn take_rebuild(pending: &mut bool, rescaled: bool) -> bool {
    *pending |= rescaled;
    std::mem::take(pending)
}

/// Reads reduced motion now and builds the factory with it (once per build).
fn motion_factory(
    platform: &dyn Platform,
    kind: EffectKind,
    feed: Arc<SnowFeed>,
    seed: u64,
) -> (bool, fp_overlay::EffectFactory) {
    let rm = platform.reduced_motion();
    (rm, factory::factory(kind, feed, rm, seed))
}

fn displays(monitors: &[MonitorInfo], lang: Lang) -> Vec<DisplayInfo> {
    monitors
        .iter()
        .map(|m| {
            let n = m.name.strip_prefix("Screen ").and_then(|n| n.parse::<usize>().ok());
            let name = n.map_or_else(|| m.name.clone(), |n| fp_text::screen_number(lang, n));
            DisplayInfo { id: m.id.clone(), name }
        })
        .collect()
}

/// Work areas in logical points, primary first (matches the startup sizing).
fn work_areas(monitors: &[MonitorInfo]) -> Vec<egui::Rect> {
    let mut sorted: Vec<&MonitorInfo> = monitors.iter().collect();
    sorted.sort_by_key(|m| !m.primary);
    sorted
        .iter()
        .map(|m| {
            let s = m.scale.max(0.1);
            let p = |x: i32, y: i32| egui::pos2((f64::from(x) / s) as f32, (f64::from(y) / s) as f32);
            egui::Rect::from_min_max(p(m.work.left, m.work.top), p(m.work.right, m.work.bottom))
        })
        .collect()
}

impl Core {
    /// Startup order (core-app 2.1 adapted): settings are already loaded by
    /// `main`; here come the language, world, tray, hooks and watchers,
    /// sources, day-and-night, and the overlay when a weather is shown.
    pub fn new(ctx: egui::Context, cfg: Config, quiet_start: bool, lang: Lang, settings: Settings) -> Self {
        let platform = Win;
        let power = platform.power();
        let monitors = platform.monitors();
        let layout = Layout::new(monitors.clone());
        let weathers = Weathers::new(settings, power.on_battery);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64 ^ u64::from(std::process::id()));
        let feed = Arc::new(SnowFeed::new());
        let world = World::new(feed.clone(), cfg.prefs.map(), mix_seed(seed));
        let clock = SystemClock::default();
        let now = clock.now();

        let packs_wake = ctx.clone();
        let packs_rx =
            crate::packs::spawn_ensure(&cfg.data_dir.join("Wallpapers"), move || packs_wake.request_repaint());
        let (hook_tx, hooks_rx) = channel();
        let hooks = HookSet::install(hook_tx);
        let system = Rc::new(RefCell::new(Vec::new()));
        let sink = system.clone();
        let wake = ctx.clone();
        let watcher = SystemWatcher::new(move |e, asleep| {
            sink.borrow_mut().push((e, asleep));
            wake.request_repaint();
        });

        let worker_wake = ctx.clone();
        let sources =
            Sources::new(Worker::spawn(move || worker_wake.request_repaint()), now, env!("CARGO_PKG_VERSION"));

        let mut core = Self {
            args: cfg.args,
            started: Instant::now(),
            data_dir: cfg.data_dir,
            prefs: cfg.prefs,
            weathers,
            lang,
            clock,
            tray: None,
            shell: Shell::new(HostInfo::default()),
            cards: Cards::new(seed),
            overlay: None,
            overlay_failed: false,
            layered: LayeredHost::new(),
            feed,
            world,
            out: WorldOutput::default(),
            tracker: Tracker::new(),
            monitors,
            layout,
            main_frame: None,
            sources,
            wallpaper: Wallpaper::default(),
            rng: Lcg64::new(mix_seed(seed ^ 0x5EED)),
            seed,
            hooks_rx,
            packs_rx,
            hooks: (hooks.count() > 0).then_some(hooks),
            system,
            watcher,
            sleep: SleepState::default(),
            open_request: cfg.open_request,
            visible: !quiet_start,
            startup_frames: 0,
            minimized: false,
            focused: false,
            quitting: None,
            shown: None,
            pushed: Pushed::default(),
            reduce_motion: platform.reduced_motion(),
            rebuild_pending: false,
            saving: power.saving,
            corner: platform.window_corner(),
            force_poll: true,
            next_power: now + POWER_EVERY_S,
            shelves: Shelves::new(),
            next_keep: now + KEEP_EVERY_S,
            next_hover: now,
            next_daynight: now,
            cursor: None,
            outside: OutsideWatch::default(),
            hot: None,
            demo_at: cfg.demo_cards.then_some(now + 1.0),
            deadline: crate::args::deadline(cfg.args.seconds, Instant::now()),
            last_panels: 0,
            last_frames: 0,
            ticks: 0,
            platform,
            ctx: ctx.clone(),
        };
        core.shell = Shell::new(core.host_info());
        core.shell.set_reduce_motion(core.reduce_motion);
        let wake = ctx.clone();
        match Tray::new(&core.weathers, lang, move || wake.request_repaint()) {
            Ok(mut t) => {
                t.set_light_taskbar(Os::taskbar_light(&core.platform));
                if let Err(e) = t.refresh(&core.weathers, lang) {
                    log::warn!("tray refresh failed: {e}");
                }
                core.tray = Some(t);
            }
            Err(e) => log::warn!("tray unavailable: {e}"),
        }
        core.recompute_main_frame();
        core.sources.sync(&core.weathers.settings, now);
        core.sync_effect();
        core
    }

    pub fn host_info(&self) -> HostInfo {
        HostInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            has_battery: self.platform.power().has_battery,
            displays: displays(&self.monitors, self.lang),
            wallpapers_dir: self.data_dir.join("Wallpapers"),
            day_pack: self.prefs.get_str(wallpaper::KEY_PACK).map(str::to_string),
            login_on: Os::login_is_on(&self.platform),
            seen_changelog: self.prefs.get_str(KEY_SEEN_CHANGELOG).map(str::to_string),
            custom_chrome: true,
            decoder: Some(Arc::new(crate::decode::decode)),
            work_areas: work_areas(&self.monitors),
            ..HostInfo::default()
        }
    }

    pub fn seen_welcome(&self) -> bool {
        self.weathers.settings.seen_welcome
    }

    pub fn overlay_stats(&self) -> (usize, u64) {
        match &self.overlay {
            Some(o) => (o.panels(), o.frames()),
            None => (self.last_panels, self.last_frames),
        }
    }

    pub fn monitor_count(&self) -> usize {
        self.monitors.len()
    }

    fn egui_now(&self) -> f64 {
        self.ctx.input(|i| i.time)
    }

    // ---- driver tick ----

    /// One driver tick (about 64 Hz).
    pub fn tick(&mut self) {
        self.ticks += 1;
        let now = self.clock.now();
        let wall = self.clock.wall();
        if let Some(t) = self.quitting {
            if now - t > FORCE_EXIT_S {
                log::warn!("window loop did not close in time, exiting");
                self.cleanup();
                std::process::exit(0);
            }
            return;
        }
        self.drain_tray();
        if packs_done(&mut self.packs_rx) {
            let host = self.host_info();
            self.shell.set_host(host);
            self.next_daynight = now;
        }
        if self.open_request.swap(false, Ordering::Relaxed) {
            self.show();
        }
        self.drain_system(now);
        self.drain_hooks(now);
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            log::info!("--seconds elapsed, exiting");
            self.quit();
            return;
        }
        if self.demo_at.is_some_and(|t| now >= t) {
            self.demo_at = None;
            self.toggle_picker();
        }
        self.run_sources(now, wall);
        if now >= self.next_power {
            self.next_power = now + POWER_EVERY_S;
            self.read_power();
        }
        let rescaled = self.overlay.as_ref().is_some_and(OverlayHost::rescaled);
        if take_rebuild(&mut self.rebuild_pending, rescaled) {
            self.rebuild_displays();
        }
        if !self.sleep.asleep() {
            self.shelves.step(now, || Win.taskbar_shelves());
        }
        self.poll_tracker(now);
        if now >= self.next_keep {
            self.next_keep = now + KEEP_EVERY_S;
            if let Some(o) = &self.overlay {
                o.set_under_dock(self.layered.under_dock_ids());
                o.keep_on_top();
                self.last_panels = o.panels();
                self.last_frames = o.frames();
            }
            self.layered.raise_front();
        }
        self.step_world(now, wall);
        if now >= self.next_daynight {
            self.next_daynight = now + wallpaper::EVERY_S;
            let packs = self.data_dir.join("Wallpapers");
            let dirs = Dirs { data: &self.data_dir, packs: &packs };
            let action = self.wallpaper.step(&self.platform, &dirs, &mut self.prefs, wall.local_minutes());
            tell_cycle_step(&mut self.shell, &action);
        }
    }

    fn drain_tray(&mut self) {
        let actions = self.tray.as_ref().map(Tray::poll).unwrap_or_default();
        for a in actions {
            self.apply_tray(a);
        }
    }

    fn drain_hooks(&mut self, now: f64) {
        while let Ok(e) = self.hooks_rx.try_recv() {
            self.tracker.on_event(e, now);
        }
    }

    fn drain_system(&mut self, now: f64) {
        let events: Vec<_> = self.system.borrow_mut().drain(..).collect();
        for (e, _) in events {
            let a = system_actions(&mut self.sleep, e);
            if a.pause {
                // Every panel pauses; waking forces a poll (desktop-native 3.7).
                if let Some(o) = &self.overlay {
                    o.update(&[], 0.0, false);
                }
            }
            if a.wake {
                self.force_poll = true;
            }
            if a.resume {
                self.sources.resume(&self.weathers.settings, now);
            }
            if a.rebuild {
                self.rebuild_pending = true;
            }
            if a.theme {
                self.read_taskbar_theme();
            }
            if a.power {
                self.read_power();
            }
        }
    }

    fn read_taskbar_theme(&mut self) {
        let light = Os::taskbar_light(&self.platform);
        if let Some(t) = self.tray.as_mut() {
            t.set_light_taskbar(light);
        }
        self.refresh_tray();
    }

    fn read_power(&mut self) {
        let p = self.platform.power();
        self.saving = p.saving;
        let c = self.weathers.set_on_battery(p.on_battery);
        self.apply_changes(c);
        self.push_overlay();
    }

    fn rebuild_displays(&mut self) {
        self.monitors = self.platform.monitors();
        self.layout = Layout::new(self.monitors.clone());
        if let Some(o) = self.overlay.as_mut() {
            match o.rebuild(&self.monitors) {
                Err(e) => log::warn!("overlay rebuild failed: {e}"),
                Ok(()) => {
                    // New panels re-read reduced motion (desktop-native 3.7).
                    if let Some(kind) = self.weathers.shown_weather() {
                        let (rm, f) = motion_factory(&self.platform, kind, self.feed.clone(), self.seed);
                        self.reduce_motion = rm;
                        o.set_effect(Some(f));
                    }
                }
            }
        }
        self.tracker.rebuilt();
        // The first poll after a rebuild reads the power state at once (3.5.2).
        self.next_power = self.clock.now();
        self.pause_if_asleep();
        self.recompute_main_frame();
        let host = self.host_info();
        self.shell.set_host(host);
        self.force_poll = true;
    }

    fn recompute_main_frame(&mut self) {
        self.main_frame = select_screens(&self.layout, &self.weathers.settings).1;
    }

    // ---- overlay ----

    /// Create or drop the overlay with the shown weather and give new
    /// panels their simulations.
    fn sync_effect(&mut self) {
        let shown = self.weathers.shown_weather();
        match shown {
            Some(kind) => {
                if self.overlay.is_none() && !self.overlay_failed {
                    match OverlayHost::new(&self.monitors) {
                        Ok(o) => {
                            self.overlay = Some(o);
                            self.pushed = Pushed::default();
                            self.pause_if_asleep();
                        }
                        Err(e) => {
                            log::warn!("overlay unavailable: {e}");
                            self.overlay_failed = true;
                        }
                    }
                }
                if let Some(o) = &self.overlay {
                    let (rm, f) = motion_factory(&self.platform, kind, self.feed.clone(), self.seed);
                    self.reduce_motion = rm;
                    o.set_effect(Some(f));
                }
            }
            None => {
                if let Some(o) = self.overlay.take() {
                    self.last_panels = o.panels();
                    self.last_frames = o.frames();
                    // Dropping stops the render threads (UI thread only).
                }
                self.overlay_failed = false;
            }
        }
        if shown != self.shown {
            self.force_poll = true;
            // KeepOnTop also runs on a Shown change (desktop-native 3.4).
            self.next_keep = self.clock.now();
        }
        self.shown = shown;
        self.push_overlay();
    }

    /// New panels start shown; while asleep every panel stays paused (3.7).
    fn pause_if_asleep(&self) {
        if self.sleep.asleep()
            && let Some(o) = &self.overlay
        {
            o.update(&[], 0.0, false);
        }
    }

    fn share(&self) -> f64 {
        (if self.saving { 0.5 } else { 1.0 }) * self.weathers.snow_strength()
    }

    /// Push share, lightning, wind, keep-clear and behind-windows when they differ.
    fn push_overlay(&mut self) {
        let Some(o) = &self.overlay else { return };
        let s = &self.weathers.settings;
        let want = Pushed {
            wind: Some(s.wind),
            keep_clear: Some(s.keep_clear),
            behind: Some(s.behind_windows),
            share: Some(self.share()),
            lightning: Some(self.weathers.lightning_shown()),
        };
        if want.share != self.pushed.share {
            o.set_share(self.share());
        }
        if want.lightning != self.pushed.lightning {
            o.set_lightning(self.weathers.lightning_shown());
        }
        if want.wind != self.pushed.wind {
            o.set_wind(s.wind);
        }
        if want.keep_clear != self.pushed.keep_clear {
            o.set_keep_clear(s.keep_clear);
        }
        if want.behind != self.pushed.behind {
            o.set_behind_windows(s.behind_windows);
        }
        self.pushed = want;
    }

    fn poll_tracker(&mut self, now: f64) {
        let Some(o) = &self.overlay else { return };
        if self.sleep.asleep() || self.shown.is_none() {
            return;
        }
        if !self.tracker.due(now, self.force_poll, self.saving) {
            return;
        }
        self.force_poll = false;
        let windows = self.platform.windows();
        let cloaked_known = windows.iter().filter(|w| w.cloaked && self.tracker.known(w.hwnd)).count();
        let listed = windows.iter().filter_map(fp_tracker::filter).collect();
        let (picked, _) = select_screens(&self.layout, &self.weathers.settings);
        let frames: Vec<Frame> = picked.iter().map(|&i| self.layout.screens()[i].points).collect();
        let result = self.tracker.poll(
            now,
            PollInput {
                listed,
                cloaked_known,
                screens: &frames,
                shelves: &self.shelves.list,
                layout: &self.layout,
                corner: self.corner,
            },
        );
        publish_screens(&self.feed, &result.screens);
        o.update(&result.screens, result.interval, result.to_other_space);
    }

    // ---- world ----

    fn step_world(&mut self, now: f64, wall: WallTime) {
        self.cursor = self.platform.cursor();
        if self.layered.fast_poll() || now >= self.next_hover {
            self.next_hover = now + HOVER_EVERY_S;
            self.hot = self.layered.hit(self.cursor);
        }
        let pointer = PointerInput {
            pos: self.cursor.map(|(x, y)| self.layout.to_points(f64::from(x), f64::from(y))),
            left_down: self.platform.left_button_down(),
            right_down: self.platform.right_button_down(),
            hot: self.hot,
        };
        self.world.set_paused_for_battery(self.weathers.paused_for_battery());
        // The quote deck follows the language the cards are shown in: with no
        // saved choice that is the startup language, not English.
        let fill_lang = self.weathers.settings.language.is_none();
        if fill_lang {
            self.weathers.settings.language = Some(self.lang);
        }
        // Windows are enumerated only when a picker add is waiting.
        let windows = if self.world.has_pending_add() {
            window_obstacles(&self.layout, &self.platform.windows())
        } else {
            Vec::new()
        };
        let input = WorldInput {
            now,
            wall,
            pointer,
            settings: &self.weathers.settings,
            layout: &self.layout,
            snowing: self.shown == Some(EffectKind::Snow),
            main_screen: self.main_frame,
            picker: self.cards.picker_rect(),
            windows: &windows,
        };
        self.world.step(&input, &mut self.out);
        if let Some((id, _)) = self.out.decor_menu.take()
            && let Some(cmd) = decor_menu_command(&self.platform, self.lang, self.cursor, id)
        {
            self.world.command(cmd, now);
        }
        if log::log_enabled!(target: "frostpane::balls", log::Level::Debug) {
            for sp in self.out.sprites.iter().filter(|sp| sp.key.owner_byte() == Owner::Ball as u8) {
                log::debug!(target: "frostpane::balls", "t {now:.2} ball {:?}", sp);
            }
        }
        if fill_lang {
            self.weathers.settings.language = None;
        }
        self.write_world_prefs();
        let world = &self.world;
        self.layered.sync(&self.out.sprites, &self.layout, &mut |k, s| world.paint(k, s));
        for req in std::mem::take(&mut self.out.cards) {
            self.open_card(req);
        }
        close_stale_cards(&mut self.cards, &self.world);
        let snow_stopped = std::mem::take(&mut self.out.snow_stopped);
        sync_station(&mut self.cards, self.world.station_open(), snow_stopped);
        self.poll_outside(now);
    }

    /// 15 ms outside-press poll for the dress and tree cards (desktop-native
    /// 3.11, `anywhere = false`).
    fn poll_outside(&mut self, now: f64) {
        let cursor = self.cursor;
        if self.outside.step(now, self.cards.outside_watch(), outside::buttons_down, || outside::over_own(cursor)) {
            self.cards.close_outside(self.egui_now());
            self.ctx.request_repaint();
        }
    }

    fn write_world_prefs(&mut self) {
        if self.out.prefs.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.out.prefs);
        if let Err(e) = self.prefs.edit(|m| {
            for (k, v) in batch {
                if v.is_null() {
                    m.remove(k);
                } else {
                    m.insert(k.to_string(), v);
                }
            }
        }) {
            log::warn!("saving world state failed: {e}");
        }
    }

    fn cursor_point(&self) -> Vec2 {
        self.cursor.map_or(Vec2::new(0.0, 0.0), |(x, y)| self.layout.to_points(f64::from(x), f64::from(y)))
    }

    fn open_card(&mut self, req: CardRequest) {
        let now = self.egui_now();
        let cursor = self.cursor_point();
        let tree_origin = match &req {
            CardRequest::TreeStyle { tree, .. } => {
                let work = tree_card_work(&self.layout, self.world.tree_card_anchor(*tree));
                work.and_then(|w| self.world.tree_card_origin(*tree, w))
            }
            _ => None,
        };
        let cx = CardCtx { lang: self.lang, layout: &self.layout, sprites: &self.out.sprites, cursor, tree_origin };
        if matches!(req, CardRequest::DressUp { .. } | CardRequest::TreeStyle { .. }) {
            // A new card arms a fresh detector: its opening press does not count.
            self.outside.reset();
        }
        self.cards.open(req, &cx, now);
        self.ctx.request_repaint();
    }

    /// Tray pointer (core-app 2.7): false when the icon rect is unknown or
    /// the icon sits in the overflow flyout.
    fn show_tray_pointer(&mut self) -> bool {
        let rect = self.tray.as_ref().and_then(Tray::icon_rect);
        let now = self.egui_now();
        let cx = CardCtx {
            lang: self.lang,
            layout: &self.layout,
            sprites: &self.out.sprites,
            cursor: self.cursor_point(),
            tree_origin: None,
        };
        let cards = &mut self.cards;
        let shown = tray_pointer(&self.platform, rect, |r| cards.open_pointer(r, &cx, now));
        self.ctx.request_repaint();
        shown
    }

    fn toggle_picker(&mut self) {
        let count = self.world.decoration_count();
        let max = self.world.decoration_max() as usize;
        let now = self.egui_now();
        let cx = CardCtx {
            lang: self.lang,
            layout: &self.layout,
            sprites: &self.out.sprites,
            cursor: self.cursor_point(),
            tree_origin: None,
        };
        self.cards.toggle_picker(&cx, count, max, now);
        self.ctx.request_repaint();
    }

    // ---- sources ----

    fn run_sources(&mut self, now: f64, wall: WallTime) {
        let due = self.sources.due(now);
        if due.forecast
            && self.weathers.settings.match_weather
            && let Some(city) = self.weathers.settings.city.clone()
        {
            self.sources.request_forecast(&city);
        }
        if due.random && self.weathers.settings.random_weather {
            let pick = fp_sources::random_pick(self.weathers.random_pick, &mut self.rng);
            let c = self.weathers.set_random_pick(Some(pick));
            self.apply_changes(c);
        }
        if due.season && self.weathers.settings.match_season {
            let (month, south) = self.season_env(wall);
            let pick = season::effect_for(month, south);
            if self.weathers.season_pick != Some(pick) {
                let c = self.weathers.set_season_pick(Some(pick));
                self.apply_changes(c);
            }
        }
        if due.update {
            self.sources.check_update(false);
        }
        let (settings, platform) = (&self.weathers.settings, &self.platform);
        let events = self.sources.poll(|| season_env(settings, platform, &platform.time_zone_id(), wall));
        for e in events {
            self.on_event(e, wall);
        }
    }

    fn season_env(&self, wall: WallTime) -> (u32, bool) {
        season_env(&self.weathers.settings, &self.platform, &self.platform.time_zone_id(), wall)
    }

    fn on_event(&mut self, e: Event, wall: WallTime) {
        let now = self.egui_now();
        match e {
            Event::Outside(o) => {
                log::info!("weather outside: {o:?}");
                let c = self.weathers.set_outside(o);
                self.apply_changes(c);
            }
            Event::ForecastFailed => {}
            Event::Cities { query, result } => self.shell.city_results(&query, result),
            Event::Update { asked, outcome } => match outcome {
                UpdateOutcome::Newer(r) => {
                    let version = r.tag.trim_start_matches(['v', 'V']).to_string();
                    self.shell.update_result(UpdateLine::Available { version: version.clone(), url: r.url.clone() });
                    let asked_at = self.prefs.get_str(KEY_UPDATE_ASKED);
                    if update::should_prompt(asked, self.seen_welcome(), asked_at, wall.unix) {
                        let cx = CardCtx {
                            lang: self.lang,
                            layout: &self.layout,
                            sprites: &self.out.sprites,
                            cursor: self.cursor_point(),
                            tree_origin: None,
                        };
                        self.cards.open_update(&version, &r.url, &cx, now);
                    }
                }
                other => {
                    if let Some(line) = settled_line(self.shell.update_line(), asked, &other, now) {
                        self.shell.update_result(line);
                    }
                }
            },
        }
        self.ctx.request_repaint();
    }

    // ---- settings ----

    /// Run the reactions of core-app 2.4 for `c`.
    fn apply_changes(&mut self, c: Changes) {
        if c.is_empty() {
            return;
        }
        let r: Reaction = react(c);
        if r.save {
            self.save_settings();
        }
        if r.sources {
            let now = self.clock.now();
            self.sources.sync(&self.weathers.settings, now);
        }
        self.recompute_main_frame();
        if r.effect {
            self.sync_effect();
        } else if r.share || r.lightning || r.overlay {
            self.push_overlay();
        }
        if r.tray {
            self.refresh_tray();
        }
    }

    fn save_settings(&mut self) {
        let s = self.weathers.settings.clone();
        if let Err(e) = self.prefs.edit(|m| s.write_into(m)) {
            log::warn!("saving settings failed: {e}");
        }
    }

    fn refresh_tray(&mut self) {
        if let Some(t) = self.tray.as_mut()
            && let Err(e) = t.refresh(&self.weathers, self.lang)
        {
            log::warn!("tray refresh failed: {e}");
        }
    }

    // ---- tray ----

    fn tray_changes(&mut self, a: TrayAction) -> Changes {
        let now = self.clock.now();
        let w = &mut self.weathers;
        match a {
            TrayAction::SnowVariant { heavy } => w.tray_snow(heavy),
            TrayAction::RainVariant { storm } => w.tray_rain(storm),
            TrayAction::Toggle(e) => w.toggle_weather(e),
            TrayAction::Wind(wind) => {
                w.settings.wind = wind;
                Changes::OTHER
            }
            TrayAction::ToggleMatchWeather => {
                let on = !w.settings.match_weather;
                w.set_match_weather(on)
            }
            TrayAction::ToggleRandom => {
                let on = !w.settings.random_weather;
                w.set_random_weather(on)
            }
            TrayAction::ToggleSeason => {
                let on = !w.settings.match_season;
                w.set_match_season(on)
            }
            TrayAction::Off => w.turn_off(),
            TrayAction::SendPlough => self.command(Command::SendPlough, now),
            TrayAction::SendSnowmen => self.command(Command::SendFight(None), now),
            TrayAction::SendYeti => self.command(Command::SendYeti, now),
            TrayAction::SendSanta => self.command(Command::SendSanta, now),
            TrayAction::SendTrain => self.command(Command::SendTrain, now),
            TrayAction::Decorate => {
                self.toggle_picker();
                Changes::NONE
            }
            TrayAction::Open => {
                self.show();
                Changes::NONE
            }
            TrayAction::Quit => {
                self.quit();
                Changes::NONE
            }
        }
    }

    fn command(&mut self, c: Command, now: f64) -> Changes {
        self.world.command(c, now);
        Changes::NONE
    }

    pub fn apply_tray(&mut self, a: TrayAction) {
        let c = self.tray_changes(a);
        self.apply_changes(c);
    }

    /// Planting forces the weather to snow first (trees 3.1).
    fn plant_tree(&mut self) {
        let c = if self.shown == Some(EffectKind::Snow) {
            Changes::NONE
        } else {
            self.weathers.set_weather(Some(EffectKind::Snow))
        };
        self.apply_changes(c);
        let now = self.clock.now();
        self.world.command(Command::PlantTree, now);
    }

    // ---- window ----

    pub fn show(&mut self) {
        self.visible = true;
        self.minimized = false;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        // Topmost on and off brings the window to the front (core-app 2.3 Open).
        self.ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.ctx.request_repaint();
    }

    /// Close hides to the tray (core-app 2.3).
    pub fn hide(&mut self) {
        self.visible = false;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.window_left();
    }

    /// The window was hidden or minimised: the UI resets what it does not
    /// remember and may ask for actions (live onboarding weather).
    fn window_left(&mut self) {
        let seen = self.seen_welcome();
        for a in self.shell.on_hide(seen) {
            self.apply_ui(a);
        }
    }

    pub fn quit(&mut self) {
        if self.quitting.is_none() {
            self.quitting = Some(self.clock.now());
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.ctx.request_repaint();
        }
    }

    /// Per-frame logic from `eframe::App::logic` (runs while hidden too).
    pub fn logic(&mut self) {
        // eframe shows the root window after its first painted frame and
        // ignores `with_visible(false)`, so a quiet start hides it again
        // during the first few frames.
        if self.startup_frames < STARTUP_HIDE_FRAMES {
            self.startup_frames += 1;
            if !self.visible {
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.ctx.request_repaint();
            }
        }
        let (close, minimized) =
            self.ctx.input(|i| (i.viewport().close_requested(), i.viewport().minimized == Some(true)));
        if close && self.quitting.is_none() {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide();
        }
        // Fallback for `--seconds` when the driver timer is not ticking.
        if self.deadline.is_some_and(|d| Instant::now() >= d) {
            self.quit();
        }
        if minimized != self.minimized {
            self.minimized = minimized;
            if minimized {
                self.window_left();
            }
        }
        let focused = self.ctx.input(|i| i.viewport().focused).unwrap_or(false);
        let activated = focused && !self.focused;
        self.focused = focused;
        if self.visible {
            recheck_login(&self.platform, &mut self.shell, activated);
        }
        if self.cards.any_open() {
            // Cards draw inside the root `ui`, which a hidden window skips.
            self.ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        }
        self.ctx.request_repaint_after(repaint_every(self.visible, self.cards.any_open()));
    }

    /// The root `ui`: the main window while visible, and the cards.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if self.visible && !self.minimized {
            let wall = self.clock.wall();
            for a in self.shell.ui(ui, &self.weathers, self.lang, wall) {
                self.apply_ui(a);
            }
        }
        let ctx = ui.ctx().clone();
        for a in self.cards.show(&ctx, self.lang) {
            self.apply_card(a);
        }
    }

    // ---- UI actions ----

    fn apply_ui(&mut self, a: UiAction) {
        let now = self.egui_now();
        let wall = self.clock.wall();
        // Fallback "Screen N" names are localized in host_info; refresh them.
        let relabel = matches!(a, UiAction::SetLanguage(_));
        let w = &mut self.weathers;
        let changes = match a {
            UiAction::Tray(t) => self.tray_changes(t),
            UiAction::GridVariant { effect, second } => w.grid_variant(effect, second),
            UiAction::SetLanguage(l) => {
                w.settings.language = Some(l);
                self.lang = l;
                self.ctx
                    .send_viewport_cmd(egui::ViewportCommand::Title(fp_text::t(l, fp_text::Key::AppName).to_string()));
                Changes::OTHER
            }
            UiAction::SetSnowballs(on) => {
                w.settings.snowballs = on;
                Changes::OTHER
            }
            UiAction::SetBehindWindows(on) => {
                w.settings.behind_windows = on;
                Changes::OTHER
            }
            UiAction::SetKeepClear(on) => {
                w.settings.keep_clear = on;
                Changes::OTHER
            }
            UiAction::SetOftenVisits(on) => {
                w.settings.often_visits = on;
                Changes::OTHER
            }
            UiAction::SetPauseOnBattery(on) => w.set_pause_on_battery(on),
            UiAction::SetVisitor(v, on) => w.set_visitor(v, on),
            UiAction::SetScreen { id, on } => {
                self.force_poll = true;
                w.set_screen(&id, on)
            }
            UiAction::SetMainScreen(id) => w.set_main_screen(Some(id)),
            UiAction::SetLaunchAtLogin(on) => {
                let ok = Os::login_set(&self.platform, on);
                login_result(&self.platform, &mut self.shell, on, ok);
                Changes::NONE
            }
            UiAction::SetWeather(e) => w.set_weather(e),
            UiAction::FinishOnboarding => {
                w.settings.seen_welcome = true;
                w.set_weather(Some(EffectKind::Snow)) | Changes::OTHER
            }
            UiAction::Wallpaper(action) => {
                self.run_wallpaper(&action, now, wall);
                Changes::NONE
            }
            UiAction::WallpaperWith { action, effect } => {
                self.run_wallpaper(&action, now, wall);
                wallpaper::with_weather(&mut self.weathers, effect)
            }
            UiAction::SearchCity(q) => {
                if !self.sources.search(&q, self.lang.code()) {
                    self.shell.city_results(&q, Err(()));
                }
                Changes::NONE
            }
            UiAction::SetCity(city) => {
                w.settings.city = city;
                let nowc = self.clock.now();
                self.sources.refresh_forecast(&self.weathers.settings, nowc);
                Changes::OTHER
            }
            UiAction::CheckUpdates => {
                let started = self.sources.check_update(true);
                let line = check_line(started, self.sources.working(), self.egui_now());
                self.shell.update_result(line);
                Changes::NONE
            }
            UiAction::OpenUrl(url) => {
                Os::open_url(&self.platform, &url);
                Changes::NONE
            }
            UiAction::OpenWallpapersFolder => {
                let dir = self.data_dir.join("Wallpapers");
                let _ = std::fs::create_dir_all(&dir);
                #[cfg(windows)]
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                Changes::NONE
            }
            UiAction::ShowTrayPointer => {
                let shown = self.show_tray_pointer();
                self.shell.pointer_result(shown);
                Changes::NONE
            }
            UiAction::SeenChangelog(v) => {
                let _ = self.prefs.put(KEY_SEEN_CHANGELOG, &v);
                Changes::NONE
            }
            UiAction::HideWindow => {
                self.hide();
                Changes::NONE
            }
            UiAction::Quit => {
                self.quit();
                Changes::NONE
            }
        };
        if relabel {
            let host = self.host_info();
            self.shell.set_host(host);
        }
        self.apply_changes(changes);
    }

    fn run_wallpaper(&mut self, a: &fp_ui::wallpapers::WallAction, now: f64, wall: WallTime) {
        let packs = self.data_dir.join("Wallpapers");
        let dirs = Dirs { data: &self.data_dir, packs: &packs };
        let ok = self.wallpaper.run(a, &self.platform, &dirs, &mut self.prefs, wall.local_minutes());
        tell_wallpaper_run(&mut self.shell, a, ok, now);
    }

    fn apply_card(&mut self, a: CardAction) {
        let now = self.clock.now();
        match a {
            CardAction::RestyleSnowman { bottom, look } => self.world.restyle_snowman(bottom, look),
            CardAction::RestyleTree { tree, look } => self.world.restyle_tree(tree, look),
            CardAction::DoomTree(id) => self.world.doom_tree(id),
            CardAction::TreeCardClosed => self.world.close_tree_card(),
            CardAction::AddDecoration(kind) => self.world.command(Command::AddDecoration(kind), now),
            CardAction::RemoveAllDecorations => self.world.command(Command::RemoveAllDecorations, now),
            CardAction::PlantTree => self.plant_tree(),
            CardAction::TrainFetch(p) => self.world.command(Command::TrainFetch(p), now),
            CardAction::TrainDismiss => self.world.command(Command::TrainDismiss, now),
            CardAction::UpdateLater => {
                let at = self.clock.wall().iso_utc();
                let _ = self.prefs.put(KEY_UPDATE_ASKED, &at);
            }
            CardAction::OpenUrl(url) => Os::open_url(&self.platform, &url),
        }
    }

    // ---- exit ----

    /// Final saves and teardown (UI thread). Safe to call twice.
    pub fn cleanup(&mut self) {
        let mut out = WorldOutput::default();
        self.world.shutdown(&mut out);
        self.out.prefs = out.prefs;
        self.write_world_prefs();
        self.layered.clear();
        if let Some(o) = self.overlay.take() {
            self.last_panels = o.panels();
            self.last_frames = o.frames();
        }
        self.tray = None;
        self.hooks = None;
        self.watcher = None;
        crate::crash::finish(&self.data_dir);
        let _ = (&self.args, self.started);
    }
}

/// Status line for a finished check that is not a newer release. A check the
/// user asked for, or one that is answering a visible "Checking", is shown.
fn settled_line(current: &UpdateLine, asked: bool, outcome: &UpdateOutcome, now: f64) -> Option<UpdateLine> {
    let show = asked || matches!(current, UpdateLine::Checking);
    match outcome {
        UpdateOutcome::Latest if show => Some(UpdateLine::UpToDate { at: now }),
        UpdateOutcome::Failed if show => Some(UpdateLine::Offline { at: now }),
        _ => None,
    }
}

/// Status line after the user pressed Check. A refused request with no check
/// running will never reply, so it reads as offline; a running check answers
/// later through [`settled_line`].
fn check_line(started: bool, working: bool, now: f64) -> UpdateLine {
    if started || working { UpdateLine::Checking } else { UpdateLine::Offline { at: now } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::geom::PixelRect;
    use fp_core::settings::City;
    use fp_win::WallpaperScreen;

    #[test]
    fn egui_passes_slow_down_while_nothing_shows() {
        assert_eq!(repaint_every(true, false), UI_REPAINT);
        assert_eq!(repaint_every(false, true), UI_REPAINT);
        assert_eq!(repaint_every(true, true), UI_REPAINT);
        assert_eq!(repaint_every(false, false), HIDDEN_REPAINT);
        assert!(HIDDEN_REPAINT > UI_REPAINT);
    }

    #[test]
    fn packs_done_fires_once_when_the_worker_finishes() {
        let mut none: Option<Receiver<usize>> = None;
        assert!(!packs_done(&mut none));
        let (tx, rx) = channel();
        let mut rx = Some(rx);
        assert!(!packs_done(&mut rx));
        assert!(rx.is_some());
        tx.send(48).unwrap();
        assert!(packs_done(&mut rx));
        assert!(rx.is_none());
        assert!(!packs_done(&mut rx));
        // worker died without sending: also done
        let (tx, r) = channel::<usize>();
        drop(tx);
        let mut r = Some(r);
        assert!(packs_done(&mut r));
    }

    #[test]
    fn decor_menu_chosen_removes_with_localised_label() {
        let mut seen = None;
        let cmd = decor_menu_with(
            |label, at| {
                seen = Some((label.to_string(), at));
                true
            },
            Lang::En,
            Some((5, 6)),
            9,
        );
        assert_eq!(cmd, Some(Command::RemoveDecoration(9)));
        assert_eq!(seen, Some((fp_text::t(Lang::En, fp_text::Key::DecorTakeDown).to_string(), (5, 6))));
        assert_eq!(decor_menu_with(|_, at| at == (0, 0), Lang::En, None, 1), Some(Command::RemoveDecoration(1)));
        assert_eq!(decor_menu_with(|_, _| false, Lang::En, None, 1), None);
    }
    use std::path::{Path, PathBuf};

    #[test]
    fn open_picker_counts_follow_the_world() {
        use fp_core::scene::PointerInput;
        use fp_core::settings::Settings;
        use fp_world::WorldInput;
        let l = Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "A".into(),
            pixels: PixelRect::new(0, 0, 1920, 1080),
            work: PixelRect::new(0, 0, 1920, 1040),
            scale: 1.0,
            primary: true,
        }]);
        let settings = Settings { max_placed: 2, ..Settings::default() };
        let mut world = World::new(std::sync::Arc::new(SnowFeed::default()), &serde_json::Map::new(), 7);
        let mut out = fp_world::WorldOutput::default();
        let clock = std::cell::Cell::new(0.0_f64);
        let mut step = |world: &mut World, cards: &mut Cards| {
            clock.set(clock.get() + 0.1);
            let now = clock.get();
            let input = WorldInput {
                now,
                wall: WallTime::new(1_700_000_000 + now as i64, 0),
                pointer: PointerInput { pos: None, left_down: false, right_down: false, hot: None },
                settings: &settings,
                layout: &l,
                snowing: false,
                main_screen: l.primary().map(|s| s.work_points),
                picker: None,
                windows: &[],
            };
            world.step(&input, &mut out);
            close_stale_cards(cards, world);
        };
        let mut cards = Cards::new(1);
        // closed picker: nothing to sync
        step(&mut world, &mut cards);
        assert_eq!(cards.picker_counts(), None);
        let cx =
            CardCtx { lang: Lang::En, layout: &l, sprites: &[], cursor: Vec2::new(500.0, 500.0), tree_origin: None };
        cards.toggle_picker(&cx, 0, 2, 0.0);
        let kind = fp_decor::Kind::RedBauble;
        world.command(Command::AddDecoration(kind), clock.get());
        step(&mut world, &mut cards);
        assert_eq!(cards.picker_counts(), Some((1, 2)));
        world.command(Command::AddDecoration(kind), clock.get());
        step(&mut world, &mut cards);
        assert_eq!(cards.picker_counts(), Some((2, 2)));
        world.command(Command::RemoveAllDecorations, clock.get());
        step(&mut world, &mut cards);
        assert_eq!(cards.picker_counts(), Some((0, 2)));
    }

    #[test]
    fn stale_dress_card_closes() {
        let l = fp_core::desktop::Layout::new(vec![]);
        let feed = std::sync::Arc::new(SnowFeed::default());
        let world = World::new(feed, &serde_json::Map::new(), 7);
        let mut cards = Cards::new(1);
        let cx =
            CardCtx { lang: Lang::En, layout: &l, sprites: &[], cursor: Vec2::new(500.0, 500.0), tree_origin: None };
        cards.open(CardRequest::DressUp { bottom: 5, look: None }, &cx, 0.0);
        assert_eq!(cards.dress_open(), Some(5));
        close_stale_cards(&mut cards, &world);
        assert_eq!(cards.dress_open(), None);
    }

    #[test]
    fn tree_card_work_follows_the_tree_not_the_cursor() {
        use fp_core::geom::Vec2;
        let mk = |id: &str, primary, left: i32| MonitorInfo {
            id: id.into(),
            name: id.into(),
            pixels: PixelRect::new(left, 0, left + 1000, 700),
            work: PixelRect::new(left, 0, left + 1000, 660),
            scale: 1.0,
            primary,
        };
        let l = Layout::new(vec![mk("a", true, 0), mk("b", false, 1000)]);
        let b = l.by_id("b").unwrap().work_points;
        // Anchor on monitor B: B's work area, whatever the cursor does.
        let w = tree_card_work(&l, Some(Vec2::new(1500.0, 10.0))).unwrap();
        assert_eq!(w, b);
        // Anchor off every monitor: the nearest one, not the primary.
        assert_eq!(tree_card_work(&l, Some(Vec2::new(5000.0, 10.0))).unwrap(), b);
        // No anchor: the primary monitor.
        assert_eq!(tree_card_work(&l, None).unwrap(), l.primary().unwrap().work_points);
        assert!(tree_card_work(&Layout::new(vec![]), None).is_none());
    }

    #[test]
    fn station_card_closes_with_the_world() {
        use fp_core::geom::Vec2;
        use fp_world::CardRequest;
        let l = Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "A".into(),
            pixels: PixelRect::new(0, 0, 1920, 1080),
            work: PixelRect::new(0, 0, 1920, 1040),
            scale: 1.0,
            primary: true,
        }]);
        let cx =
            CardCtx { lang: Lang::En, layout: &l, sprites: &[], cursor: Vec2::new(500.0, 500.0), tree_origin: None };
        let station = || CardRequest::TrainStation { at: Vec2::new(300.0, 100.0), choices: Vec::new() };
        let mut cards = Cards::new(1);
        cards.open(station(), &cx, 0.0);
        // world still has the train waiting: card stays
        sync_station(&mut cards, true, false);
        assert!(cards.station_open());
        // train left
        sync_station(&mut cards, false, false);
        assert!(!cards.any_open());
        // snow off closes dress, quote and station
        cards.open(station(), &cx, 0.0);
        cards.open(CardRequest::Quote { at: Vec2::new(900.0, 600.0), quote: 0 }, &cx, 0.0);
        cards.open(CardRequest::DressUp { bottom: 1, look: None }, &cx, 0.0);
        sync_station(&mut cards, true, true);
        assert!(!cards.any_open());
    }

    #[test]
    fn shelves_read_in_background_and_throttled() {
        let rect = PixelRect::new(0, 1, 2, 3);
        let mut sh = Shelves::new();
        assert!(sh.list.is_empty());
        sh.step(0.0, move || vec![rect]);
        assert!(sh.rx.is_some());
        // The poll that started the read still sees the previous (empty) list.
        assert!(sh.list.is_empty());
        for _ in 0..200 {
            std::thread::sleep(Duration::from_millis(5));
            sh.step(0.5, || panic!("not due"));
            if sh.rx.is_none() {
                break;
            }
        }
        assert_eq!(sh.list, vec![rect]);
        // Due again after 1 s: a new read starts.
        sh.step(1.0, Vec::new);
        assert!(sh.rx.is_some());
    }

    struct Fake(Option<f64>);

    impl Os for Fake {
        fn open_url(&self, _: &str) {}
        fn login_is_on(&self) -> bool {
            false
        }
        fn login_blocked(&self) -> bool {
            false
        }
        fn login_set(&self, _: bool) -> bool {
            true
        }
        fn show_startup_apps(&self) {}
        fn taskbar_light(&self) -> bool {
            false
        }
        fn tray_icon_promoted(&self) -> bool {
            true
        }
        fn tray_icon_spot(&self, r: PixelRect) -> Option<PixelRect> {
            Some(r)
        }
        fn region_latitude(&self) -> Option<f64> {
            self.0
        }
        fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
            Vec::new()
        }
        fn wallpaper_set(&self, _: &Path) -> bool {
            true
        }
        fn wallpaper_set_each(&self, _: &[(String, PathBuf)]) -> bool {
            true
        }
        fn wallpaper_current(&self) -> Option<PathBuf> {
            None
        }
    }

    struct LoginOs {
        blocked: bool,
        on: bool,
        opened: std::cell::Cell<u32>,
    }

    impl Os for LoginOs {
        fn open_url(&self, _: &str) {}
        fn login_is_on(&self) -> bool {
            self.on
        }
        fn login_blocked(&self) -> bool {
            self.blocked
        }
        fn login_set(&self, _: bool) -> bool {
            false
        }
        fn show_startup_apps(&self) {
            self.opened.set(self.opened.get() + 1);
        }
        fn taskbar_light(&self) -> bool {
            false
        }
        fn tray_icon_promoted(&self) -> bool {
            true
        }
        fn tray_icon_spot(&self, r: PixelRect) -> Option<PixelRect> {
            Some(r)
        }
        fn region_latitude(&self) -> Option<f64> {
            None
        }
        fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
            Vec::new()
        }
        fn wallpaper_set(&self, _: &Path) -> bool {
            true
        }
        fn wallpaper_set_each(&self, _: &[(String, PathBuf)]) -> bool {
            true
        }
        fn wallpaper_current(&self) -> Option<PathBuf> {
            None
        }
    }

    struct TrayOs {
        promoted: bool,
        bar: PixelRect,
    }

    impl Os for TrayOs {
        fn open_url(&self, _: &str) {}
        fn login_is_on(&self) -> bool {
            false
        }
        fn login_blocked(&self) -> bool {
            false
        }
        fn login_set(&self, _: bool) -> bool {
            true
        }
        fn show_startup_apps(&self) {}
        fn taskbar_light(&self) -> bool {
            false
        }
        fn tray_icon_promoted(&self) -> bool {
            self.promoted
        }
        fn tray_icon_spot(&self, r: PixelRect) -> Option<PixelRect> {
            crate::core::tests::spot(r, self.bar)
        }
        fn region_latitude(&self) -> Option<f64> {
            None
        }
        fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
            Vec::new()
        }
        fn wallpaper_set(&self, _: &Path) -> bool {
            true
        }
        fn wallpaper_set_each(&self, _: &[(String, PathBuf)]) -> bool {
            true
        }
        fn wallpaper_current(&self) -> Option<PathBuf> {
            None
        }
    }

    fn spot(r: PixelRect, bar: PixelRect) -> Option<PixelRect> {
        (r.left >= bar.left && r.top >= bar.top && r.right <= bar.right && r.bottom <= bar.bottom).then_some(r)
    }

    #[test]
    fn tray_pointer_follows_promotion_and_taskbar_spot() {
        let bar = PixelRect::new(0, 1040, 1920, 1080);
        let inside = Some([1800.0, 1048.0, 24.0, 24.0]);
        let outside = Some([1800.0, 500.0, 24.0, 24.0]);
        // Not promoted, rect on the taskbar: pointer opens, result false (warning).
        let mut opened = 0;
        let os = TrayOs { promoted: false, bar };
        let shown = tray_pointer(&os, inside, |_| {
            opened += 1;
            true
        });
        assert_eq!(opened, 1);
        assert!(!shown);
        // Rect outside the taskbar (overflow flyout): no pointer, false.
        let os = TrayOs { promoted: true, bar };
        assert!(!tray_pointer(&os, outside, |_| panic!("must not open")));
        // Unknown rect: false.
        assert!(!tray_pointer(&os, None, |_| panic!("must not open")));
        // Promoted and inside: true; a pointer that fails to open is false.
        assert!(tray_pointer(&os, inside, |_| true));
        assert!(!tray_pointer(&os, inside, |_| false));
    }

    fn login_shell() -> Shell {
        Shell::new(HostInfo::default())
    }

    #[test]
    fn blocked_enable_opens_startup_apps_once() {
        let os = LoginOs { blocked: true, on: false, opened: Default::default() };
        let mut shell = login_shell();
        login_result(&os, &mut shell, true, false);
        assert_eq!(os.opened.get(), 1);
        assert!(!shell.host().login_on);
    }

    #[test]
    fn other_failure_and_success_do_not_open_startup_apps() {
        let os = LoginOs { blocked: false, on: true, opened: Default::default() };
        let mut shell = login_shell();
        login_result(&os, &mut shell, true, false);
        assert_eq!(os.opened.get(), 0);
        login_result(&os, &mut shell, true, true);
        login_result(&os, &mut shell, false, true);
        assert_eq!(os.opened.get(), 0);
    }

    #[test]
    fn activation_rechecks_login_only_while_blocked_note_shows() {
        let mut os = LoginOs { blocked: true, on: false, opened: Default::default() };
        let mut shell = login_shell();
        shell.show_tab(fp_ui::model::Tab::Settings);
        shell.show_pane(fp_ui::model::Pane::General);
        login_result(&os, &mut shell, true, false);
        assert!(shell.login_recheck_wanted());
        os.on = true;
        recheck_login(&os, &mut shell, false);
        assert!(shell.login_recheck_wanted());
        recheck_login(&os, &mut shell, true);
        assert!(!shell.login_recheck_wanted());
    }

    fn running_cycle_shell() -> Shell {
        Shell::new(HostInfo { day_pack: Some("winter".into()), ..HostInfo::default() })
    }

    #[test]
    fn day_cycle_stop_reaches_the_ui() {
        use fp_sources::daynight::Action;
        let mut s = running_cycle_shell();
        assert_eq!(s.picked_id(), Some("winter-day"));
        tell_cycle_step(&mut s, &Action::Nothing);
        assert_eq!(s.picked_id(), Some("winter-day"));
        tell_cycle_step(&mut s, &Action::Stop);
        assert_eq!(s.picked_id(), None);
    }

    #[test]
    fn failed_turn_on_clears_the_old_cycle_ring() {
        use fp_ui::wallpapers::WallAction;
        let mut s = running_cycle_shell();
        tell_wallpaper_run(&mut s, &WallAction::Main { pack: "rain".into() }, false, 0.0);
        assert_eq!(s.picked_id(), Some("winter-day"));
        tell_wallpaper_run(&mut s, &WallAction::TurnOn { pack: "spring".into() }, false, 0.0);
        assert_eq!(s.picked_id(), None);
        tell_wallpaper_run(&mut s, &WallAction::TurnOn { pack: "spring".into() }, true, 0.0);
        assert_eq!(s.picked_id(), Some("spring-day"));
    }

    fn july() -> WallTime {
        // 2026-07-15 12:00:00 UTC
        WallTime::new(1_784_116_800, 0)
    }

    #[test]
    fn city_latitude_decides_the_hemisphere() {
        let mut s = Settings {
            city: Some(City {
                name: "Sydney".into(),
                admin: String::new(),
                country: "Australia".into(),
                latitude: -33.9,
                longitude: 151.2,
            }),
            ..Settings::default()
        };
        let (month, south) = season_env(&s, &Fake(Some(10.0)), "W. Europe Standard Time", july());
        assert_eq!(month, 7);
        assert!(south);
        s.city.as_mut().unwrap().latitude = 59.9;
        assert!(!season_env(&s, &Fake(Some(-10.0)), "AUS Eastern Standard Time", july()).1);
    }

    #[test]
    fn without_a_city_the_time_zone_and_region_decide() {
        let s = Settings::default();
        assert!(season_env(&s, &Fake(None), "AUS Eastern Standard Time", july()).1);
        assert!(!season_env(&s, &Fake(None), "W. Europe Standard Time", july()).1);
        // A split zone uses the region's latitude.
        assert!(season_env(&s, &Fake(Some(-23.0)), "E. South America Standard Time", july()).1);
    }

    #[test]
    fn work_areas_are_logical_and_primary_first() {
        let mk = |primary, left, top, right, bottom, scale| MonitorInfo {
            id: String::new(),
            name: String::new(),
            pixels: PixelRect { left, top, right, bottom },
            work: PixelRect { left, top, right, bottom },
            scale,
            primary,
        };
        let a = work_areas(&[mk(false, 0, -2160, 3840, -80, 2.0), mk(true, 0, 0, 1920, 1000, 0.0)]);
        // Primary first; zero scale clamps to 0.1 like the startup sizing.
        assert_eq!(a[0].max, egui::pos2(19200.0, 10000.0));
        assert_eq!(a[1].min, egui::pos2(0.0, -1080.0));
        assert_eq!(a[1].height(), 1040.0);
    }

    #[test]
    fn display_list_keeps_ids_and_names() {
        let m = MonitorInfo {
            id: "a".into(),
            name: "Left".into(),
            pixels: PixelRect { left: 0, top: 0, right: 10, bottom: 10 },
            work: PixelRect { left: 0, top: 0, right: 10, bottom: 10 },
            scale: 1.0,
            primary: true,
        };
        assert_eq!(
            displays(std::slice::from_ref(&m), Lang::En),
            vec![DisplayInfo { id: "a".into(), name: "Left".into() }]
        );
        let unnamed = MonitorInfo { name: "Screen 2".into(), ..m };
        let want = fp_text::screen_number(Lang::De, 2);
        assert_eq!(
            displays(std::slice::from_ref(&unnamed), Lang::De),
            vec![DisplayInfo { id: "a".into(), name: want }]
        );
        // Switching language relabels the same monitor list (apply_ui rebuilds host).
        let en = fp_text::screen_number(Lang::En, 2);
        assert_eq!(displays(&[unnamed], Lang::En)[0].name, en);
        assert_ne!(en, fp_text::screen_number(Lang::De, 2));
    }

    /// The user's screen: 2880x1800 px at 175 % (1645.7 x 1028.6 pt) with an
    /// 84 px bottom taskbar, polled by the real tracker with no windows.
    fn taskbar_screen() -> (Layout, Vec<ScreenSnap>) {
        let m = MonitorInfo {
            id: "a".into(),
            name: "a".into(),
            pixels: PixelRect::new(0, 0, 2880, 1800),
            work: PixelRect::new(0, 0, 2880, 1716),
            scale: 1.75,
            primary: true,
        };
        let layout = Layout::new(vec![m]);
        let frames: Vec<Frame> = layout.screens().iter().map(|s| s.points).collect();
        let shelves = [PixelRect::new(0, 1716, 2880, 1800)];
        let input = PollInput {
            listed: Vec::new(),
            cloaked_known: 0,
            screens: &frames,
            shelves: &shelves,
            layout: &layout,
            corner: 8.0,
        };
        let snaps = Tracker::new().poll(1.0, input).screens;
        (layout, snaps)
    }

    /// Runs a world on `feed` for `seconds`, rubbing snow at the drift on the
    /// taskbar top (48 pt) during the first second, and returns every ball
    /// sprite frame of the last step.
    fn rub_the_taskbar(feed: Arc<SnowFeed>, layout: &Layout, seconds: f64) -> Vec<Frame> {
        let mut world = World::new(feed.clone(), &serde_json::Map::new(), 7);
        let settings = Settings::default();
        let mut out = WorldOutput::default();
        let mut now = 10.0;
        for _ in 0..(seconds * 60.0) as usize {
            now += 1.0 / 60.0;
            if now < 11.0 {
                feed.gather(40.0, Vec2::new(800.0, 55.0), Vec2::new(20.0, 0.0), Vec2::ZERO);
            }
            let input = WorldInput {
                now,
                wall: july(),
                pointer: PointerInput::default(),
                settings: &settings,
                layout,
                snowing: true,
                main_screen: None,
                picker: None,
                windows: &[],
            };
            world.step(&input, &mut out);
        }
        out.sprites.iter().filter(|s| s.key.owner_byte() == Owner::Ball as u8).map(|s| s.frame).collect()
    }

    #[test]
    fn a_polled_snapshot_lets_balls_rest_on_the_taskbar() {
        let (layout, snaps) = taskbar_screen();
        let dock = snaps[0].surfaces.iter().find(|s| s.id == fp_core::surface::SurfaceId::Dock).unwrap();
        assert!((dock.frame.max_y() - 48.0).abs() < 1e-9);
        let feed = Arc::new(SnowFeed::new());
        publish_screens(&feed, &snaps);
        let balls = rub_the_taskbar(feed, &layout, 3.0);
        // One ball that grew from all the rubbing, resting on the taskbar top.
        assert_eq!(balls.len(), 1);
        let b = balls[0];
        assert!(b.min_y() < 48.0 && b.min_y() > 40.0 && b.max_y() > 60.0, "{b:?}");
    }

    #[test]
    fn without_the_snapshot_balls_fall_off_the_screen() {
        // The bug the poll hand-over fixes: no ledges, so balls never land.
        let (layout, _) = taskbar_screen();
        let balls = rub_the_taskbar(Arc::new(SnowFeed::new()), &layout, 3.0);
        assert!(!balls.is_empty());
        assert!(balls.iter().all(|b| b.max_y() < 0.0), "{balls:?}");
    }

    struct Release;

    impl fp_sources::Fetch for Release {
        fn get(&self, _: &str) -> Result<String, String> {
            Ok(r#"{"tag_name":"v0.1.0","html_url":"https://example.org/r"}"#.to_string())
        }
    }

    #[test]
    fn window_obstacles_are_filtered_and_in_points() {
        let l = Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "A".into(),
            pixels: PixelRect::new(0, 0, 1920, 1080),
            work: PixelRect::new(0, 0, 1920, 1040),
            scale: 1.0,
            primary: true,
        }]);
        let shown = fp_tracker::WindowInfo {
            hwnd: 1,
            visible: true,
            bounds: Some(PixelRect::new(100, 200, 500, 600)),
            ..Default::default()
        };
        let hidden = fp_tracker::WindowInfo { hwnd: 2, visible: false, ..shown.clone() };
        let got = window_obstacles(&l, &[shown, hidden]);
        assert_eq!(got, vec![Frame::new(100.0, 480.0, 400.0, 400.0)]);
    }

    #[test]
    fn asked_check_during_background_check_clears_checking() {
        let mut sources = Sources::new(Worker::with_fetch(Release, || {}), 0.0, "0.1.0");
        assert!(sources.check_update(false));
        let started = sources.check_update(true);
        assert!(!started);
        let line = check_line(started, sources.working(), 1.0);
        assert_eq!(line, UpdateLine::Checking);
        let mut events = Vec::new();
        for _ in 0..400 {
            events = sources.poll(|| (1, false));
            if !events.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let Some(Event::Update { asked, outcome }) = events.pop() else { panic!("no reply") };
        assert!(!asked);
        assert_eq!(settled_line(&line, asked, &outcome, 2.0), Some(UpdateLine::UpToDate { at: 2.0 }));
        assert_eq!(settled_line(&UpdateLine::Idle, asked, &outcome, 2.0), None);
    }

    #[test]
    fn failed_check_and_refused_request() {
        assert_eq!(
            settled_line(&UpdateLine::Checking, false, &UpdateOutcome::Failed, 3.0),
            Some(UpdateLine::Offline { at: 3.0 })
        );
        assert_eq!(
            settled_line(&UpdateLine::Idle, true, &UpdateOutcome::Failed, 3.0),
            Some(UpdateLine::Offline { at: 3.0 })
        );
        assert_eq!(
            settled_line(&UpdateLine::Checking, true, &UpdateOutcome::Latest, 3.0),
            Some(UpdateLine::UpToDate { at: 3.0 })
        );
        assert_eq!(check_line(false, false, 4.0), UpdateLine::Offline { at: 4.0 });
        assert_eq!(check_line(true, true, 4.0), UpdateLine::Checking);
    }

    struct Motion(std::cell::Cell<bool>);

    impl Platform for Motion {
        fn monitors(&self) -> Vec<MonitorInfo> {
            Vec::new()
        }
        fn data_dir(&self) -> PathBuf {
            PathBuf::new()
        }
        fn os_language(&self) -> String {
            "en".into()
        }
        fn power(&self) -> fp_win::PowerStatus {
            fp_win::PowerStatus::default()
        }
        fn reduced_motion(&self) -> bool {
            self.0.get()
        }
        fn cursor(&self) -> Option<(i32, i32)> {
            None
        }
        fn right_button_down(&self) -> bool {
            false
        }
        fn context_menu(&self, _: &str, _: (i32, i32)) -> bool {
            false
        }
        fn left_button_down(&self) -> bool {
            false
        }
        fn windows(&self) -> Vec<fp_tracker::WindowInfo> {
            Vec::new()
        }
        fn taskbar_shelves(&self) -> Vec<PixelRect> {
            Vec::new()
        }
        fn time_zone_id(&self) -> String {
            String::new()
        }
        fn window_corner(&self) -> f64 {
            0.0
        }
    }

    #[test]
    fn each_build_rereads_reduced_motion() {
        let p = Motion(std::cell::Cell::new(false));
        let feed = Arc::new(SnowFeed::new());
        let (a, _) = motion_factory(&p, EffectKind::Rain, feed.clone(), 1);
        p.0.set(true);
        let (b, _) = motion_factory(&p, EffectKind::Rain, feed, 1);
        assert!(!a);
        assert!(b);
    }

    #[test]
    fn unlock_and_display_on_do_not_resume_sources() {
        let mut s = SleepState::default();
        system_actions(&mut s, SystemEvent::Locked);
        let a = system_actions(&mut s, SystemEvent::Unlocked);
        assert!(a.wake && !a.resume);
        system_actions(&mut s, SystemEvent::DisplayOff);
        let a = system_actions(&mut s, SystemEvent::DisplayOn);
        assert!(a.wake && !a.resume);
    }

    #[test]
    fn resume_restarts_sources_even_while_locked() {
        let mut s = SleepState::default();
        system_actions(&mut s, SystemEvent::Locked);
        let a = system_actions(&mut s, SystemEvent::Suspend);
        assert!(!a.pause && !a.resume);
        let a = system_actions(&mut s, SystemEvent::Resume);
        assert!(a.resume && !a.wake && s.asleep());
        let mut s = SleepState::default();
        assert!(system_actions(&mut s, SystemEvent::Suspend).pause);
    }

    #[test]
    fn other_events_map_to_their_actions() {
        let mut s = SleepState::default();
        assert!(system_actions(&mut s, SystemEvent::DisplaysChanged).rebuild);
        assert!(system_actions(&mut s, SystemEvent::ThemeChanged).theme);
        assert!(system_actions(&mut s, SystemEvent::PowerChanged).power);
    }

    #[test]
    fn queued_display_events_rebuild_once_per_tick() {
        let mut s = SleepState::default();
        let mut pending = false;
        for _ in 0..3 {
            pending |= system_actions(&mut s, SystemEvent::DisplaysChanged).rebuild;
        }
        assert!(take_rebuild(&mut pending, true));
        assert!(!take_rebuild(&mut pending, false));
        assert!(take_rebuild(&mut pending, true));
        assert!(!pending);
    }
}
