//! The portable overlay host: one render thread per monitor, the above
//! panel and the staged behind-windows z-order (desktop-native 3.1-3.4, 3.8).
//! Panel windows come from an [`OverlayBackend`] (Win32 + DirectComposition
//! in `fp_win::overlay`).

use crate::Config;
use crate::gpu::{Gpu, Slots};
use crate::plan::{self, Act, PanelInfo, SitPlan, Stager};
use fp_core::desktop::MonitorInfo;
use fp_core::falling::Falling;
use fp_core::frame::FrameTimer;
use fp_core::gpu::{DrawList, Layer};
use fp_core::surface::ScreenSnap;
use fp_core::wind::Wind;
use fp_core::{Frame, PixelRect};
use fp_platform::Guard;
use fp_platform::overlay::{OverlayBackend, PanelTarget as Target, PanelWindow, Stack};
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A panel window as the host keeps it.
type PanelWin = Box<dyn PanelWindow>;

/// How every panel's swap chain is configured (one choice per host).
#[derive(Clone, Copy, Debug)]
struct Present {
    alpha: wgpu::CompositeAlphaMode,
    mode: wgpu::PresentMode,
}

fn surface_config(size: (u32, u32), p: Present) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: crate::gpu::FORMAT,
        width: size.0,
        height: size.1,
        present_mode: p.mode,
        // Two swap chain buffers (desktop-native 3.2); wgpu adds one buffer
        // to the latency. Each extra full-screen buffer is 4 bytes a pixel of
        // committed memory on an integrated GPU.
        desired_maximum_frame_latency: 1,
        alpha_mode: p.alpha,
        view_formats: vec![],
        color_space: Default::default(),
    }
}

/// `KeepOnTop` for one screen: topmost panels, or in behind mode the main
/// panel right above the desktop shell. No-op while the screen is hidden.
fn keep_on_top(s: &Screen) {
    if !s.shown {
        return;
    }
    if !s.behind {
        s.main.stack(Stack::Front);
    }
    if let Some(a) = &s.above {
        a.stack(Stack::Front);
    }
    if s.behind {
        s.main.stack(Stack::Behind);
    }
}

// ---- render loop ---------------------------------------------------------

/// What the loop thread presents for one panel this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Draw,
    Clear,
    Idle,
}

struct Gate {
    fall: Option<Box<dyn Falling>>,
    above: Option<Arc<Target>>,
    /// The above panel was cleared since it last had content (3.3).
    above_cleared: bool,
    /// The main panel must be cleared once (effect replaced or removed).
    main_dirty: bool,
}

/// State shared between the UI thread and one monitor's render thread.
struct Loop {
    main: Arc<Target>,
    gate: Mutex<Gate>,
    stop: AtomicBool,
    /// Paused <=> the screen is hidden (`resume` event of 3.8).
    run: Mutex<bool>,
    wake: Condvar,
    frames: AtomicU64,
    size: (f32, f32),
    /// Swap chain size of the main (and above) panel.
    swap: (u32, u32),
    /// Swap chain pixels per point when the backend sizes the swap chain
    /// above the DIP size (X11); 1 otherwise. Scales the shaders' edge width.
    px_per_pt: f32,
    pixels: PixelRect,
    scale: f64,
    present: Present,
}

impl Loop {
    fn with_fall<R>(&self, f: impl FnOnce(&mut dyn Falling) -> R) -> Option<R> {
        let mut g = lock(&self.gate);
        g.fall.as_mut().map(|b| f(b.as_mut()))
    }

    fn set_run(&self, on: bool) {
        *lock(&self.run) = on;
        self.wake.notify_all();
    }
}

struct Work {
    main: Job,
    above: Option<(Arc<Target>, Job)>,
}

