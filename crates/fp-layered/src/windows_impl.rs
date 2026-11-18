//! Win32 backend: one WS_POPUP layered window per sprite.
//!
//! Window recipe (snowballs 3.7, decorations 3.4): `WS_EX_LAYERED |
//! WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT`, premultiplied
//! BGRA DIB section, `UpdateLayeredWindow` with the destination point, z-order
//! with `SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_NOOWNERZORDER` (0x213).

use std::collections::HashMap;
use std::ffi::c_void;

use fp_core::geom::PixelRect;
use fp_paint::tiny_skia::Pixmap;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowExW, GA_ROOT, GW_HWNDNEXT,
    GW_HWNDPREV, GWL_EXSTYLE, GetAncestor, GetClassNameW, GetForegroundWindow, GetShellWindow, GetTopWindow, GetWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST, IDC_HAND,
    IsWindowVisible, LoadCursorW, MA_NOACTIVATE, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SET_WINDOW_POS_FLAGS,
    SW_HIDE, SW_SHOWNOACTIVATE, SetCursor, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, ULW_ALPHA,
    UpdateLayeredWindow, WINDOW_EX_STYLE, WM_MOUSEACTIVATE, WM_SETCURSOR, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WindowFromPoint,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::{Backend, Place, WinId};

/// NOSIZE | NOMOVE | NOACTIVATE | NOOWNERZORDER.
const Z_FLAGS: SET_WINDOW_POS_FLAGS = SET_WINDOW_POS_FLAGS(0x213);
const CLASS: PCWSTR = w!("FrostpaneObject");
const TITLE: PCWSTR = w!("Frostpane object window");
const WALK_LIMIT: usize = 4096;

fn hwnd(id: WinId) -> HWND {
    HWND(id.0 as *mut c_void)
}

fn id_of(h: HWND) -> WinId {
    WinId(h.0 as isize)
}

fn ex_style(h: HWND) -> u32 {
    // SAFETY: plain query on a window handle; an invalid handle returns 0.
    unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 }
}

fn is_topmost(h: HWND) -> bool {
    ex_style(h) & WS_EX_TOPMOST.0 != 0
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: buffer outlives the call.
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn is_taskbar_class(name: &str) -> bool {
    name == "Shell_TrayWnd" || name == "Shell_SecondaryTrayWnd"
}

fn window_pid(h: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: out pointer is valid for the call.
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn next_below(h: HWND) -> Option<HWND> {
    // SAFETY: plain z-order query.
    unsafe { GetWindow(h, GW_HWNDNEXT) }.ok().filter(|w| !w.0.is_null())
}

fn next_above(h: HWND) -> Option<HWND> {
    // SAFETY: plain z-order query.
    unsafe { GetWindow(h, GW_HWNDPREV) }.ok().filter(|w| !w.0.is_null())
}

fn top_window() -> Option<HWND> {
    // SAFETY: plain z-order query.
    unsafe { GetTopWindow(None) }.ok().filter(|w| !w.0.is_null())
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Clicks never steal focus.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            // SAFETY: stock cursor load and set, no ownership involved.
            unsafe {
                if let Ok(c) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(c));
                }
            }
            LRESULT(1)
        }
        // SAFETY: forwarding the unchanged message.
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

struct Dib {
    bmp: HBITMAP,
    bits: *mut u8,
    w: i32,
    h: i32,
}

struct Win {
    hwnd: HWND,
    dib: Option<Dib>,
    visible: bool,
}

/// Win32 implementation of [`Backend`].
#[derive(Default)]
pub struct WinBackend {
    dc: Option<HDC>,
    class_ready: bool,
    wins: HashMap<isize, Win>,
}

impl WinBackend {
    fn ensure_ready(&mut self) -> Option<HDC> {
        if !self.class_ready {
            // SAFETY: registers a window class with a static name and a wndproc
            // that only calls system functions.
            unsafe {
                let inst = GetModuleHandleW(None).ok()?;
                let wc = WNDCLASSEXW {
                    cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                    lpfnWndProc: Some(wndproc),
                    hInstance: inst.into(),
                    lpszClassName: CLASS,
                    ..Default::default()
                };
                // 0 also means "already registered" (e.g. a second host); the
                // create call below reveals a real failure.
                RegisterClassExW(&wc);
            }
            self.class_ready = true;
        }
        if self.dc.is_none() {
            // SAFETY: memory DC compatible with the screen, freed in Drop.
            let dc = unsafe { CreateCompatibleDC(None) };
            if dc.is_invalid() {
                return None;
            }
            self.dc = Some(dc);
        }
        self.dc
    }

    fn set_ex(&self, h: HWND, bit: u32, on: bool) {
        let cur = ex_style(h);
        let new = if on { cur | bit } else { cur & !bit };
        if new != cur {
            // SAFETY: style change on our own window.
            unsafe { SetWindowLongPtrW(h, GWL_EXSTYLE, new as _) };
        }
    }

    fn own(&self, h: HWND) -> bool {
        self.wins.contains_key(&(h.0 as isize))
    }

