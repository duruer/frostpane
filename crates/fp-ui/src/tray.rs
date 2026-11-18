//! Tray icon and menu (core-app 2.6) on top of `tray-icon` and `muda`.

use crate::TrayAction;
use crate::menu::{Entry, Item, MenuIcon, plan, tray_icon_color, tray_icon_name};
use fp_core::effect::Lang;
use fp_core::settings::Weathers;
use fp_paint::{icon_rgba, wind_icon_rgba};
use fp_text::Key;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{IconMenuItem, Menu, MenuEvent, MenuId, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Menu image size (small icon).
const MENU_ICON: u32 = 16;
/// Tray image size.
const TRAY_ICON: u32 = 32;

/// Set by the menu key hook when Ctrl+Q was pressed in the open tray menu.
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Wake-up of the UI loop for `QUIT_REQUESTED`.
static QUIT_WAKE: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// True for exactly Ctrl+Q (core-app 2.3): `vk` is the key, no Alt, no Shift.
pub(crate) fn is_quit_chord(vk: u32, ctrl: bool, alt: bool, shift: bool) -> bool {
    vk == u32::from(b'Q') && ctrl && !alt && !shift
}

/// Record a quit request from the menu loop and wake the UI.
fn request_quit() {
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    if let Ok(g) = QUIT_WAKE.lock()
        && let Some(w) = g.as_ref()
    {
        w();
    }
}

fn take_quit() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::SeqCst)
}

/// `WH_MSGFILTER` hook on the UI thread: the modal menu loop of
/// `TrackPopupMenu` never translates accelerators, so Ctrl+Q is caught here.
#[cfg(windows)]
mod menu_hook {
    use super::{is_quit_chord, request_quit};
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, EndMenu, HHOOK, MSG, MSGF_MENU, SetWindowsHookExW, UnhookWindowsHookEx, WH_MSGFILTER,
        WM_KEYDOWN,
    };

    fn down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
        // SAFETY: GetKeyState has no preconditions.
        unsafe { GetKeyState(i32::from(vk.0)) < 0 }
    }

    unsafe extern "system" fn proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == MSGF_MENU as i32 && lparam.0 != 0 {
            // SAFETY: for MSGF_MENU, `lparam` points to the menu loop's MSG.
            let msg = unsafe { &*(lparam.0 as *const MSG) };
            if msg.message == WM_KEYDOWN
                && is_quit_chord(msg.wParam.0 as u32, down(VK_CONTROL), down(VK_MENU), down(VK_SHIFT))
            {
                // SAFETY: closes the menu being tracked on this thread.
                let _ = unsafe { EndMenu() };
                request_quit();
                return LRESULT(1);
            }
        }
        // SAFETY: forwards the unchanged hook arguments.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    /// Installed hook; removed on drop.
    pub struct Hook(HHOOK);

    impl Hook {
        pub fn install() -> Option<Self> {
            // SAFETY: thread-local hook with a `'static` procedure.
            unsafe { SetWindowsHookExW(WH_MSGFILTER, Some(proc), None, GetCurrentThreadId()) }.ok().map(Self)
        }
    }

    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: the handle came from `SetWindowsHookExW`.
            let _ = unsafe { UnhookWindowsHookEx(self.0) };
        }
    }
}

/// Tray icon + menu. Menu events are forwarded through our own channel
/// (a muda handler replaces the global receiver, so we install one).
pub struct Tray {
    icon: TrayIcon,
    actions: HashMap<MenuId, TrayAction>,
    /// Ids of the menu before the last `refresh`: a click sent just before a
    /// rebuild still maps to its action when it is polled afterwards.
    previous: HashMap<MenuId, TrayAction>,
    rx: Receiver<MenuEvent>,
    light_taskbar: bool,
    /// Ctrl+Q catcher for the open menu (the muda accelerator is only a label).
    #[cfg(windows)]
    _hook: Option<menu_hook::Hook>,
}

fn menu_icon(i: MenuIcon) -> Option<muda::Icon> {
    let rgba = match i {
        MenuIcon::Named(name, c) => icon_rgba(name, c, MENU_ICON),
        MenuIcon::Wind(w, c) => wind_icon_rgba(w, c, MENU_ICON),
    };
    muda::Icon::from_rgba(rgba, MENU_ICON, MENU_ICON).ok()
}

fn add_item(parent: &dyn Appender, it: &Item, map: &mut HashMap<MenuId, TrayAction>) -> Result<(), String> {
    let accel = it.ctrl_q.then(|| Accelerator::new(Modifiers::CONTROL, Code::KeyQ));
    let mi = IconMenuItem::new(&it.label, it.enabled, it.icon.and_then(menu_icon), accel);
    if let Some(a) = it.action {
        map.insert(mi.id().clone(), a);
    }
    parent.push(&mi)
}

/// Common `append` of `Menu` and `Submenu`.
trait Appender {
    fn push(&self, item: &dyn muda::IsMenuItem) -> Result<(), String>;
}

