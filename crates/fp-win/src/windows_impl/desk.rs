//! Window enumeration, taskbar shelves and the desktop-native 3.10 helpers.

use crate::logic;
use fp_core::geom::PixelRect;
use fp_tracker::WindowInfo;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PWSTR, w};

fn id(h: HWND) -> i64 {
    h.0 as isize as i64
}

fn hwnd(i: i64) -> HWND {
    HWND(i as isize as *mut _)
}

fn class_of(h: HWND) -> String {
    let mut b = [0u16; 128];
    // SAFETY: valid buffer.
    let n = unsafe { GetClassNameW(h, &mut b) };
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}

fn pid_of(h: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: valid out pointer.
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

thread_local! {
    static NAMES: RefCell<HashMap<u32, String>> = RefCell::new(HashMap::new());
}

fn process_name(pid: u32) -> String {
    if let Some(n) = NAMES.with(|m| m.borrow().get(&pid).cloned()) {
        return n;
    }
    let mut name = String::new();
    // SAFETY: handle closed after use; buffer sized by `len`.
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                name = logic::file_name(&String::from_utf16_lossy(&buf[..len as usize]));
            }
            let _ = CloseHandle(h);
        }
    }
    NAMES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 512 {
            m.clear();
        }
        m.insert(pid, name.clone());
    });
    name
}

fn dword_attr(h: HWND, attr: DWMWINDOWATTRIBUTE) -> Option<u32> {
    let mut v = 0u32;
    // SAFETY: v is a valid 4-byte out buffer.
    unsafe { DwmGetWindowAttribute(h, attr, &mut v as *mut u32 as *mut _, 4) }.ok().map(|_| v)
}

fn frame_bounds(h: HWND) -> Option<PixelRect> {
    let mut r = RECT::default();
    // SAFETY: RECT is a valid out buffer of the declared size.
    unsafe {
        DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, size_of::<RECT>() as u32)
    }
    .ok()?;
    Some(PixelRect::new(r.left, r.top, r.right, r.bottom))
}

fn info(h: HWND, own_pid: u32) -> WindowInfo {
    // SAFETY: plain queries on a window handle; stale handles just fail.
    unsafe {
        let visible = IsWindowVisible(h).as_bool();
        let iconic = IsIconic(h).as_bool();
        let mut w = WindowInfo { hwnd: id(h), visible, iconic, ..Default::default() };
        if !visible || iconic {
            return w;
        }
        w.cloaked = dword_attr(h, DWMWA_CLOAKED).unwrap_or(0) != 0;
        w.style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        w.ex_style = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let pid = pid_of(h);
        w.own_process = pid == own_pid;
        w.class = class_of(h);
        if w.ex_style & WS_EX_LAYERED.0 != 0 {
            let (mut alpha, mut flags) = (0u8, LAYERED_WINDOW_ATTRIBUTES_FLAGS(0));
            if GetLayeredWindowAttributes(h, None, Some(&mut alpha), Some(&mut flags)).is_ok()
                && flags.0 & LWA_ALPHA.0 != 0
            {
                w.layered_alpha = Some(alpha);
            }
        }
        if !w.cloaked {
            w.bounds = frame_bounds(h);
            w.process_name = process_name(pid);
        }
        w
    }
}

unsafe extern "system" fn enum_cb(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the (Vec, pid) tuple passed by `windows`.
    let (out, pid) = unsafe { &mut *(lp.0 as *mut (Vec<WindowInfo>, u32)) };
    out.push(info(h, *pid));
    TRUE
}

/// Top-level windows in z-order with raw attributes (desktop-native 3.5.3).
pub fn windows() -> Vec<WindowInfo> {
    // SAFETY: the callback only touches the tuple during the call.
    unsafe {
        let mut st = (Vec::<WindowInfo>::new(), GetCurrentProcessId());
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut st as *mut _ as isize));
        st.0
    }
}

fn rect_of(h: HWND) -> Option<PixelRect> {
    let mut r = RECT::default();
    // SAFETY: valid out pointer.
    unsafe { GetWindowRect(h, &mut r) }.ok()?;
    Some(PixelRect::new(r.left, r.top, r.right, r.bottom))
}

/// Bottom, non-auto-hide taskbars (desktop-native 3.5.6).
pub fn taskbar_shelves() -> Vec<PixelRect> {
    let mut out = Vec::new();
    // SAFETY: APPBARDATA is initialised with its size; shell calls only read it.
    unsafe {
        let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, ..Default::default() };
        if SHAppBarMessage(ABM_GETSTATE, &mut abd) as u32 & logic::ABS_AUTOHIDE != 0 {
            return out;
        }
        let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, ..Default::default() };
        if SHAppBarMessage(ABM_GETTASKBARPOS, &mut abd) == 0 || abd.uEdge != logic::ABE_BOTTOM {
            return out;
        }
        let r = PixelRect::new(abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom);
        if logic::is_shelf(&r) {
            out.push(r);
        }
        let mut prev: Option<HWND> = None;
        while let Ok(h) = FindWindowExW(None, prev, w!("Shell_SecondaryTrayWnd"), None) {
            prev = Some(h);
            if IsWindowVisible(h).as_bool()
                && let Some(r) = rect_of(h)
                && logic::is_shelf(&r)
            {
                out.push(r);
            }
        }
    }
    out
}

