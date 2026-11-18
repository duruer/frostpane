//! Windows implementation: panels (window + DirectComposition visual + wgpu
//! surface), one render thread per monitor, the above panel and the staged
//! behind-windows z-order (desktop-native 3.1-3.4, 3.8).

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
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, RwLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, IUnknown, Interface, PCWSTR, w};

const CLASS: PCWSTR = w!("FrostpaneOverlay");

/// SWP_NOSIZE | NOMOVE | NOACTIVATE | NOOWNERZORDER | NOSENDCHANGING = 1555
/// (desktop-native 3.4).
const WALK_LIMIT: usize = 4096;
const SWP: SET_WINDOW_POS_FLAGS =
    SET_WINDOW_POS_FLAGS(SWP_NOSIZE.0 | SWP_NOMOVE.0 | SWP_NOACTIVATE.0 | SWP_NOOWNERZORDER.0 | SWP_NOSENDCHANGING.0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---- window class, DPI map, rescale flag ---------------------------------

/// Creation DPI per panel handle, to recognise a real DPI change.
static DPI_MAP: Mutex<Vec<(isize, u32)>> = Mutex::new(Vec::new());
/// Raised by `WM_DPICHANGED` when a panel's DPI differs from its creation DPI.
static RESCALED: AtomicBool = AtomicBool::new(false);

/// Returns and clears the "a monitor's DPI changed, rebuild" flag.
pub(crate) fn take_rescaled() -> bool {
    RESCALED.swap(false, Ordering::SeqCst)
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // The suggested rect is ignored; a changed DPI asks for a full rebuild.
        WM_DPICHANGED => {
            let new = (wp.0 & 0xFFFF) as u32;
            let known = lock_vec(&DPI_MAP).iter().find(|(k, _)| *k == h.0 as isize).map(|(_, d)| *d);
            if known.is_some_and(|old| old != new) {
                RESCALED.store(true, Ordering::SeqCst);
            }
            LRESULT(0)
        }
        // SAFETY: forwarding unchanged arguments.
        _ => unsafe { DefWindowProcW(h, m, wp, lp) },
    }
}

fn lock_vec<T>(m: &'static Mutex<Vec<T>>) -> MutexGuard<'static, Vec<T>> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn register() {
    // SAFETY: registering a class with a static wndproc; re-registration fails harmlessly.
    unsafe {
        let hi: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hi, lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
    }
}

// ---- panels --------------------------------------------------------------

/// Swap chain surface of one panel. Size is the DIP-sized swap chain.
struct Target {
    surface: wgpu::Surface<'static>,
}

struct Dcomp {
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
}

/// A click-through window with its composition tree and surface.
struct PanelWin {
    hwnd: HWND,
    dcomp: Dcomp,
    target: Arc<Target>,
    dpi_key: isize,
}

// SAFETY: panels are created, shown, re-ordered and destroyed on the UI
// thread only (the host is !Send); the render threads see only `Target`.
unsafe impl Send for PanelWin {}

fn surface_config(size: (u32, u32)) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: crate::gpu::FORMAT,
        width: size.0,
        height: size.1,
        present_mode: wgpu::PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
        view_formats: vec![],
        color_space: Default::default(),
    }
}

