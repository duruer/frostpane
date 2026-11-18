//! Hidden windows and hooks that live on the UI thread: the driver timer
//! (architecture section 4), the power/session window (desktop-native 3.7) and the
//! WinEvent hooks (desktop-native 3.5.1).

use crate::logic::{self, SleepState, SystemEvent};
use fp_tracker::HookEvent;
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::*;
use windows::Win32::System::RemoteDesktop::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{GUID, PCWSTR, w};

/// GUID_CONSOLE_DISPLAY_STATE (desktop-native 3.7).
const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID::from_u128(0x6FE69556_704A_47A0_8F24_C28D936FDA47);
/// WM_POWERBROADCAST wParam carrying a POWERBROADCAST_SETTING.
const PBT_POWERSETTINGCHANGE: usize = 0x8013;

type Handler = Box<dyn FnMut(u32, WPARAM, LPARAM)>;

/// A hidden window whose messages go to a Rust closure (UI thread only).
struct HostWindow {
    hwnd: HWND,
    state: *mut RefCell<Handler>,
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: GWLP_USERDATA is either 0 or the RefCell leaked by `HostWindow::create`,
    // which outlives the window (freed after DestroyWindow).
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut RefCell<Handler>;
    if !ptr.is_null() {
        // SAFETY: see above; a re-entrant call finds the cell borrowed and is skipped.
        if let Ok(mut h) = unsafe { &*ptr }.try_borrow_mut() {
            (h)(msg, wp, lp);
        }
    }
    // SAFETY: default handling of the same message.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

impl HostWindow {
    fn create(class: PCWSTR, message_only: bool, handler: Handler) -> Option<HostWindow> {
        // SAFETY: standard class registration and window creation on this thread.
        unsafe {
            let hinst = GetModuleHandleW(None).ok()?;
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(host_proc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            // Already registered (second host of the same class) is fine.
            RegisterClassExW(&wc);
            let parent = if message_only { Some(HWND_MESSAGE) } else { None };
            let style = if message_only { WINDOW_STYLE(0) } else { WS_POPUP };
            let hwnd = CreateWindowExW(
                if message_only { WINDOW_EX_STYLE(0) } else { WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE },
                class,
                w!("Frostpane host"),
                style,
                0,
                0,
                0,
                0,
                parent,
                None,
                Some(hinst.into()),
                None,
            )
            .ok()?;
            let state = Box::into_raw(Box::new(RefCell::new(handler)));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as _);
            Some(HostWindow { hwnd, state })
        }
    }
}

impl Drop for HostWindow {
    fn drop(&mut self) {
        // SAFETY: we created both; the window is destroyed before the state is freed.
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.state));
        }
    }
}

const TIMER_ID: usize = 1;

/// Hidden message-only window with a repeating `SetTimer`; `on_tick` runs on the
/// thread that created the driver (its message loop must be pumped).
pub struct Driver {
    host: HostWindow,
}

impl Driver {
    /// `interval_ms` of 10 gives about 64 Hz in practice (USER timer granularity).
    pub fn new(interval_ms: u32, mut on_tick: impl FnMut() + 'static) -> Option<Driver> {
        let host = HostWindow::create(
            w!("FrostpaneDriver"),
            true,
            Box::new(move |msg, wp, _| {
                if msg == WM_TIMER && wp.0 == TIMER_ID {
                    on_tick();
                }
            }),
        )?;
        // SAFETY: valid window owned by us.
        if unsafe { SetTimer(Some(host.hwnd), TIMER_ID, interval_ms, None) } == 0 {
            return None;
        }
        Some(Driver { host })
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // SAFETY: the timer belongs to our window.
        unsafe {
            let _ = KillTimer(Some(self.host.hwnd), TIMER_ID);
        }
    }
}

/// Session lock, suspend/resume, display-off, display-change and theme
/// notifications. Callback gets the event and, when the combined "asleep"
/// state changed, the new value.
pub struct SystemWatcher {
    host: HostWindow,
    power: Option<HPOWERNOTIFY>,
    wts: bool,
}