/// Cursor or `(0, 0)` when unavailable (desktop-native 3.10 `Cursor`).
pub fn cursor_or_zero() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: valid out pointer.
    if unsafe { GetCursorPos(&mut p) }.is_ok() { (p.x, p.y) } else { (0, 0) }
}

/// Root window under a pixel, 0 when none (desktop-native 3.10 `RootAt`).
pub fn root_at(x: i32, y: i32) -> i64 {
    // SAFETY: plain queries.
    unsafe {
        let h = WindowFromPoint(POINT { x, y });
        if h.is_invalid() { 0 } else { id(GetAncestor(h, GA_ROOT)) }
    }
}

/// True when the window under the pixel belongs to this process (outside-click rule).
pub fn point_in_own_process(x: i32, y: i32) -> bool {
    // SAFETY: plain queries.
    unsafe {
        let h = WindowFromPoint(POINT { x, y });
        !h.is_invalid() && pid_of(h) == GetCurrentProcessId()
    }
}

/// `(0x8000 bit)` state of a virtual key, physical buttons (no swap handling).
pub fn key_down(vk: u16) -> bool {
    // SAFETY: plain query.
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) < 0 }
}

/// Monitors fully covered by the foreground window (desktop-native 3.10 `FullScreen`).
pub fn full_screens(monitors: &[fp_core::desktop::MonitorInfo]) -> Vec<PixelRect> {
    // SAFETY: plain queries.
    unsafe {
        let h = GetForegroundWindow();
        if h.is_invalid() {
            return Vec::new();
        }
        let Some(r) = rect_of(h) else { return Vec::new() };
        logic::fullscreen_monitors(&r, pid_of(h) == GetCurrentProcessId(), &class_of(h), monitors)
    }
}

unsafe extern "system" fn frames_cb(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the (Vec, pid) tuple passed by `window_frames`.
    let (out, pid) = unsafe { &mut *(lp.0 as *mut (Vec<PixelRect>, u32)) };
    if let Some(r) = logic::snap_frame(&info(h, *pid)) {
        out.push(r);
    }
    TRUE
}

/// Snap targets for decorations (desktop-native 3.10 `WindowFrames`).
pub fn window_frames() -> Vec<PixelRect> {
    // SAFETY: the callback only touches the tuple during the call.
    unsafe {
        let mut st = (Vec::<PixelRect>::new(), GetCurrentProcessId());
        let _ = EnumWindows(Some(frames_cb), LPARAM(&mut st as *mut _ as isize));
        st.0
    }
}

/// A topmost taskbar sits above `target` in z-order (desktop-native 3.10 `TaskbarAbove`).
pub fn taskbar_above(target: i64) -> bool {
    let mut chain = Vec::new();
    // SAFETY: plain z-order walk.
    unsafe {
        let mut cur = GetTopWindow(None).ok();
        while let Some(h) = cur {
            let topmost = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
            chain.push((id(h), class_of(h), topmost));
            if id(h) == target || chain.len() > 4096 {
                break;
            }
            cur = GetWindow(h, GW_HWNDNEXT).ok();
        }
    }
    logic::taskbar_above(&chain, target)
}

/// True when `root` is above `of` in z-order (used by hit-test cover checks).
pub fn is_above(root: i64, of: i64) -> bool {
    // SAFETY: plain z-order walk.
    unsafe {
        let mut cur = GetWindow(hwnd(of), GW_HWNDPREV).ok();
        let mut guard = 0;
        while let Some(h) = cur {
            if id(h) == root {
                return true;
            }
            guard += 1;
            if guard > 4096 {
                break;
            }
            cur = GetWindow(h, GW_HWNDPREV).ok();
        }
    }
    false
}

unsafe extern "system" fn workerw_cb(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: lp is the i64 slot passed by `desktop_host`.
    let slot = unsafe { &mut *(lp.0 as *mut i64) };
    // SAFETY: plain child lookup.
    if unsafe { FindWindowExW(Some(h), None, w!("SHELLDLL_DefView"), None) }.is_ok() {
        *slot = id(h);
        return FALSE;
    }
    TRUE
}

/// Window that hosts the desktop icons (desktop-native 3.10 `DesktopHost`).
pub fn desktop_host() -> i64 {
    // SAFETY: plain window lookups.
    unsafe {
        if let Ok(p) = FindWindowW(w!("Progman"), None)
            && FindWindowExW(Some(p), None, w!("SHELLDLL_DefView"), None).is_ok()
        {
            return id(p);
        }
        let mut slot = 0i64;
        let _ = EnumWindows(Some(workerw_cb), LPARAM(&mut slot as *mut _ as isize));
        if slot != 0 {
            return slot;
        }
        id(GetShellWindow())
    }
}

/// `SetWindowPos(hwnd, after, 0,0,0,0, 531)` (NOSIZE|NOMOVE|NOACTIVATE|NOOWNERZORDER).
pub fn order(target: i64, after: i64) {
    // SAFETY: plain call; failures ignored.
    unsafe {
        let _ = SetWindowPos(hwnd(target), Some(hwnd(after)), 0, 0, 0, 0, SET_WINDOW_POS_FLAGS(531));
    }
}

/// Extended-style TOPMOST bit.
pub fn is_topmost(target: i64) -> bool {
    // SAFETY: plain query.
    unsafe { GetWindowLongPtrW(hwnd(target), GWL_EXSTYLE) as u32 & logic::WS_EX_TOPMOST != 0 }
}