/// Steps the simulation and fills the draw lists under the gate (3.8 step 5).
fn prepare<B: OverlayBackend>(sh: &Loop, dt: f64, time: f64, main: &mut DrawList, above: &mut DrawList) -> Work {
    let cursor = cursor_now::<B>(sh);
    let (w, h) = sh.size;
    let px = sh.px_per_pt;
    let mut guard = lock(&sh.gate);
    let Gate { fall, above: above_panel, above_cleared, main_dirty } = &mut *guard;
    let Some(fall) = fall else {
        let main_job = if std::mem::take(main_dirty) { Job::Clear } else { Job::Idle };
        let above_job = above_panel.clone().filter(|_| !*above_cleared).map(|a| (a, Job::Clear));
        if above_job.is_some() {
            *above_cleared = true;
        }
        return Work { main: main_job, above: above_job };
    };
    // One wall-clock time base for the changing wind on every monitor
    // (other-weather 1.1).
    if let Ok(wall) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        fall.set_wind_time(wall.as_secs_f64());
    }
    fall.step(dt, cursor);
    fill(main, w, h, time, px, |l| fall.draw(Layer::Main, l));
    *main_dirty = false;
    let above_job = above_panel.clone().and_then(|a| {
        let has = fall.has_above();
        if !has && *above_cleared {
            return None;
        }
        fill(above, w, h, time, px, |l| fall.draw(Layer::Above, l));
        *above_cleared = !has;
        Some((a, Job::Draw))
    });
    Work { main: Job::Draw, above: above_job }
}

/// Resets a list to the panel's scene and lets the simulation draw into it.
/// `px` (swap chain pixels per point, see `Loop::px_per_pt`) multiplies the
/// scene scale.
fn fill(list: &mut DrawList, w: f32, h: f32, time: f64, px: f32, draw: impl FnOnce(&mut DrawList)) {
    list.clear();
    list.scene = [w, h, time as f32, 1.0];
    draw(list);
    if !(list.scene[0] > 0.0 && list.scene[1] > 0.0) {
        list.scene[0] = w;
        list.scene[1] = h;
    }
    if list.scene[3].is_nan() || list.scene[3] <= 0.0 {
        list.scene[3] = 1.0;
    }
    if px != 1.0 {
        list.scene[3] *= px;
    }
}

fn cursor_now<B: OverlayBackend>(sh: &Loop) -> Option<fp_core::Vec2> {
    let (x, y) = B::cursor_px()?;
    plan::cursor_local(sh.pixels, sh.scale, x, y)
}

/// Acquires the next buffer, draws `list` (or clears), presents.
/// An `Outdated` surface is reconfigured (frame skipped); other failures
/// (timeout, occluded, lost) skip the frame.
fn present(
    gpu: &Gpu,
    gate: &RwLock<()>,
    slots: &mut Slots,
    target: &Target,
    swap: (u32, u32),
    present: Present,
    list: Option<&DrawList>,
) -> bool {
    // Held for the whole frame: surface configuration takes the write side.
    let frame = gate.read().unwrap_or_else(PoisonError::into_inner);
    match target.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
            let view = f.texture.create_view(&Default::default());
            gpu.render(slots, &view, list);
            gpu.queue.present(f);
            true
        }
        wgpu::CurrentSurfaceTexture::Outdated => {
            drop(frame);
            let _w = gate.write().unwrap_or_else(PoisonError::into_inner);
            target.surface.configure(&gpu.device, &surface_config(swap, present));
            false
        }
        _ => false,
    }
}

