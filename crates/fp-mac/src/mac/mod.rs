//! The macOS implementation of the platform seam.

mod instance;
mod notify;
pub mod overlay;
mod screens;
mod shell;
mod sprites;
mod window;
mod winlist;

use crate::logic;
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_platform::{
    Driver, Guard, Instance, Limits, Parts, Platform, PowerStatus, Shell, SleepState, SystemEvent, SystemEvents,
    WallpaperScreen,
};
use fp_tracker::synth::HookSynth;
use fp_tracker::{HookEvent, WindowInfo};
use notify::{Observers, Timer};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSWorkspace};
use objc2_foundation::{NSDistributedNotificationCenter, NSNotificationCenter};
use overlay::MacOverlay;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// How long a Dock shelf read is reused (the reader is asked for every tick).
const SHELVES_FOR: Duration = Duration::from_millis(500);

type HookTx = Rc<RefCell<Option<Sender<HookEvent>>>>;

/// The macOS `Platform`, `Shell`, `SystemEvents` and `Driver`. Main thread only.
pub struct Mac {
    mtm: MainThreadMarker,
    started: Instant,
    synth: RefCell<HookSynth>,
    hook_tx: HookTx,
    space_switched: Rc<Cell<bool>>,
    shelves: RefCell<Option<(Instant, Vec<PixelRect>)>>,
}

impl Mac {
    pub fn new(mtm: MainThreadMarker) -> Self {
        Self {
            mtm,
            started: Instant::now(),
            synth: RefCell::new(HookSynth::new()),
            hook_tx: Rc::default(),
            space_switched: Rc::default(),
            shelves: RefCell::new(None),
        }
    }
}

/// The macOS backends. Must run on the main thread (it does: `main`).
pub fn backend() -> Parts<MacOverlay> {
    let mtm = MainThreadMarker::new().expect("fp_mac::backend must run on the main thread");
    Parts {
        services: Rc::new(Mac::new(mtm)),
        sprites: Box::new(sprites::MacSprites::new(mtm)),
        overlay: MacOverlay::new(mtm),
        limits: Limits::default(),
    }
}

/// Window hooks: removes the observers and stops synthesised events on drop.
struct Hooks {
    _obs: Observers,
    tx: HookTx,
}

impl Drop for Hooks {
    fn drop(&mut self) {
        self.tx.borrow_mut().take();
    }
}

impl Platform for Mac {
    fn monitors(&self) -> Vec<MonitorInfo> {
        let (raws, _) = screens::read(self.mtm);
        logic::monitors(&raws).0
    }

    fn data_dir(&self) -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Library/Application Support/Frostpane")
    }

    fn os_language(&self) -> String {
        shell::language()
            .or_else(|| std::env::var("LANG").ok().and_then(|l| logic::lang_code(&l)))
            .unwrap_or_else(|| "en".into())
    }

    fn power(&self) -> PowerStatus {
        shell::power()
    }

    fn reduced_motion(&self) -> bool {
        shell::reduced_motion()
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        let p = NSEvent::mouseLocation();
        Some(screens::space().cocoa_px(p.x, p.y))
    }

    fn left_button_down(&self) -> bool {
        logic::buttons(NSEvent::pressedMouseButtons()).0
    }

    fn right_button_down(&self) -> bool {
        logic::buttons(NSEvent::pressedMouseButtons()).1
    }

    fn context_menu(&self, label: &str, at: (i32, i32)) -> bool {
        shell::context_menu(self.mtm, label, at)
    }

    /// Polled listing (porting design 3.1 "Window source"), diffed into
    /// hook events; windows that left with a Space switch are listed as
    /// cloaked so the tracker sees the switch.
    fn windows(&self) -> Vec<WindowInfo> {
        let sp = screens::space();
        let own = winlist::own_pid();
        let mut out: Vec<WindowInfo> =
            winlist::on_screen().iter().filter_map(|w| logic::portable(w, own, sp)).map(WindowInfo::portable).collect();
        let mut synth = self.synth.borrow_mut();
        if self.space_switched.replace(false) {
            synth.space_switched();
        }
        let events = synth.poll(self.started.elapsed().as_secs_f64(), &out);
        if let Some(tx) = self.hook_tx.borrow().as_ref() {
            for e in events {
                let _ = tx.send(e);
            }
        }
        out.extend(synth.take_cloaked().into_iter().map(|id| WindowInfo::portable(logic::cloaked(id))));
        out
    }

    fn taskbar_shelves(&self) -> Vec<PixelRect> {
        let mut cache = self.shelves.borrow_mut();
        if let Some((at, list)) = cache.as_ref()
            && at.elapsed() < SHELVES_FOR
        {
            return list.clone();
        }
        let (raws, sp) = screens::read(self.mtm);
        let list = logic::shelves(&raws, sp);
        *cache = Some((Instant::now(), list.clone()));
        list
    }

    fn time_zone_id(&self) -> String {
        shell::time_zone()
    }

    fn window_corner(&self) -> f64 {
        logic::WINDOW_CORNER
    }

    fn utc_offset_s(&self) -> i32 {
        shell::utc_offset_s()
    }

    fn uptime_ms(&self) -> u64 {
        shell::uptime_ms()
    }

    /// The window under the point belongs to this app.
    fn over_own(&self, at: (i32, i32)) -> bool {
        let (x, y) = screens::space().px_cocoa(at.0, at.1);
        let n = objc2_app_kit::NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(
            objc2_foundation::NSPoint::new(x, y),
            0,
            self.mtm,
        );
        n > 0 && window::by_number(self.mtm, n as i64).is_some()
    }

    /// Foreground changes from app activation; Space switches mark the next
    /// poll. Moves and destroys come from [`Platform::windows`].
    fn hooks(&self, tx: Sender<HookEvent>) -> Option<Guard> {
        *self.hook_tx.borrow_mut() = Some(tx);
        let ws = NSWorkspace::sharedWorkspace().notificationCenter();
        let mut obs = Observers::default();
        let fg = self.hook_tx.clone();
        obs.add(&ws, "NSWorkspaceDidActivateApplicationNotification", move |_| {
            if let Some(tx) = fg.borrow().as_ref() {
                let _ = tx.send(HookEvent::Foreground);
            }
        });
        let flag = self.space_switched.clone();
        obs.add(&ws, "NSWorkspaceActiveSpaceDidChangeNotification", move |_| flag.set(true));
        Some(Box::new(Hooks { _obs: obs, tx: self.hook_tx.clone() }))
    }

    fn has_geometry(&self) -> bool {
        true
    }

    /// AppKit is main-thread only, so the read happens here (cached) and the
    /// background thread only hands it over.
    fn shelves_reader(&self) -> Box<dyn FnOnce() -> Vec<PixelRect> + Send> {
        let list = self.taskbar_shelves();
        Box::new(move || list)
    }

    fn single_instance(&self, quiet: bool) -> Instance {
        instance::acquire(&self.data_dir(), quiet)
    }
}

