//! `Platform` + `Shell` + `SystemEvents` + `Driver` on X11: display, window
//! source, pointer and hooks come from X; session services (data folder,
//! language, time zone, single instance, power, theme, autostart, wallpaper,
//! `xdg-open`) are delegated to `fp-linux`.

use crate::watch;
use crate::x::Xc;
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_platform::{
    Driver, Guard, Instance, Platform, PowerStatus, Services, Shell, SystemEvent, SystemEvents, WallpaperScreen,
};
use fp_tracker::{HookEvent, WindowInfo};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use x11rb::protocol::xproto::{ConnectionExt as _, KeyButMask, MapState};

type OnEvent = Rc<RefCell<Box<dyn FnMut(SystemEvent, Option<bool>)>>>;

thread_local! {
    /// `SystemEvents::watch` callbacks of this (UI) thread.
    static WATCHERS: RefCell<Vec<(u64, OnEvent)>> = const { RefCell::new(Vec::new()) };
    static NEXT: RefCell<u64> = const { RefCell::new(0) };
}

/// Delivers queued X system events (RandR display changes) to the watchers.
fn deliver() {
    let events = watch::take_system();
    if events.is_empty() {
        return;
    }
    let list: Vec<OnEvent> = WATCHERS.with(|w| w.borrow().iter().map(|(_, f)| f.clone()).collect());
    for e in events {
        for f in &list {
            if let Ok(mut f) = f.try_borrow_mut() {
                f(e, None);
            }
        }
    }
}

struct WatchGuard {
    id: u64,
    _inner: Option<Guard>,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        let id = self.id;
        WATCHERS.with(|w| w.borrow_mut().retain(|(i, _)| *i != id));
    }
}

struct HookGuard;

impl Drop for HookGuard {
    fn drop(&mut self) {
        watch::set_hooks(None);
    }
}

/// The X11 services.
pub struct X11Platform {
    pub(crate) x: &'static Xc,
    pub(crate) inner: Box<dyn Services>,
}

impl X11Platform {
    fn pointer_mask(&self) -> u16 {
        self.x.pointer().map_or(0, |p| p.2)
    }

    /// Root child at `at`, top-down, skipping click-through windows of ours.
    fn top_at(&self, at: (i32, i32)) -> Option<u32> {
        if let Some((px, py, _, child)) = self.x.pointer()
            && (px, py) == at
        {
            return Some(child).filter(|c| *c != 0);
        }
        let own = self.x.own();
        let skip: Vec<u32> = own.panels.iter().chain(own.click_through.iter()).copied().collect();
        drop(own);
        let c = &self.x.conn;
        for w in self.x.stack().into_iter().rev().filter(|w| !skip.contains(w)) {
            let viewable = c
                .get_window_attributes(w)
                .ok()
                .and_then(|r| r.reply().ok())
                .is_some_and(|a| a.map_state == MapState::VIEWABLE);
            if !viewable {
                continue;
            }
            if let Some(g) = c.get_geometry(w).ok().and_then(|r| r.reply().ok()) {
                let (x, y) = (i32::from(g.x), i32::from(g.y));
                let (r, b) = (x + i32::from(g.width), y + i32::from(g.height));
                if at.0 >= x && at.0 < r && at.1 >= y && at.1 < b {
                    return Some(w);
                }
            }
        }
        None
    }
}