/// Window + composition + surface for one monitor (desktop-native 3.1, 3.2).
/// The surface is not configured yet.
fn make_panel(instance: &wgpu::Instance, pixels: PixelRect, swap: (u32, u32), scale: f64) -> Result<PanelWin, String> {
    // SAFETY: Win32/DComp calls with valid arguments; failures are mapped to
    // errors and the window is destroyed on the error path.
    unsafe {
        let hi: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let ex = WS_EX_TOPMOST
            | WS_EX_TRANSPARENT
            | WS_EX_TOOLWINDOW
            | WS_EX_LAYERED
            | WS_EX_NOREDIRECTIONBITMAP
            | WS_EX_NOACTIVATE;
        let hwnd = CreateWindowExW(
            ex,
            CLASS,
            w!("Frostpane"),
            WS_POPUP,
            pixels.left,
            pixels.top,
            pixels.width(),
            pixels.height(),
            None,
            None,
            Some(hi),
            None,
        )
        .map_err(|e| e.to_string())?;
        let key = hwnd.0 as isize;
        lock_vec(&DPI_MAP).push((key, plan::creation_dpi(scale)));
        let built = (|| -> Result<(Dcomp, wgpu::Surface<'static>), String> {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA).map_err(|e| e.to_string())?;
            let off: BOOL = TRUE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_TRANSITIONS_FORCEDISABLED,
                &off as *const _ as *const _,
                size_of::<BOOL>() as u32,
            );
            let device: IDCompositionDevice =
                DCompositionCreateDevice2(None::<&IUnknown>).map_err(|e| e.to_string())?;
            let target = device.CreateTargetForHwnd(hwnd, true).map_err(|e| e.to_string())?;
            let visual = device.CreateVisual().map_err(|e| e.to_string())?;
            // Render at DIP size, stretch to the monitor's pixels (desktop-native 3.2).
            let (sx, sy) = plan::visual_scale(pixels, swap);
            let _ = visual.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR);
            // Row-major 3x2 matrix (M11 M12 M21 M22 M31 M32): a pure scale.
            let matrix: [f32; 6] = [sx, 0.0, 0.0, sy, 0.0, 0.0];
            visual.SetTransform2(matrix.as_ptr().cast()).map_err(|e| e.to_string())?;
            target.SetRoot(&visual).map_err(|e| e.to_string())?;
            device.Commit().map_err(|e| e.to_string())?;
            let surface = instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(visual.as_raw()))
                .map_err(|e| e.to_string())?;
            Ok((Dcomp { device, _target: target, _visual: visual }, surface))
        })();
        match built {
            Ok((dcomp, surface)) => Ok(PanelWin { hwnd, dcomp, target: Arc::new(Target { surface }), dpi_key: key }),
            Err(e) => {
                lock_vec(&DPI_MAP).retain(|(k, _)| *k != key);
                let _ = DestroyWindow(hwnd);
                Err(e)
            }
        }
    }
}

impl PanelWin {
    /// wgpu sets the visual content but never commits (spike finding).
    fn commit(&self) {
        // SAFETY: valid composition device owned by this panel.
        unsafe {
            let _ = self.dcomp.device.Commit();
        }
    }

