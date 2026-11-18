//! `Platform` on Wayland: the `fp-linux` session services plus what only the
//! compositor connection and the window source know (outputs, the pointer
//! over our surfaces, foreign windows, panel shelves).

use super::Shared;
use super::sources::{Source, Wake};
use crate::logic::{self, Snapshot};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_platform::{Driver, Guard, Instance, Platform, PowerStatus, Shell, SystemEvent, SystemEvents, WallpaperScreen};
use fp_tracker::synth::HookSynth;
use fp_tracker::{HookEvent, WindowInfo};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::PoisonError;
use std::sync::mpsc::Sender;
use std::time::Instant;

pub struct WlServices<T> {
    pub(super) inner: T,
    pub(super) wl: Option<Shared>,
    pub(super) source: RefCell<Option<Box<dyn Source>>>,
    pub(super) snap: Rc<RefCell<Snapshot>>,
    pub(super) wake: Wake,
    pub(super) synth: RefCell<HookSynth>,
    pub(super) start: Instant,
}

/// Clears the hook sender when dropped.
struct HookGuard(Wake);

impl Drop for HookGuard {
    fn drop(&mut self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

impl<T> WlServices<T> {
    fn outputs(&self) -> Vec<logic::Output> {
        let Some(wl) = &self.wl else { return Vec::new() };
        let mut wl = wl.borrow_mut();
        wl.dispatch();
        wl.outputs().into_iter().map(|(_, o)| o).collect()
    }

    fn scale(&self) -> f64 {
        fp_platform::coords::global_scale(&self.outputs().iter().map(|o| o.scale).collect::<Vec<_>>())
    }

    /// Re-lists the source; keeps the last snapshot on failure.
    fn refresh(&self) {
        if let Some(src) = self.source.borrow_mut().as_mut()
            && let Some(s) = src.list()
        {
            *self.snap.borrow_mut() = s;
        }
    }

    fn send(&self, events: Vec<HookEvent>) {
        if let Some(tx) = self.wake.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            for e in events {
                let _ = tx.send(e);
            }
        }
    }
}

impl<T: Platform> Platform for WlServices<T> {
    /// xdg-output layout times the largest output scale (design 2.4).
    fn monitors(&self) -> Vec<MonitorInfo> {
        let outs = self.outputs();
        if outs.is_empty() {
            return self.inner.monitors();
        }
        if self.snap.borrow().reserved.is_empty() {
            self.refresh();
        }
        let (s, list) = logic::monitors(&outs, &self.snap.borrow().reserved);
        if let Some(wl) = &self.wl {
            wl.borrow_mut().st.scale = s;
        }
        list
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
    /// Only over our own surfaces: Wayland has no global pointer.
    fn cursor(&self) -> Option<(i32, i32)> {
        if let Some(wl) = &self.wl {
            wl.borrow_mut().dispatch();
        }
        super::wl::cursor()
    }
    fn left_button_down(&self) -> bool {
        self.wl.as_ref().is_some_and(|w| w.borrow().st.left)
    }
    fn right_button_down(&self) -> bool {
        self.wl.as_ref().is_some_and(|w| w.borrow().st.right)
    }
    fn context_menu(&self, label: &str, at: (i32, i32)) -> bool {
        self.inner.context_menu(label, at)
    }
    /// The window source's listing (top first); drags and closes are
    /// synthesised from successive listings.
    fn windows(&self) -> Vec<WindowInfo> {
        if self.source.borrow().is_none() {
            return Vec::new();
        }
        self.refresh();
        let infos = logic::to_infos(&self.snap.borrow().windows, self.scale(), std::process::id());
        let events = self.synth.borrow_mut().poll(self.start.elapsed().as_secs_f64(), &infos);
        self.send(events);
        infos
    }
    fn taskbar_shelves(&self) -> Vec<PixelRect> {
        let outs = self.outputs();
        let s = fp_platform::coords::global_scale(&outs.iter().map(|o| o.scale).collect::<Vec<_>>());
        logic::shelves(&outs, &self.snap.borrow().reserved, s)
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
    /// On one of our sprites, or on a window of this process the source sees.
    fn over_own(&self, at: (i32, i32)) -> bool {
        let Some(wl) = &self.wl else { return false };
        let (x, y) = (f64::from(at.0), f64::from(at.1));
        if wl.borrow().st.own_rects.values().any(|r| r.contains(x, y)) {
            return true;
        }
        self.snap.borrow().own_at(std::process::id(), wl.borrow().st.scale, at)
    }
    fn hooks(&self, tx: Sender<HookEvent>) -> Option<Guard> {
        self.source.borrow().as_ref()?;
        *self.wake.lock().unwrap_or_else(PoisonError::into_inner) = Some(tx);
        Some(Box::new(HookGuard(self.wake.clone())))
    }
    fn has_geometry(&self) -> bool {
        self.source.borrow().is_some()
    }
    /// Shelves are known on the UI thread already (source listing).
    fn shelves_reader(&self) -> Box<dyn FnOnce() -> Vec<PixelRect> + Send> {
        let list = self.taskbar_shelves();
        Box::new(move || list)
    }
    fn single_instance(&self, quiet: bool) -> Instance {
        self.inner.single_instance(quiet)
    }
}

impl<T: Shell> Shell for WlServices<T> {
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

impl<T: SystemEvents> SystemEvents for WlServices<T> {
    fn watch(&self, on: Box<dyn FnMut(SystemEvent, Option<bool>)>) -> Option<Guard> {
        self.inner.watch(on)
    }
}

impl<T: Driver> Driver for WlServices<T> {
    /// No compositor timer worth using: the app's loop wrapper ticks.
    fn start(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<Guard> {
        self.inner.start(every_ms, tick)
    }
}