impl Shell for Mac {
    fn open_url(&self, url: &str) {
        shell::open_url(url);
    }
    fn login_is_on(&self) -> bool {
        shell::login_is_on()
    }
    fn login_blocked(&self) -> bool {
        shell::login_blocked()
    }
    fn login_set(&self, on: bool) -> bool {
        shell::login_set(on)
    }
    fn show_startup_apps(&self) {
        shell::show_login_items();
    }
    /// The tray icon is a template image the menu bar tints, so this only
    /// matters for the onboarding sketch.
    fn taskbar_light(&self) -> bool {
        shell::light_theme()
    }
    /// Menu bar extras are always shown.
    fn tray_icon_promoted(&self) -> bool {
        true
    }
    /// `TrayIcon::rect` is the status item's own frame.
    fn tray_icon_spot(&self, rect: PixelRect) -> Option<PixelRect> {
        (!rect.is_empty()).then_some(rect)
    }
    fn region_latitude(&self) -> Option<f64> {
        None
    }
    fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
        shell::wallpaper_screens(self.mtm)
    }
    fn wallpaper_set(&self, path: &Path) -> bool {
        shell::wallpaper_set(self.mtm, path)
    }
    fn wallpaper_set_each(&self, items: &[(String, PathBuf)]) -> bool {
        shell::wallpaper_set_each(self.mtm, items)
    }
    fn wallpaper_current(&self) -> Option<PathBuf> {
        shell::wallpaper_current(self.mtm)
    }
    fn wallpaper_supported(&self) -> bool {
        true
    }
    fn open_folder(&self, dir: &Path) {
        shell::open_folder(dir);
    }
}

impl SystemEvents for Mac {
    /// Workspace (sleep, screens, session), distributed (lock, theme) and app
    /// (screen parameters, Low Power Mode) notifications, on the main queue.
    fn watch(&self, on: Box<dyn FnMut(SystemEvent, Option<bool>)>) -> Option<Guard> {
        let state = Rc::new(RefCell::new((SleepState::default(), on)));
        let mut obs = Observers::default();
        let centers: [(objc2::rc::Retained<NSNotificationCenter>, &[&str]); 3] = [
            (NSWorkspace::sharedWorkspace().notificationCenter(), &logic::WORKSPACE_EVENTS),
            (
                objc2::rc::Retained::into_super(NSDistributedNotificationCenter::defaultCenter()),
                &logic::DISTRIBUTED_EVENTS,
            ),
            (NSNotificationCenter::defaultCenter(), &logic::APP_EVENTS),
        ];
        for (center, names) in centers {
            for name in names {
                let st = state.clone();
                obs.add(&center, name, move |n| {
                    let Some(ev) = logic::system_event(n) else { return };
                    match st.try_borrow_mut() {
                        Ok(mut g) => {
                            let (sleep, on) = &mut *g;
                            let changed = sleep.apply(ev);
                            on(ev, changed);
                        }
                        Err(_) => log::warn!("system event {ev:?} dropped (handler busy)"),
                    }
                });
            }
        }
        (!obs.is_empty()).then(|| Box::new(obs) as Guard)
    }
}

impl Driver for Mac {
    /// An `NSTimer` in the common modes: it keeps ticking while a menu tracks,
    /// which winit's own loop does not.
    fn start(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<Guard> {
        Some(Box::new(Timer::start(every_ms, tick)))
    }
}