    fn show(&self, on: bool) {
        // SAFETY: own window.
        unsafe {
            let _ = ShowWindow(self.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    /// Release order of desktop-native 3.2: composition objects and surface
    /// first (callers hold the render gate for writing), then the window.
    fn destroy(self) {
        let PanelWin { hwnd, dcomp, target, dpi_key } = self;
        drop(target);
        drop(dcomp);
        lock_vec(&DPI_MAP).retain(|(k, _)| *k != dpi_key);
        // SAFETY: own window, destroyed on the creating (UI) thread.
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

// ---- z-order (desktop-native 3.4) ----------------------------------------

fn set_pos(hwnd: HWND, after: HWND) {
    // SAFETY: own window handle, flags 1555.
    unsafe {
        let _ = SetWindowPos(hwnd, Some(after), 0, 0, 0, 0, SWP);
    }
}

/// `KeepOnTop` for one screen: topmost panels, or in behind mode the main
/// panel right above the desktop shell. No-op while the screen is hidden.
fn keep_on_top(s: &Screen) {
    if !s.shown {
        return;
    }
    if !s.behind {
        set_pos(s.main.hwnd, HWND_TOPMOST);
    }
    if let Some(a) = &s.above {
        set_pos(a.hwnd, HWND_TOPMOST);
    }
    if s.behind {
        // SAFETY: plain queries on own and shell window handles.
        unsafe {
            let ex = GetWindowLongPtrW(s.main.hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOPMOST.0 != 0 {
                set_pos(s.main.hwnd, HWND_NOTOPMOST);
            }
            let shell = GetShellWindow();
            if let Ok(prev) = GetWindow(shell, GW_HWNDPREV)
                && prev != s.main.hwnd
            {
                set_pos(s.main.hwnd, prev);
            }
        }
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
    pixels: PixelRect,
    scale: f64,
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
fn prepare(sh: &Loop, dt: f64, time: f64, main: &mut DrawList, above: &mut DrawList) -> Work {
    let cursor = cursor_now(sh);
    let (w, h) = sh.size;
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
    fill(main, w, h, time, |l| fall.draw(Layer::Main, l));
    *main_dirty = false;
    let above_job = above_panel.clone().and_then(|a| {
        let has = fall.has_above();
        if !has && *above_cleared {
            return None;
        }
        fill(above, w, h, time, |l| fall.draw(Layer::Above, l));
        *above_cleared = !has;
        Some((a, Job::Draw))
    });
    Work { main: Job::Draw, above: above_job }
}

/// Resets a list to the panel's scene and lets the simulation draw into it.
fn fill(list: &mut DrawList, w: f32, h: f32, time: f64, draw: impl FnOnce(&mut DrawList)) {
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
}

fn cursor_now(sh: &Loop) -> Option<fp_core::Vec2> {
    let mut p = POINT::default();
    // SAFETY: valid out pointer.
    unsafe { GetCursorPos(&mut p).ok()? };
    plan::cursor_local(sh.pixels, sh.scale, p.x, p.y)
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
            target.surface.configure(&gpu.device, &surface_config(swap));
            false
        }
        _ => false,
    }
}

/// Render threads run above normal priority (desktop-native 3.8).
fn raise_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};
    // SAFETY: pseudo handle of the calling thread.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
}

/// The frame loop of one monitor (desktop-native 3.8): 60 fps cap on top of
/// Fifo vsync, dt clamp, main panel then above panel.
fn render_loop(sh: Arc<Loop>, gpu: Arc<Gpu>, gate: Arc<RwLock<()>>) {
    raise_priority();
    let swap = sh.swap;
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
        let work = prepare(&sh, dt, time, &mut main_list, &mut above_list);
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
        if work.main != Job::Idle && present(&gpu, &gate, &mut slots, &sh.main, swap, list) {
            sh.frames.fetch_add(1, Ordering::Relaxed);
        }
        if let Some((target, job)) = work.above {
            // The above panel has the main panel's size (3.3).
            let list = (job == Job::Draw).then_some(&above_list);
            present(&gpu, &gate, &mut slots, &target, swap, list);
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

fn spawn_loop(sh: Arc<Loop>, gpu: Arc<Gpu>, gate: Arc<RwLock<()>>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("frostpane overlay".into())
        .spawn(move || {
            if std::panic::catch_unwind(AssertUnwindSafe(|| render_loop(sh, gpu, gate))).is_err() {
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

struct State {
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
fn create_above(instance: &wgpu::Instance, gpu: &Gpu, gate: &RwLock<()>, s: &mut Screen) -> Result<(), String> {
    let swap = plan::swap_size(s.info.width, s.info.height);
    let panel = make_panel(instance, s.pixels, swap, s.info.scale)?;
    {
        let _w = gate.write().unwrap_or_else(PoisonError::into_inner);
        panel.target.surface.configure(&gpu.device, &surface_config(swap));
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
        g.above = Some(p.target.clone());
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

impl State {
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
        let Self { instance, gpu, gate, screens, .. } = self;
        let s = &mut screens[i];
        let turn = s.stager.begin();
        let draws_above = s.shared.with_fall(|f| f.draws_above()).unwrap_or(false);
        let mut ready = false;
        let mut raise = false;
        if behind && draws_above {
            ready = s.above.is_some() || create_above(instance, gpu, gate, s).is_ok();
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
            .flat_map(|s| std::iter::once(s.main.hwnd).chain(s.above.iter().map(|a| a.hwnd)))
            .map(|h| h.0 as isize)
            .collect();
        let mut seen = Vec::new();
        // SAFETY: plain z-order queries on foreign and own handles.
        unsafe {
            let me = GetCurrentThreadId();
            let mut cur = GetTopWindow(None).ok().filter(|h| !h.0.is_null());
            for _ in 0..WALK_LIMIT {
                let Some(h) = cur else { break };
                if GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0 {
                    break;
                }
                seen.push(plan::Seen {
                    hwnd: h.0 as isize,
                    topmost: true,
                    visible: IsWindowVisible(h).as_bool(),
                    own_thread: GetWindowThreadProcessId(h, None) == me,
                });
                cur = GetWindow(h, GW_HWNDNEXT).ok().filter(|h| !h.0.is_null());
            }
        }
        for h in plan::raise_targets(&seen, &panels, &self.under_dock) {
            set_pos(HWND(h as *mut _), HWND_TOPMOST);
        }
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

// ---- timer ---------------------------------------------------------------

/// A host and the UI thread that owns it.
struct Live(Weak<Mutex<State>>, std::thread::ThreadId);

// SAFETY: the weak handle is upgraded and used only from its owning UI thread
// (`timer_proc` filters by thread id; `Host` is !Send).
unsafe impl Send for Live {}

static LIVE: Mutex<Vec<Live>> = Mutex::new(Vec::new());

unsafe extern "system" fn timer_proc(_: HWND, _: u32, _: usize, _: u32) {
    // A thread timer fires on the thread that set it: tick only that thread's hosts.
    let me = std::thread::current().id();
    let hosts: Vec<Arc<Mutex<State>>> =
        lock(&LIVE).iter().filter(|l| l.1 == me).filter_map(|l| l.0.upgrade()).collect();
    for h in hosts {
        // Skip a tick instead of re-entering a call that pumps messages.
        if let Ok(mut st) = h.try_lock() {
            st.tick();
        }
    }
}

/// All overlay panels. Create, use and drop on the UI thread.
pub(crate) struct Host {
    state: Arc<Mutex<State>>,
    timer: usize,
    _ui: PhantomData<*const ()>,
}

impl Host {
    pub(crate) fn new(monitors: &[MonitorInfo], cfg: Config) -> Result<Self, String> {
        register();
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(desc);

        let mut pending: Vec<(PanelInfo, PixelRect, PanelWin, (u32, u32))> = Vec::new();
        for (m, info) in monitors.iter().zip(plan::panel_infos(monitors)) {
            let swap = plan::swap_size(info.width, info.height);
            match make_panel(&instance, m.pixels, swap, info.scale) {
                Ok(p) => pending.push((info, m.pixels, p, swap)),
                Err(e) => log::warn!("overlay: monitor {} skipped: {e}", m.id),
            }
        }
        let first = pending.first().ok_or("no overlay panel could be created")?;
        let device = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&first.2.target.surface),
            ..Default::default()
        }))
        .map_err(|e| format!("no adapter: {e}"))
        .and_then(|adapter| {
            pollster::block_on(adapter.request_device(&crate::gpu::device_descriptor()))
                .map_err(|e| format!("no device: {e}"))
        });
        let (device, queue) = match device {
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
            p.target.surface.configure(&gpu.device, &surface_config(*swap));
            p.commit();
        }
        let mut screens = Vec::new();
        for (info, pixels, main, swap) in pending {
            let shared = Arc::new(Loop {
                main: main.target.clone(),
                gate: Mutex::new(Gate { fall: None, above: None, above_cleared: true, main_dirty: true }),
                stop: AtomicBool::new(false),
                run: Mutex::new(true),
                wake: Condvar::new(),
                frames: AtomicU64::new(0),
                size: (info.width as f32, info.height as f32),
                swap,
                pixels,
                scale: info.scale,
            });
            let thread = spawn_loop(shared.clone(), gpu.clone(), gate.clone());
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
        let mut state = State { gpu, instance, gate, cfg, screens, epoch: Instant::now(), under_dock: Vec::new() };
        state.install_effect();
        let state = Arc::new(Mutex::new(state));
        lock(&LIVE).push(Live(Arc::downgrade(&state), std::thread::current().id()));
        // SAFETY: a thread timer on the UI thread; killed in `drop`.
        let timer = unsafe { SetTimer(None, 0, 50, Some(timer_proc)) };
        Ok(Host { state, timer, _ui: PhantomData })
    }

    fn st(&self) -> MutexGuard<'_, State> {
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
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: kills the timer created in `new` on this same thread.
        unsafe {
            let _ = KillTimer(None, self.timer);
        }
        let me = Arc::as_ptr(&self.state);
        lock(&LIVE).retain(|l| l.0.as_ptr() != me && l.0.strong_count() > 0);
        self.st().shutdown();
    }
}