/// The frame loop of one monitor (desktop-native 3.8): 60 fps cap on top of
/// Fifo vsync, dt clamp, main panel then above panel.
fn render_loop<B: OverlayBackend>(sh: Arc<Loop>, gpu: Arc<Gpu>, gate: Arc<RwLock<()>>) {
    B::raise_priority();
    let swap = sh.swap;
    let pres = sh.present;
    let mut slots = Slots::new(&gpu);
    let mut timer = FrameTimer::new();
    let epoch = Instant::now();
    let frame_time = Duration::from_secs_f64(1.0 / 60.0);
    let mut next = epoch;
    let (mut main_list, mut above_list) = (DrawList::default(), DrawList::default());
    let mut paused = false;
    let stats_on = log::log_enabled!(log::Level::Debug);
    let mut stats = plan::FrameStats::new(0.0);
    while !sh.stop.load(Ordering::Relaxed) {
        {
            let run = lock(&sh.run);
            if !*run {
                paused = true;
                drop(sh.wake.wait_timeout(run, Duration::from_millis(100)).unwrap_or_else(PoisonError::into_inner));
                continue;
            }
        }
        if paused {
            paused = false;
            timer.reset();
            stats.pause();
            next = Instant::now();
        }
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        next = next.max(now) + frame_time;
        let time = epoch.elapsed().as_secs_f64();
        let dt = timer.tick(time);
        let work = prepare::<B>(&sh, dt, time, &mut main_list, &mut above_list);
        if work.main == Job::Idle && work.above.is_none() {
            stats.pause();
            std::thread::sleep(Duration::from_millis(50));
            next = Instant::now();
            continue;
        }
        if work.main == Job::Draw {
            gpu.warm_families(&main_list);
        }
        let prepared = epoch.elapsed().as_secs_f64();
        let list = match work.main {
            Job::Draw => Some(&main_list),
            _ => None,
        };
        if work.main != Job::Idle && present(&gpu, &gate, &mut slots, &sh.main, swap, pres, list) {
            sh.frames.fetch_add(1, Ordering::Relaxed);
        }
        if let Some((target, job)) = work.above {
            // The above panel has the main panel's size (3.3).
            let list = (job == Job::Draw).then_some(&above_list);
            present(&gpu, &gate, &mut slots, &target, swap, pres, list);
        }
        if stats_on {
            let done = epoch.elapsed().as_secs_f64();
            stats.frame(time, prepared - time, done - prepared, frame_time.as_secs_f64());
            if let Some(r) = stats.report(done) {
                log::debug!(
                    "overlay frames: {} ({:.1} fps), gap avg {:.2} ms max {:.2} ms, {} late; prepare {:.3} ms, present {:.3} ms",
                    r.frames,
                    r.fps,
                    r.gap_avg_ms,
                    r.gap_max_ms,
                    r.late,
                    r.prepare_ms,
                    r.present_ms
                );
            }
        }
    }
}

fn spawn_loop<B: OverlayBackend>(sh: Arc<Loop>, gpu: Arc<Gpu>, gate: Arc<RwLock<()>>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("frostpane overlay".into())
        .spawn(move || {
            if std::panic::catch_unwind(AssertUnwindSafe(|| render_loop::<B>(sh, gpu, gate))).is_err() {
                log::error!("overlay render thread panicked and stopped");
            }
        })
        .expect("spawn overlay thread")
}

// ---- screens and host ----------------------------------------------------

struct Screen {
    info: PanelInfo,
    pixels: PixelRect,
    main: PanelWin,
    above: Option<PanelWin>,
    /// Panels detached from the loop, released when their time is up.
    leaving: Vec<(f64, PanelWin)>,
    shared: Arc<Loop>,
    thread: Option<JoinHandle<()>>,
    shown: bool,
    behind: bool,
    stager: Stager,
}

struct State<B> {
    backend: B,
    /// Alpha and present mode of every panel's swap chain.
    present: Present,
    gpu: Arc<Gpu>,
    instance: wgpu::Instance,
    /// Frames hold it for reading, surface configuration for writing.
    gate: Arc<RwLock<()>>,
    cfg: Config,
    screens: Vec<Screen>,
    epoch: Instant,
    /// Host windows kept under the taskbar; never re-raised (3.4).
    under_dock: Vec<isize>,
}

/// Creates the above panel of a screen (configured under the render gate).
fn create_above<B: OverlayBackend>(
    backend: &B,
    present: Present,
    instance: &wgpu::Instance,
    gpu: &Gpu,
    gate: &RwLock<()>,
    s: &mut Screen,
) -> Result<(), String> {
    // The above panel has the main panel's swap chain size (3.3).
    let swap = s.shared.swap;
    let panel = backend.create_panel(instance, s.pixels, swap, s.info.scale)?;
    {
        let _w = gate.write().unwrap_or_else(PoisonError::into_inner);
        panel.target().surface.configure(&gpu.device, &surface_config(swap, present));
    }
    panel.commit();
    panel.show(s.shown);
    s.above = Some(panel);
    keep_on_top(s);
    Ok(())
}

fn attach_above(s: &Screen) {
    if let Some(p) = &s.above {
        let mut g = lock(&s.shared.gate);
        g.above = Some(p.target());
        g.above_cleared = false;
        if let Some(f) = g.fall.as_mut() {
            f.set_split(true);
        }
    }
}

/// Detaches the above panel from the loop and parks it for release.
fn detach_above(s: &mut Screen, now: f64) {
    {
        let mut g = lock(&s.shared.gate);
        g.above = None;
        if let Some(f) = g.fall.as_mut() {
            f.set_split(false);
        }
    }
    if let Some(p) = s.above.take() {
        s.leaving.push((now + plan::HANDOVER, p));
    }
}

