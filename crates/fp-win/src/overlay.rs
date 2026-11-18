//! Win32 overlay panels for `fp-overlay`'s host: click-through windows with a
//! DirectComposition visual each (desktop-native 3.1, 3.2), their z-order
//! (3.4), `RaisePanels`, the render-thread cursor and priority, and the
//! UI-thread timer that drives the host's staged steps.
//!
//! Moved unchanged from `fp-overlay/src/win.rs`; only the seams
//! (`fp_platform::overlay::{OverlayBackend, PanelWindow}`) are new.

use fp_core::PixelRect;
use fp_platform::overlay::{self as plan, OverlayBackend, PanelTarget, PanelWindow, Stack};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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

struct Dcomp {
    device: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
}

/// A click-through window with its composition tree and surface.
struct PanelWin {
    hwnd: HWND,
    dcomp: Dcomp,
    target: Arc<PanelTarget>,
    dpi_key: isize,
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
            Ok((dcomp, surface)) => {
                Ok(PanelWin { hwnd, dcomp, target: Arc::new(PanelTarget { surface }), dpi_key: key })
            }
            Err(e) => {
                lock_vec(&DPI_MAP).retain(|(k, _)| *k != key);
                let _ = DestroyWindow(hwnd);
                Err(e)
            }
        }
    }
}

impl PanelWindow for PanelWin {
    fn target(&self) -> Arc<PanelTarget> {
        self.target.clone()
    }

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

    /// `Front`: topmost. `Behind`: drop topmost, then sit right above the
    /// desktop shell (desktop-native 3.4).
    fn stack(&self, s: Stack) {
        match s {
            Stack::Front => set_pos(self.hwnd, HWND_TOPMOST),
            // SAFETY: plain queries on own and shell window handles.
            Stack::Behind => unsafe {
                let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
                if ex & WS_EX_TOPMOST.0 != 0 {
                    set_pos(self.hwnd, HWND_NOTOPMOST);
                }
                let shell = GetShellWindow();
                if let Ok(prev) = GetWindow(shell, GW_HWNDPREV)
                    && prev != self.hwnd
                {
                    set_pos(self.hwnd, prev);
                }
            },
        }
    }

    fn id(&self) -> isize {
        self.hwnd.0 as isize
    }

    /// Release order of desktop-native 3.2: composition objects and surface
    /// first (callers hold the render gate for writing), then the window.
    fn destroy(self: Box<Self>) {
        let PanelWin { hwnd, dcomp, target, dpi_key } = *self;
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

/// `RaisePanels` (3.4): re-apply TOPMOST to the app's own topmost windows
/// (cards, tray pointer, decorations) so they stay above the overlay.
fn raise_panels(panels: &[isize], under_dock: &[isize]) {
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
    for h in plan::raise_targets(&seen, panels, under_dock) {
        set_pos(HWND(h as *mut _), HWND_TOPMOST);
    }
}

// ---- render threads ------------------------------------------------------

fn cursor_now() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    // SAFETY: valid out pointer.
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x, p.y))
}

/// Render threads run above normal priority (desktop-native 3.8).
fn raise_priority() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};
    // SAFETY: pseudo handle of the calling thread.
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
}

// ---- timer ---------------------------------------------------------------

type Tick = Rc<RefCell<Box<dyn FnMut()>>>;

thread_local! {
    /// Host ticks of this (UI) thread by timer id.
    static LIVE: RefCell<Vec<(usize, Tick)>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn timer_proc(_: HWND, _: u32, id: usize, _: u32) {
    // A thread timer fires on the thread that set it: tick only that thread's hosts.
    let hosts: Vec<Tick> = LIVE.with(|l| l.borrow().iter().filter(|(t, _)| *t == id).map(|(_, f)| f.clone()).collect());
    for h in hosts {
        // Skip a tick instead of re-entering a call that pumps messages.
        if let Ok(mut tick) = h.try_borrow_mut() {
            tick();
        }
    }
}

/// A thread timer on the UI thread; killed on drop.
struct Ticker {
    timer: usize,
}

impl Drop for Ticker {
    fn drop(&mut self) {
        // SAFETY: kills the timer created by `start_ticker` on this same thread.
        unsafe {
            let _ = KillTimer(None, self.timer);
        }
        let me = self.timer;
        LIVE.with(|l| l.borrow_mut().retain(|(t, _)| *t != me));
    }
}

// ---- backend -------------------------------------------------------------

/// The Windows overlay backend: DX12 + DirectComposition panels.
#[derive(Clone, Copy, Debug)]
pub struct WinOverlay(());

impl WinOverlay {
    /// Registers the panel window class.
    pub fn new() -> Self {
        register();
        Self(())
    }
}

impl Default for WinOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayBackend for WinOverlay {
    const BACKENDS: wgpu::Backends = wgpu::Backends::DX12;

    /// DirectComposition presents premultiplied alpha (desktop-native 3.2).
    fn alpha_mode(&self, _: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode {
        wgpu::CompositeAlphaMode::PreMultiplied
    }

    fn create_panel(
        &self,
        instance: &wgpu::Instance,
        pixels: PixelRect,
        swap: (u32, u32),
        scale: f64,
    ) -> Result<Box<dyn PanelWindow>, String> {
        make_panel(instance, pixels, swap, scale).map(|p| Box::new(p) as Box<dyn PanelWindow>)
    }

    fn raise_own(&self, panels: &[isize], under_dock: &[isize]) {
        raise_panels(panels, under_dock);
    }

    fn take_rescaled(&self) -> bool {
        take_rescaled()
    }

    fn start_ticker(&self, every_ms: u32, tick: Box<dyn FnMut()>) -> Option<fp_platform::Guard> {
        // SAFETY: a thread timer on the UI thread; killed when the guard drops.
        let timer = unsafe { SetTimer(None, 0, every_ms, Some(timer_proc)) };
        LIVE.with(|l| l.borrow_mut().push((timer, Rc::new(RefCell::new(tick)))));
        Some(Box::new(Ticker { timer }))
    }

    fn cursor_px() -> Option<(i32, i32)> {
        cursor_now()
    }

    fn raise_priority() {
        raise_priority();
    }
}