impl SystemWatcher {
    pub fn new(mut on_event: impl FnMut(SystemEvent, Option<bool>) + 'static) -> Option<SystemWatcher> {
        let mut state = SleepState::default();
        let host = HostWindow::create(
            w!("FrostpanePower"),
            false,
            Box::new(move |msg, wp, lp| {
                let ev = match msg {
                    WM_WTSSESSION_CHANGE => logic::session_event(wp.0),
                    WM_POWERBROADCAST if wp.0 == PBT_POWERSETTINGCHANGE => {
                        // SAFETY: lParam points at POWERBROADCAST_SETTING for this notification.
                        let s = unsafe { (lp.0 as *const POWERBROADCAST_SETTING).as_ref() };
                        s.filter(|s| s.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && s.DataLength >= 4).map(|s| {
                            // SAFETY: DataLength >= 4 bytes follow the header.
                            let d = unsafe { std::ptr::read_unaligned(s.Data.as_ptr() as *const u32) };
                            logic::display_state_event(d)
                        })
                    }
                    WM_POWERBROADCAST => logic::power_mode_event(wp.0),
                    WM_DISPLAYCHANGE => Some(SystemEvent::DisplaysChanged),
                    WM_SETTINGCHANGE => {
                        // SAFETY: lParam is NUL-terminated text or 0 for this message.
                        let theme = lp.0 != 0
                            && unsafe { PCWSTR(lp.0 as *const u16).to_string() }
                                .is_ok_and(|s| s == "ImmersiveColorSet");
                        theme.then_some(SystemEvent::ThemeChanged)
                    }
                    _ => None,
                };
                if let Some(ev) = ev {
                    let changed = state.apply(ev);
                    on_event(ev, changed);
                }
            }),
        )?;
        // SAFETY: valid window handle owned by us.
        let wts = unsafe { WTSRegisterSessionNotification(host.hwnd, NOTIFY_FOR_THIS_SESSION) }.is_ok();
        // SAFETY: the window handle doubles as the recipient handle.
        let power = unsafe {
            RegisterPowerSettingNotification(
                HANDLE(host.hwnd.0),
                &GUID_CONSOLE_DISPLAY_STATE,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        }
        .ok();
        Some(SystemWatcher { host, power, wts })
    }
}

impl Drop for SystemWatcher {
    fn drop(&mut self) {
        // SAFETY: both registrations were made against our window.
        unsafe {
            if let Some(p) = self.power.take() {
                let _ = UnregisterPowerSettingNotification(p);
            }
            if self.wts {
                let _ = WTSUnRegisterSessionNotification(self.host.hwnd);
            }
        }
    }
}

static HOOK_TX: Mutex<Option<Sender<HookEvent>>> = Mutex::new(None);

unsafe extern "system" fn hook_proc(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_obj: i32,
    id_child: i32,
    _: u32,
    _: u32,
) {
    if let Some(ev) = logic::map_hook(event, hwnd.0 as isize as i64, id_obj, id_child)
        && let Ok(g) = HOOK_TX.lock()
        && let Some(tx) = g.as_ref()
    {
        let _ = tx.send(ev);
    }
}

/// Installed WinEvent hooks; removed on drop. Callbacks arrive on the installing
/// thread's message loop and are forwarded to `tx`.
pub struct HookSet {
    hooks: Vec<HWINEVENTHOOK>,
}

impl HookSet {
    /// Hooks that fail to install are silently omitted (see [`HookSet::count`]).
    pub fn install(tx: Sender<HookEvent>) -> HookSet {
        if let Ok(mut g) = HOOK_TX.lock() {
            *g = Some(tx);
        }
        let mut hooks = Vec::new();
        for (lo, hi) in [(3u32, 3u32), (10, 11), (22, 22), (32769, 32771)] {
            // SAFETY: out-of-context hook with a static callback.
            let h = unsafe { SetWinEventHook(lo, hi, None, Some(hook_proc), 0, 0, WINEVENT_OUTOFCONTEXT) };
            if !h.is_invalid() {
                hooks.push(h);
            }
        }
        HookSet { hooks }
    }