impl<B: OverlayBackend> State<B> {
    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// Applies the stored settings to a fresh simulation.
    fn configure_fall(cfg: &Config, f: &mut dyn Falling) {
        f.set_share(cfg.share);
        f.set_wind(cfg.wind);
        f.set_lightning(cfg.lightning);
        f.keep_clear(cfg.keep_clear);
        f.show_ground(true);
    }

    /// Builds a simulation per screen from the stored factory.
    fn install_effect(&mut self) {
        for i in 0..self.screens.len() {
            let fall = self.cfg.factory.as_ref().map(|make| {
                let mut f = make(&self.screens[i].info);
                Self::configure_fall(&self.cfg, f.as_mut());
                f
            });
            {
                let mut g = lock(&self.screens[i].shared.gate);
                g.fall = fall;
                g.main_dirty = true;
                g.above_cleared = false;
                let split = g.above.is_some();
                if let Some(f) = g.fall.as_mut() {
                    f.set_split(split);
                }
            }
            self.layout(i, true);
        }
    }

    /// `Sit(behind)` of desktop-native 3.3 for one screen. `immediate` skips
    /// the 0.1 s hand-over delays (used when a simulation is created).
    fn layout(&mut self, i: usize, immediate: bool) {
        let now = self.now();
        let behind = self.cfg.behind;
        let Self { backend, present, instance, gpu, gate, screens, .. } = self;
        let s = &mut screens[i];
        let turn = s.stager.begin();
        let draws_above = s.shared.with_fall(|f| f.draws_above()).unwrap_or(false);
        let mut ready = false;
        let mut raise = false;
        if behind && draws_above {
            ready = s.above.is_some() || create_above(backend, *present, instance, gpu, gate, s).is_ok();
        }
        match plan::sit_plan(behind, draws_above, ready) {
            SitPlan::Staged if immediate => {
                attach_above(s);
                s.behind = true;
                keep_on_top(s);
                raise = plan::needs_raise(s.shown, s.behind, s.above.is_some());
            }
            SitPlan::Staged => {
                s.stager.at(now, plan::HANDOVER, turn, Act::AttachAbove);
                s.stager.at(now, 2.0 * plan::HANDOVER, turn, Act::PlaceBehind);
            }
            SitPlan::Immediate => {
                s.behind = behind;
                detach_above(s, now);
                keep_on_top(s);
                raise = plan::needs_raise(s.shown, s.behind, s.above.is_some());
            }
        }
        if raise {
            self.raise_panels();
        }
    }

    fn set_shown(&mut self, i: usize, shown: bool) {
        let s = &mut self.screens[i];
        if s.shown == shown {
            return;
        }
        s.shown = shown;
        s.main.show(shown);
        if let Some(a) = &s.above {
            a.show(shown);
        }
        s.shared.set_run(shown);
        if shown {
            keep_on_top(s);
            if plan::needs_raise(s.shown, s.behind, s.above.is_some()) {
                self.raise_panels();
            }
        }
    }

    /// `RaisePanels` (3.4): re-apply TOPMOST to the app's own topmost windows
    /// (cards, tray pointer, decorations) so they stay above the overlay.
    fn raise_panels(&self) {
        let panels: Vec<isize> = self
            .screens
            .iter()
            .flat_map(|s| std::iter::once(s.main.id()).chain(s.above.iter().map(|a| a.id())))
            .collect();
        self.backend.raise_own(&panels, &self.under_dock);
    }

    fn each_fall(&self, f: impl Fn(&mut dyn Falling)) {
        for s in &self.screens {
            s.shared.with_fall(&f);
        }
    }

    fn update(&mut self, snaps: &[ScreenSnap], interval: f64, to_other_space: bool) {
        let frames: Vec<Frame> = snaps.iter().map(|s| s.frame).collect();
        for i in 0..self.screens.len() {
            match plan::match_frame(&frames, &self.screens[i].info.frame) {
                Some(k) => {
                    let snap = &snaps[k];
                    self.set_shown(i, snap.shown);
                    self.screens[i].shared.with_fall(|f| {
                        f.show_ground(snap.ground);
                        f.update(&snap.surfaces, interval, to_other_space);
                    });
                }
                None => self.set_shown(i, false),
            }
        }
    }