impl Appender for Menu {
    fn push(&self, item: &dyn muda::IsMenuItem) -> Result<(), String> {
        self.append(item).map_err(|e| e.to_string())
    }
}

impl Appender for Submenu {
    fn push(&self, item: &dyn muda::IsMenuItem) -> Result<(), String> {
        self.append(item).map_err(|e| e.to_string())
    }
}

fn add_entries(parent: &dyn Appender, entries: &[Entry], map: &mut HashMap<MenuId, TrayAction>) -> Result<(), String> {
    for e in entries {
        match e {
            Entry::Item(i) => add_item(parent, i, map)?,
            Entry::Separator => parent.push(&PredefinedMenuItem::separator())?,
            Entry::Submenu { label, icon, items } => {
                let sub = Submenu::new(label, true);
                if let Some(ic) = icon.and_then(menu_icon) {
                    sub.set_icon(Some(ic));
                }
                add_entries(&sub, items, map)?;
                parent.push(&sub)?;
            }
        }
    }
    Ok(())
}

fn build_menu(w: &Weathers, lang: Lang) -> Result<(Menu, HashMap<MenuId, TrayAction>), String> {
    let mut map = HashMap::new();
    let menu = Menu::new();
    add_entries(&menu, &plan(w, lang), &mut map)?;
    Ok((menu, map))
}

fn tray_image(w: &Weathers, light_taskbar: bool) -> Icon {
    let rgba = icon_rgba(tray_icon_name(w), tray_icon_color(light_taskbar), TRAY_ICON);
    Icon::from_rgba(rgba, TRAY_ICON, TRAY_ICON).unwrap_or_else(|_| crate::placeholder_icon())
}

impl Tray {
    /// `wake` is called from the menu thread on every event (e.g. request a repaint).
    pub fn new(w: &Weathers, lang: Lang, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let (tx, rx): (Sender<MenuEvent>, _) = channel();
        let tx = Mutex::new(tx);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        if let Ok(mut g) = QUIT_WAKE.lock() {
            *g = Some(Arc::clone(&wake));
        }
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if let Ok(t) = tx.lock() {
                let _ = t.send(e);
            }
            wake();
        }));
        let (menu, actions) = build_menu(w, lang)?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(fp_text::t(lang, Key::AppName))
            .with_icon(tray_image(w, false))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            icon,
            actions,
            previous: HashMap::new(),
            rx,
            light_taskbar: false,
            #[cfg(windows)]
            _hook: menu_hook::Hook::install(),
        })
    }

    /// Rebuild the menu and icon after settings or language changed.
    pub fn refresh(&mut self, w: &Weathers, lang: Lang) -> Result<(), String> {
        let (menu, actions) = build_menu(w, lang)?;
        self.icon.set_menu(Some(Box::new(menu)));
        self.previous = std::mem::replace(&mut self.actions, actions);
        self.icon.set_tooltip(Some(fp_text::t(lang, Key::AppName))).map_err(|e| e.to_string())?;
        self.icon.set_icon(Some(tray_image(w, self.light_taskbar))).map_err(|e| e.to_string())
    }

    /// Tell the tray whether the taskbar uses the light theme (the app reads
    /// `SystemUsesLightTheme`); the icon is repainted by the next `refresh`.
    pub fn set_light_taskbar(&mut self, light: bool) {
        self.light_taskbar = light;
    }

    /// Screen rect of the tray icon in physical pixels (Y down):
    /// `[left, top, width, height]`. `None` when Windows does not report it.
    pub fn icon_rect(&self) -> Option<[f64; 4]> {
        let r = self.icon.rect()?;
        let (w, h) = (f64::from(r.size.width), f64::from(r.size.height));
        (w > 0.0 && h > 0.0).then_some([r.position.x, r.position.y, w, h])
    }

    /// Drain pending clicks.
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut v: Vec<TrayAction> = self
            .rx
            .try_iter()
            .filter_map(|e| self.actions.get(&e.id).or_else(|| self.previous.get(&e.id)).copied())
            .collect();
        if take_quit() {
            v.push(TrayAction::Quit);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn quit_chord_is_exactly_ctrl_q() {
        let q = u32::from(b'Q');
        assert!(is_quit_chord(q, true, false, false));
        assert!(!is_quit_chord(q, false, false, false));
        assert!(!is_quit_chord(q, true, true, false));
        assert!(!is_quit_chord(q, true, false, true));
        assert!(!is_quit_chord(u32::from(b'W'), true, false, false));
    }

    #[test]
    fn menu_quit_request_wakes_and_is_taken_once() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = Arc::clone(&n);
        *QUIT_WAKE.lock().unwrap() = Some(Arc::new(move || {
            n2.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(!take_quit());
        request_quit();
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert!(take_quit());
        assert!(!take_quit());
        *QUIT_WAKE.lock().unwrap() = None;
    }
}