    pub fn count(&self) -> usize {
        self.hooks.len()
    }
}

impl Drop for HookSet {
    fn drop(&mut self) {
        for h in self.hooks.drain(..) {
            // SAFETY: hooks were installed by us.
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
        if let Ok(mut g) = HOOK_TX.lock() {
            *g = None;
        }
    }
}

/// Waits up to `timeout_ms` for messages and dispatches what arrived. Returns
/// `false` after `WM_QUIT`. For headless loops (smoke binary, tests).
pub fn pump_messages(timeout_ms: u32) -> bool {
    // SAFETY: standard message pump on the calling thread.
    unsafe {
        let _ =
            windows::Win32::UI::WindowsAndMessaging::MsgWaitForMultipleObjects(None, false, timeout_ms, QS_ALLINPUT);
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return false;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    #[test]
    fn system_watcher_maps_messages_and_tracks_sleep() {
        type Seen = Vec<(SystemEvent, Option<bool>)>;
        let seen: Rc<RefCell<Seen>> = Rc::default();
        let s2 = seen.clone();
        let w = SystemWatcher::new(move |e, a| s2.borrow_mut().push((e, a))).expect("watcher");
        let h = w.host.hwnd;
        // Drop anything Windows sent on registration (initial display state).
        pump_messages(10);
        seen.borrow_mut().clear();
        let send = |msg: u32, wp: usize, lp: isize| {
            // SAFETY: same-thread send to our own window; lParam payloads outlive the call.
            unsafe { SendMessageW(h, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) };
        };
        send(WM_WTSSESSION_CHANGE, logic::WTS_SESSION_LOCK, 0);
        send(WM_POWERBROADCAST, logic::PBT_APMSUSPEND, 0);
        send(WM_POWERBROADCAST, logic::PBT_APMPOWERSTATUSCHANGE, 0);
        send(WM_WTSSESSION_CHANGE, logic::WTS_SESSION_UNLOCK, 0);
        send(WM_POWERBROADCAST, logic::PBT_APMRESUMEAUTOMATIC, 0);
        // POWERBROADCAST_SETTING header followed by its full 4-byte payload.
        // `POWERBROADCAST_SETTING::Data` is declared as `[u8; 1]`, so the
        // payload needs its own storage or the handler reads past the struct.
        #[repr(C)]
        struct Setting {
            guid: GUID,
            len: u32,
            data: u32,
        }
        // Display off through a real POWERBROADCAST_SETTING layout.
        let mut setting = Setting { guid: GUID_CONSOLE_DISPLAY_STATE, len: 4, data: 0 };
        send(WM_POWERBROADCAST, PBT_POWERSETTINGCHANGE, &mut setting as *mut _ as isize);
        // Another GUID is ignored.
        let mut other = Setting { guid: GUID::zeroed(), len: 4, data: 1 };
        send(WM_POWERBROADCAST, PBT_POWERSETTINGCHANGE, &mut other as *mut _ as isize);
        setting.data = 2;
        send(WM_POWERBROADCAST, PBT_POWERSETTINGCHANGE, &mut setting as *mut _ as isize);
        let theme: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
        send(WM_SETTINGCHANGE, 0, theme.as_ptr() as isize);
        let other_setting: Vec<u16> = "Policy\0".encode_utf16().collect();
        send(WM_SETTINGCHANGE, 0, other_setting.as_ptr() as isize);
        send(WM_SETTINGCHANGE, 0, 0);
        send(WM_DISPLAYCHANGE, 32, 0);
        let got: Vec<_> = seen.borrow().iter().copied().filter(|(e, _)| *e != SystemEvent::PowerChanged).collect();
        assert_eq!(
            got,
            vec![
                (SystemEvent::Locked, Some(true)),
                (SystemEvent::Suspend, None),
                (SystemEvent::Unlocked, None),
                (SystemEvent::Resume, Some(false)),
                (SystemEvent::DisplayOff, Some(true)),
                (SystemEvent::DisplayOn, Some(false)),
                (SystemEvent::ThemeChanged, None),
                (SystemEvent::DisplaysChanged, None),
            ]
        );
        assert!(seen.borrow().contains(&(SystemEvent::PowerChanged, None)));
        drop(w);
    }

    #[test]
    fn hooks_report_own_window_destroy() {
        let (tx, rx) = std::sync::mpsc::channel();
        let hooks = HookSet::install(tx);
        assert_eq!(hooks.count(), 4);
        let win = HostWindow::create(w!("FrostpaneHookProbe"), false, Box::new(|_, _, _| {})).expect("window");
        let id = win.hwnd.0 as isize as i64;
        drop(win);
        let end = Instant::now() + Duration::from_secs(3);
        let mut found = false;
        while !found && Instant::now() < end {
            pump_messages(20);
            found = rx.try_iter().any(|e| e == HookEvent::Destroy(id));
        }
        assert!(found, "no Destroy event for our own window");
        drop(hooks);
        assert!(HOOK_TX.lock().unwrap().is_none());
    }
}