    /// Runs due staged steps and releases parked panels.
    fn tick(&mut self) {
        let now = self.now();
        let gate = self.gate.clone();
        let mut raise = false;
        for s in &mut self.screens {
            for act in s.stager.due(now) {
                match act {
                    Act::AttachAbove => attach_above(s),
                    Act::PlaceBehind => {
                        s.behind = true;
                        keep_on_top(s);
                        raise |= plan::needs_raise(s.shown, s.behind, s.above.is_some());
                    }
                }
            }
            if s.leaving.iter().any(|(due, _)| *due <= now) {
                let _w = gate.write().unwrap_or_else(PoisonError::into_inner);
                let (due, keep): (Vec<_>, Vec<_>) =
                    std::mem::take(&mut s.leaving).into_iter().partition(|(d, _)| *d <= now);
                s.leaving = keep;
                for (_, p) in due {
                    p.destroy();
                }
            }
        }
        if raise {
            self.raise_panels();
        }
    }

    /// Stops every thread and destroys every window (UI thread).
    fn shutdown(&mut self) {
        for s in &self.screens {
            s.shared.stop.store(true, Ordering::Relaxed);
            s.shared.wake.notify_all();
        }
        for s in &mut self.screens {
            if let Some(t) = s.thread.take() {
                let _ = t.join();
            }
        }
        let gate = self.gate.clone();
        let _w = gate.write().unwrap_or_else(PoisonError::into_inner);
        for s in self.screens.drain(..) {
            let Screen { main, above, leaving, shared, .. } = s;
            drop(shared);
            for (_, p) in leaving {
                p.destroy();
            }
            if let Some(a) = above {
                a.destroy();
            }
            main.destroy();
        }
    }
}

/// All overlay panels. Create, use and drop on the UI thread.
pub(crate) struct Host<B: OverlayBackend> {
    state: Arc<Mutex<State<B>>>,
    /// The backend's UI-thread timer; `None` when the app ticks by hand.
    ticker: Option<Guard>,
    _ui: PhantomData<*const ()>,
}

impl<B: OverlayBackend> Host<B> {
    pub(crate) fn new(backend: B, monitors: &[MonitorInfo], cfg: Config) -> Result<Self, String> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = B::BACKENDS;
        let instance = wgpu::Instance::new(desc);

        let mut pending: Vec<(PanelInfo, PixelRect, PanelWin, (u32, u32))> = Vec::new();
        for (m, info) in monitors.iter().zip(plan::panel_infos(monitors)) {
            let swap = backend.swap_size(m.pixels, plan::swap_size(info.width, info.height));
            match backend.create_panel(&instance, m.pixels, swap, info.scale) {
                Ok(p) => pending.push((info, m.pixels, p, swap)),
                Err(e) => log::warn!("overlay: monitor {} skipped: {e}", m.id),
            }
        }
        let first = pending.first().ok_or("no overlay panel could be created")?;
        let first_target = first.2.target();
        let device = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&first_target.surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no adapter: {e}"))
        .and_then(|adapter| {
            let caps = first_target.surface.get_capabilities(&adapter);
            let present = Present { alpha: backend.alpha_mode(&caps), mode: backend.present_mode(&caps) };
            pollster::block_on(adapter.request_device(&crate::gpu::device_descriptor(&adapter)))
                .map(|(d, q)| (d, q, present))
                .map_err(|e| format!("no device: {e}"))
        });
        drop(first_target);
        let (device, queue, present) = match device {
            Ok(d) => d,
            Err(e) => {
                // No GPU: release the windows made so far (3.1 edge case).
                for (_, _, p, _) in pending {
                    p.destroy();
                }
                return Err(e);
            }
        };
        let gpu = Arc::new(Gpu::new(device, queue));
        let gate = Arc::new(RwLock::new(()));