    fn desktop_host(&self) -> Option<HWND> {
        // SAFETY: z-order/shell queries only.
        unsafe {
            let shell = GetShellWindow();
            if shell.0.is_null() {
                return None;
            }
            if FindWindowExW(Some(shell), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok() {
                return Some(shell);
            }
            struct Find(Option<HWND>);
            unsafe extern "system" fn each(h: HWND, lp: LPARAM) -> BOOL {
                // SAFETY: `lp` is the `&mut Find` passed below.
                let f = unsafe { &mut *(lp.0 as *mut Find) };
                // SAFETY: child lookup on a top-level handle.
                if unsafe { FindWindowExW(Some(h), None, w!("SHELLDLL_DefView"), PCWSTR::null()) }.is_ok() {
                    f.0 = Some(h);
                    return BOOL(0);
                }
                BOOL(1)
            }
            let mut f = Find(None);
            // The callback stops the walk with FALSE, which EnumWindows reports as an error.
            let _ = EnumWindows(Some(each), LPARAM(&mut f as *mut Find as isize));
            Some(f.0.unwrap_or(shell))
        }
    }
}

impl Backend for WinBackend {
    fn create(&mut self) -> Option<WinId> {
        self.ensure_ready()?;
        // SAFETY: class registered above; no parent, no creation parameter.
        let h = unsafe {
            let inst = GetModuleHandleW(None).ok()?;
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0),
                CLASS,
                TITLE,
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(inst.into()),
                None,
            )
        }
        .ok()?;
        self.wins.insert(h.0 as isize, Win { hwnd: h, dib: None, visible: false });
        Some(id_of(h))
    }

    fn destroy(&mut self, id: WinId) {
        if let Some(w) = self.wins.remove(&id.0) {
            // SAFETY: our own window and bitmap, removed from the map first.
            unsafe {
                let _ = DestroyWindow(w.hwnd);
                if let Some(d) = w.dib {
                    let _ = DeleteObject(HGDIOBJ(d.bmp.0));
                }
            }
        }
    }

    fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool {
        let Some(dc) = self.dc else { return false };
        let Some(w) = self.wins.get_mut(&id.0) else { return false };
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        if w.dib.as_ref().is_none_or(|d| d.w != pw || d.h != ph) {
            if let Some(old) = w.dib.take() {
                // SAFETY: bitmap created by us, no longer selected into a DC.
                unsafe {
                    let _ = DeleteObject(HGDIOBJ(old.bmp.0));
                }
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: pw,
                    biHeight: -ph, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            // SAFETY: valid BITMAPINFO and out pointer.
            let bmp = unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) };
            match bmp {
                Ok(b) if !bits.is_null() => w.dib = Some(Dib { bmp: b, bits: bits as *mut u8, w: pw, h: ph }),
                _ => return false,
            }
        }
        let Some(d) = w.dib.as_ref() else { return false };
        let n = (pw as usize) * (ph as usize) * 4;
        // SAFETY: the DIB section holds exactly w*h*4 bytes at `bits`.
        let dst = unsafe { std::slice::from_raw_parts_mut(d.bits, n) };
        for (o, i) in dst.as_chunks_mut::<4>().0.iter_mut().zip(pm.pixels()) {
            // premultiplied RGBA -> premultiplied BGRA
            *o = [i.blue(), i.green(), i.red(), i.alpha()];
        }
        true
    }

    fn present(&mut self, id: WinId, x: i32, y: i32, alpha: u8) {
        let Some(dc) = self.dc else { return };
        let Some(w) = self.wins.get_mut(&id.0) else { return };
        let Some(d) = w.dib.as_ref() else { return };
        let pt = POINT { x, y };
        let size = SIZE { cx: d.w, cy: d.h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        // SAFETY: all pointers live for the call; the DIB is ours.
        unsafe {
            let old = SelectObject(dc, HGDIOBJ(d.bmp.0));
            let _ = UpdateLayeredWindow(
                w.hwnd,
                None,
                Some(&pt),
                Some(&size),
                Some(dc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(dc, old);
            if !w.visible {
                let _ = ShowWindow(w.hwnd, SW_SHOWNOACTIVATE);
                w.visible = true;
            }
        }
    }

    fn hide(&mut self, id: WinId) {
        if let Some(w) = self.wins.get_mut(&id.0) {
            // SAFETY: our own window.
            unsafe {
                let _ = ShowWindow(w.hwnd, SW_HIDE);
            }
            w.visible = false;
        }
    }

    fn set_click_through(&mut self, id: WinId, on: bool) {
        if let Some(w) = self.wins.get(&id.0) {
            self.set_ex(w.hwnd, WS_EX_TRANSPARENT.0, on);
        }
    }

    fn place(&mut self, id: WinId, p: Place) {
        let Some(w) = self.wins.get(&id.0) else { return };
        let after = match p {
            Place::Topmost => HWND_TOPMOST,
            Place::NotTopmost => HWND_NOTOPMOST,
            Place::Top => HWND_TOP,
            Place::After(a) => hwnd(a),
        };
        // SAFETY: z-order only (no size/move/activate flags).
        unsafe {
            let _ = SetWindowPos(w.hwnd, Some(after), 0, 0, 0, 0, Z_FLAGS);
        }
    }

    fn lowest_taskbar(&self) -> Option<WinId> {
        let mut cur = top_window();
        let mut found = None;
        for _ in 0..WALK_LIMIT {
            let Some(h) = cur else { break };
            if !is_topmost(h) {
                break;
            }
            if is_taskbar_class(&class_name(h)) {
                found = Some(id_of(h));
            }
            cur = next_below(h);
        }
        found
    }

    fn taskbar_above(&self, id: WinId) -> bool {
        let target = hwnd(id);
        let mut cur = top_window();
        for _ in 0..WALK_LIMIT {
            let Some(h) = cur else { return false };
            if h == target {
                return false;
            }
            if !is_topmost(h) {
                return false;
            }
            if is_taskbar_class(&class_name(h)) {
                return true;
            }
            cur = next_below(h);
        }
        false
    }

    fn covered(&self, id: WinId, cursor: (i32, i32)) -> bool {
        let me = hwnd(id);
        // SAFETY: point query and ancestor lookup.
        let root = unsafe {
            let under = WindowFromPoint(POINT { x: cursor.0, y: cursor.1 });
            if under.0.is_null() {
                return false;
            }
            GetAncestor(under, GA_ROOT)
        };
        if root.0.is_null() || root == me || self.own(root) {
            return false;
        }
        let mut cur = next_above(me);
        for _ in 0..WALK_LIMIT {
            let Some(h) = cur else { return false };
            if h == root {
                return true;
            }
            cur = next_above(h);
        }
        false
    }

    fn fullscreen(&self) -> Vec<PixelRect> {
        // SAFETY: foreground query and window rect read.
        unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null() {
                return Vec::new();
            }
            // SAFETY: plain query.
            if window_pid(fg) == GetCurrentProcessId() {
                return Vec::new();
            }
            let cls = class_name(fg);
            if matches!(cls.as_str(), "Progman" | "WorkerW") || is_taskbar_class(&cls) {
                return Vec::new();
            }
            let mut r = RECT::default();
            if GetWindowRect(fg, &mut r).is_err() {
                return Vec::new();
            }
            vec![PixelRect::new(r.left, r.top, r.right, r.bottom)]
        }
    }

    fn raise_front(&mut self, ids: &[WinId]) {
        let mut found: Vec<HWND> = Vec::new();
        let mut cur = top_window();
        for _ in 0..WALK_LIMIT {
            let Some(h) = cur else { break };
            if !is_topmost(h) {
                break;
            }
            // SAFETY: visibility query.
            if ids.contains(&id_of(h)) && unsafe { IsWindowVisible(h) }.as_bool() {
                found.push(h);
            }
            cur = next_below(h);
        }
        for h in found.into_iter().rev() {
            // SAFETY: z-order only.
            unsafe {
                let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, Z_FLAGS);
            }
        }
    }

    fn stack_desktop(&mut self, ids: &[WinId]) {
        let Some(host) = self.desktop_host() else { return };
        let cands: Vec<HWND> = ids.iter().map(|i| hwnd(*i)).collect();
        if cands.is_empty() {
            return;
        }
        // Run of our windows directly above the host, bottom to top.
        // SAFETY: process id query.
        let me = unsafe { GetCurrentProcessId() };
        let mut run: Vec<WinId> = Vec::new();
        let mut cur = next_above(host);
        for _ in 0..WALK_LIMIT {
            let Some(h) = cur else { break };
            // SAFETY: visibility query.
            if !unsafe { IsWindowVisible(h) }.as_bool() {
                cur = next_above(h);
                continue;
            }
            let ours = window_pid(h) == me;
            if !ours || is_topmost(h) {
                break;
            }
            if self.own(h) || ex_style(h) & WS_EX_TOOLWINDOW.0 != 0 {
                run.push(id_of(h));
            } else {
                break;
            }
            cur = next_above(h);
        }
        if crate::logic::stack_ok(&run, ids) {
            return;
        }
        for c in cands.iter().rev() {
            let above = next_above(host);
            if above == Some(*c) {
                continue;
            }
            let after = match above {
                Some(a) if !is_topmost(a) => a,
                _ => HWND_TOP,
            };
            // SAFETY: z-order only.
            unsafe {
                let _ = SetWindowPos(*c, Some(after), 0, 0, 0, 0, Z_FLAGS);
            }
        }
    }

    fn pump(&mut self) {
        let mut msg = MSG::default();
        // SAFETY: standard thread message dispatch.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl Drop for WinBackend {
    fn drop(&mut self) {
        let ids: Vec<isize> = self.wins.keys().copied().collect();
        for i in ids {
            self.destroy(WinId(i));
        }
        if let Some(dc) = self.dc.take() {
            // SAFETY: DC created by us.
            unsafe {
                let _ = DeleteDC(dc);
            }
        }
    }
}