impl Platform for X11Platform {
    fn monitors(&self) -> Vec<MonitorInfo> {
        self.x.monitors()
    }
    fn data_dir(&self) -> PathBuf {
        self.inner.data_dir()
    }
    fn os_language(&self) -> String {
        self.inner.os_language()
    }
    fn power(&self) -> PowerStatus {
        self.inner.power()
    }
    fn reduced_motion(&self) -> bool {
        self.inner.reduced_motion()
    }
    /// Also the UI-thread point where queued X events are delivered (the
    /// app reads the cursor every tick).
    fn cursor(&self) -> Option<(i32, i32)> {
        deliver();
        self.x.drain();
        self.x.pointer().map(|(x, y, _, _)| (x, y))
    }
    fn left_button_down(&self) -> bool {
        self.pointer_mask() & u16::from(KeyButMask::BUTTON1) != 0
    }
    fn right_button_down(&self) -> bool {
        self.pointer_mask() & u16::from(KeyButMask::BUTTON3) != 0
    }
    fn context_menu(&self, label: &str, at: (i32, i32)) -> bool {
        self.inner.context_menu(label, at)
    }
    fn windows(&self) -> Vec<WindowInfo> {
        self.x.windows()
    }
    fn taskbar_shelves(&self) -> Vec<PixelRect> {
        self.x.shelves()
    }
    fn time_zone_id(&self) -> String {
        self.inner.time_zone_id()
    }
    fn window_corner(&self) -> f64 {
        0.0
    }
    fn init_process(&self) {
        self.inner.init_process();
    }
    fn utc_offset_s(&self) -> i32 {
        self.inner.utc_offset_s()
    }
    fn uptime_ms(&self) -> u64 {
        self.inner.uptime_ms()
    }
    fn over_own(&self, at: (i32, i32)) -> bool {
        let Some(w) = self.top_at(at) else { return false };
        let own = self.x.own();
        if own.panels.contains(&w) || own.sprites.contains_key(&w) {
            return true;
        }
        drop(own);
        self.x.own_frames(false).contains(&w)
    }
    fn hooks(&self, tx: Sender<HookEvent>) -> Option<Guard> {
        watch::set_hooks(Some(tx));
        watch::ensure();
        Some(Box::new(HookGuard))
    }
    fn has_geometry(&self) -> bool {
        true
    }
    fn shelves_reader(&self) -> Box<dyn FnOnce() -> Vec<PixelRect> + Send> {
        let x = self.x;
        Box::new(move || x.shelves())
    }
    fn single_instance(&self, quiet: bool) -> Instance {
        self.inner.single_instance(quiet)
    }
}

impl Shell for X11Platform {
    fn open_url(&self, url: &str) {
        self.inner.open_url(url);
    }
    fn login_is_on(&self) -> bool {
        self.inner.login_is_on()
    }
    fn login_blocked(&self) -> bool {
        self.inner.login_blocked()
    }
    fn login_set(&self, on: bool) -> bool {
        self.inner.login_set(on)
    }
    fn show_startup_apps(&self) {
        self.inner.show_startup_apps();
    }
    fn taskbar_light(&self) -> bool {
        self.inner.taskbar_light()
    }
    fn tray_icon_promoted(&self) -> bool {
        self.inner.tray_icon_promoted()
    }
    fn tray_icon_spot(&self, rect: PixelRect) -> Option<PixelRect> {
        self.inner.tray_icon_spot(rect)
    }
    fn region_latitude(&self) -> Option<f64> {
        self.inner.region_latitude()
    }
    fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
        self.inner.wallpaper_screens()
    }
    fn wallpaper_set(&self, path: &Path) -> bool {
        self.inner.wallpaper_set(path)
    }
    fn wallpaper_set_each(&self, items: &[(String, PathBuf)]) -> bool {
        self.inner.wallpaper_set_each(items)
    }
    fn wallpaper_current(&self) -> Option<PathBuf> {
        self.inner.wallpaper_current()
    }
    fn wallpaper_supported(&self) -> bool {
        self.inner.wallpaper_supported()
    }
    fn open_folder(&self, dir: &Path) {
        self.inner.open_folder(dir);
    }
}

impl SystemEvents for X11Platform {
    /// The session watcher of `fp-linux` plus RandR display changes.
    fn watch(&self, on: Box<dyn FnMut(SystemEvent, Option<bool>)>) -> Option<Guard> {
        let shared: OnEvent = Rc::new(RefCell::new(on));
        let fwd = shared.clone();
        let inner = self.inner.watch(Box::new(move |e, s| {
            if let Ok(mut f) = fwd.try_borrow_mut() {
                f(e, s);
            }
        }));
        let id = NEXT.with(|n| {
            let mut n = n.borrow_mut();
            *n += 1;
            *n
        });
        WATCHERS.with(|w| w.borrow_mut().push((id, shared)));
        watch::ensure();
        Some(Box::new(WatchGuard { id, _inner: inner }))
    }
}

impl Driver for X11Platform {
    /// `None`: the app's 64 Hz loop wrapper ticks (porting design 3.4.1).
    fn start(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<Guard> {
        self.inner.start(every_ms, tick)
    }
}