        // Configure every surface before any render thread starts: configure
        // waits for the shared device to go idle, which fails while another
        // panel's thread keeps submitting (found while scaffolding).
        for (_, _, p, swap) in &pending {
            p.target().surface.configure(&gpu.device, &surface_config(*swap, present));
            p.commit();
        }
        let mut screens = Vec::new();
        for (info, pixels, main, swap) in pending {
            let dip = plan::swap_size(info.width, info.height);
            let px_per_pt = if swap == dip { 1.0 } else { swap.0 as f32 / info.width.max(1.0) as f32 };
            let shared = Arc::new(Loop {
                main: main.target(),
                gate: Mutex::new(Gate { fall: None, above: None, above_cleared: true, main_dirty: true }),
                stop: AtomicBool::new(false),
                run: Mutex::new(true),
                wake: Condvar::new(),
                frames: AtomicU64::new(0),
                size: (info.width as f32, info.height as f32),
                swap,
                px_per_pt,
                pixels,
                scale: info.scale,
                present,
            });
            let thread = spawn_loop::<B>(shared.clone(), gpu.clone(), gate.clone());
            main.show(true);
            screens.push(Screen {
                info,
                pixels,
                main,
                above: None,
                leaving: Vec::new(),
                shared,
                thread: Some(thread),
                shown: true,
                behind: false,
                stager: Stager::new(),
            });
        }
        let mut state = State {
            backend,
            present,
            gpu,
            instance,
            gate,
            cfg,
            screens,
            epoch: Instant::now(),
            under_dock: Vec::new(),
        };
        state.install_effect();
        let state = Arc::new(Mutex::new(state));
        let weak = Arc::downgrade(&state);
        let ticker = lock(&state).backend.start_ticker(
            50,
            Box::new(move || {
                // Skip a tick instead of re-entering a call that pumps messages.
                if let Some(h) = weak.upgrade()
                    && let Ok(mut st) = h.try_lock()
                {
                    st.tick();
                }
            }),
        );
        Ok(Host { state, ticker, _ui: PhantomData })
    }

    fn st(&self) -> MutexGuard<'_, State<B>> {
        lock(&self.state)
    }

    pub(crate) fn config(&self) -> Config {
        self.st().cfg.clone()
    }

    pub(crate) fn panels(&self) -> usize {
        self.st().screens.len()
    }

    pub(crate) fn infos(&self) -> Vec<PanelInfo> {
        self.st().screens.iter().map(|s| s.info.clone()).collect()
    }

    pub(crate) fn frames(&self) -> u64 {
        self.st().screens.iter().map(|s| s.shared.frames.load(Ordering::Relaxed)).sum()
    }

    pub(crate) fn set_effect(&self, factory: Option<crate::EffectFactory>) {
        let mut st = self.st();
        st.cfg.factory = factory;
        st.install_effect();
    }

    pub(crate) fn set_share(&self, share: f64) {
        let mut st = self.st();
        st.cfg.share = share;
        st.each_fall(|f| f.set_share(share));
    }

    pub(crate) fn set_wind(&self, wind: Wind) {
        let mut st = self.st();
        st.cfg.wind = wind;
        st.each_fall(|f| f.set_wind(wind));
    }

    pub(crate) fn set_lightning(&self, on: bool) {
        let mut st = self.st();
        st.cfg.lightning = on;
        st.each_fall(|f| f.set_lightning(on));
    }

    pub(crate) fn set_keep_clear(&self, on: bool) {
        let mut st = self.st();
        st.cfg.keep_clear = on;
        st.each_fall(|f| f.keep_clear(on));
    }

    pub(crate) fn set_behind_windows(&self, on: bool) {
        let mut st = self.st();
        if st.cfg.behind == on {
            return;
        }
        st.cfg.behind = on;
        for i in 0..st.screens.len() {
            st.layout(i, false);
        }
    }

    pub(crate) fn update(&self, snaps: &[ScreenSnap], interval: f64, to_other_space: bool) {
        self.st().update(snaps, interval, to_other_space);
    }

    pub(crate) fn keep_on_top(&self) {
        let st = self.st();
        for s in &st.screens {
            keep_on_top(s);
        }
        if st.screens.iter().any(|s| plan::needs_raise(s.shown, s.behind, s.above.is_some())) {
            st.raise_panels();
        }
    }

    pub(crate) fn set_under_dock(&self, ids: Vec<isize>) {
        self.st().under_dock = ids;
    }

    pub(crate) fn tick(&self) {
        self.st().tick();
    }

    /// No backend timer: the owner must call [`Host::tick`].
    pub(crate) fn needs_tick(&self) -> bool {
        self.ticker.is_none()
    }
}

impl<B: OverlayBackend> Drop for Host<B> {
    fn drop(&mut self) {
        // Stop the timer first, then tear down on this same thread.
        self.ticker = None;
        self.st().shutdown();
    }
}
