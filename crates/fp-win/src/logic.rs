//! Pure, platform-independent decisions used by the Windows layer.
//!
//! Everything here is unit tested; the Win32 code only gathers raw facts and
//! feeds them through these functions.

use crate::PowerStatus;
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
pub use fp_platform::events::{SleepState, SystemEvent};
use fp_tracker::{HookEvent, WindowInfo};
use std::collections::HashMap;
use std::path::Path;

pub const WS_EX_TOPMOST: u32 = 0x8;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

/// `"tr-TR"` -> `"tr"`. Empty input gives `"en"`.
pub fn lang2(locale: &str) -> String {
    let code: String = locale.chars().take_while(|c| c.is_ascii_alphabetic()).take(2).collect();
    if code.len() < 2 { "en".into() } else { code.to_ascii_lowercase() }
}

/// desktop-native 3.7 battery saver rule. `ok` is the API result.
pub fn power_from(ok: bool, ac_line: u8, battery_flag: u8, system_flag: u8) -> PowerStatus {
    if !ok {
        return PowerStatus::default();
    }
    let on_battery = ac_line == 0;
    // 128 = no system battery, 255 = unknown.
    let has_battery = battery_flag != 128 && battery_flag != 255;
    let saving = on_battery || system_flag == 1;
    PowerStatus { on_battery, has_battery, saving }
}

/// Rounded app windows start with Windows 11 (build 22000).
pub fn corner_for_build(build: u32) -> f64 {
    if build >= 22000 { 8.0 } else { 0.0 }
}

/// Invariant-culture float, as returned by `GetGeoInfoW(GEO_LATITUDE)`.
pub fn parse_latitude(s: &str) -> Option<f64> {
    let v: f64 = s.trim_end_matches('\0').trim().parse().ok()?;
    v.is_finite().then_some(v)
}

/// One monitor as enumerated, before names are attached.
#[derive(Clone, Debug, PartialEq)]
pub struct RawDisplay {
    pub gdi: String,
    pub pixels: PixelRect,
    pub work: PixelRect,
    pub scale: f64,
    pub primary: bool,
}

/// desktop-native 3.6 `Displays.All`. `names` maps the lower-cased GDI name to
/// (friendly name, device path).
pub fn assemble_displays(mut raws: Vec<RawDisplay>, names: &HashMap<String, (String, String)>) -> Vec<MonitorInfo> {
    raws.sort_by_key(|r| (!r.primary, r.pixels.left));
    let mut out: Vec<MonitorInfo> = raws
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            let (friendly, path) = names.get(&r.gdi.to_lowercase()).cloned().unwrap_or_default();
            let id = if path.is_empty() { r.gdi.clone() } else { path };
            let name = if friendly.trim().is_empty() { format!("Screen {}", i + 1) } else { friendly };
            MonitorInfo { id, name, pixels: r.pixels, work: r.work, scale: r.scale, primary: r.primary }
        })
        .collect();
    let mut count: HashMap<String, usize> = HashMap::new();
    for m in &out {
        *count.entry(m.name.clone()).or_default() += 1;
    }
    for (i, m) in out.iter_mut().enumerate() {
        if count[&m.name] > 1 {
            m.name = format!("{} {}", m.name, i + 1);
        }
    }
    out
}

/// Run-key value data: quoted exe path, a space, `--quiet` (core-app 2.10).
pub fn run_command(exe: &Path) -> String {
    format!("\"{}\" --quiet", exe.display())
}

/// StartupApproved check: a non-empty REG_BINARY whose first-byte bit 0 is set
/// means Windows has the item disabled. Any other shape is "not blocked".
pub fn startup_blocked(is_binary: bool, data: &[u8]) -> bool {
    is_binary && data.first().is_some_and(|b| b & 1 == 1)
}

pub const EVENT_SYSTEM_FOREGROUND: u32 = 3;
pub const EVENT_SYSTEM_MOVESIZESTART: u32 = 10;
pub const EVENT_SYSTEM_MOVESIZEEND: u32 = 11;
pub const EVENT_SYSTEM_MINIMIZESTART: u32 = 22;
pub const EVENT_OBJECT_DESTROY: u32 = 32769;
pub const EVENT_OBJECT_HIDE: u32 = 32771;

/// desktop-native 3.5.1: raw WinEvent -> tracker event. `SHOW` is ignored.
pub fn map_hook(event: u32, hwnd: i64, id_object: i32, id_child: i32) -> Option<HookEvent> {
    if hwnd == 0 || id_object != 0 || id_child != 0 {
        return None;
    }
    match event {
        EVENT_SYSTEM_FOREGROUND => Some(HookEvent::Foreground),
        EVENT_SYSTEM_MOVESIZESTART => Some(HookEvent::MoveSizeStart),
        EVENT_SYSTEM_MOVESIZEEND => Some(HookEvent::MoveSizeEnd),
        EVENT_SYSTEM_MINIMIZESTART => Some(HookEvent::MinimizeStart(hwnd)),
        EVENT_OBJECT_DESTROY => Some(HookEvent::Destroy(hwnd)),
        EVENT_OBJECT_HIDE => Some(HookEvent::Hide(hwnd)),
        _ => None,
    }
}

/// desktop-native 3.5.6: only bottom taskbars (wider than tall) are shelves.
pub fn is_shelf(r: &PixelRect) -> bool {
    r.width() > r.height()
}

/// ABE_BOTTOM
pub const ABE_BOTTOM: u32 = 3;
/// ABS_AUTOHIDE
pub const ABS_AUTOHIDE: u32 = 1;

/// Only `https:` and `ms-settings:` links are opened (core-app 2.15, 2.10).
pub fn url_allowed(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("ms-settings:")
}

pub const WTS_CONSOLE_CONNECT: usize = 1;
pub const WTS_CONSOLE_DISCONNECT: usize = 2;
pub const WTS_REMOTE_CONNECT: usize = 3;
pub const WTS_REMOTE_DISCONNECT: usize = 4;
pub const WTS_SESSION_LOCK: usize = 7;
pub const WTS_SESSION_UNLOCK: usize = 8;

/// WM_WTSSESSION_CHANGE wParam.
pub fn session_event(w: usize) -> Option<SystemEvent> {
    match w {
        WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => Some(SystemEvent::Locked),
        WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => Some(SystemEvent::Unlocked),
        _ => None,
    }
}

pub const PBT_APMSUSPEND: usize = 0x4;
pub const PBT_APMPOWERSTATUSCHANGE: usize = 0xA;
pub const PBT_APMRESUMESUSPEND: usize = 0x7;
pub const PBT_APMRESUMEAUTOMATIC: usize = 0x12;

/// WM_POWERBROADCAST wParam: suspend and resume drive sleep; a status change
/// only asks for a battery re-read.
pub fn power_mode_event(w: usize) -> Option<SystemEvent> {
    match w {
        PBT_APMSUSPEND => Some(SystemEvent::Suspend),
        PBT_APMRESUMESUSPEND | PBT_APMRESUMEAUTOMATIC => Some(SystemEvent::Resume),
        PBT_APMPOWERSTATUSCHANGE => Some(SystemEvent::PowerChanged),
        _ => None,
    }
}

/// Console display state payload: 0 off, 1 on, 2 dimmed (on).
pub fn display_state_event(data: u32) -> SystemEvent {
    if data == 0 { SystemEvent::DisplayOff } else { SystemEvent::DisplayOn }
}

/// desktop-native 3.10 `TaskbarAreas`: parts of the monitor outside the work area.
pub fn taskbar_areas(m: &MonitorInfo) -> Vec<PixelRect> {
    let (p, w) = (m.pixels, m.work);
    [
        PixelRect::new(p.left, w.bottom, p.right, p.bottom),
        PixelRect::new(p.left, p.top, p.right, w.top),
        PixelRect::new(p.left, p.top, w.left, p.bottom),
        PixelRect::new(w.right, p.top, p.right, p.bottom),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
}

/// desktop-native 3.10 `FullScreen`: monitors fully covered by `win`, unless the
/// window is ours or a shell window.
pub fn fullscreen_monitors(win: &PixelRect, own: bool, class: &str, monitors: &[MonitorInfo]) -> Vec<PixelRect> {
    if own || matches!(class, "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return Vec::new();
    }
    monitors
        .iter()
        .map(|m| m.pixels)
        .filter(|p| win.left <= p.left && win.top <= p.top && win.right >= p.right && win.bottom >= p.bottom)
        .collect()
}

/// desktop-native 3.10 `WindowFrames`: snap targets for decorations.
pub fn snap_frame(w: &WindowInfo) -> Option<PixelRect> {
    if !w.visible || w.iconic || w.cloaked {
        return None;
    }
    if w.ex_style & 0x80 != 0 && w.ex_style & 0x4_0000 == 0 {
        return None;
    }
    if w.ex_style & (WS_EX_TOPMOST | WS_EX_NOACTIVATE) != 0 || w.own_process {
        return None;
    }
    if matches!(w.class.as_str(), "Progman" | "WorkerW") {
        return None;
    }
    w.bounds.filter(|b| b.width() > 0 && b.height() > 0)
}

/// Walks the z-order from the top as (hwnd, class, topmost) until `hwnd`
/// (desktop-native 3.10 `TaskbarAbove`).
pub fn taskbar_above(chain: &[(i64, String, bool)], hwnd: i64) -> bool {
    for (h, class, topmost) in chain {
        if *h == hwnd {
            return false;
        }
        if *topmost && (class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd") {
            return true;
        }
        if !*topmost {
            return false;
        }
    }
    false
}

/// core-app 2.6 `IconSpot`: the tray icon rect counts only when it lies inside
/// the taskbar window (otherwise the icon sits in the overflow flyout).
pub fn icon_spot(icon: PixelRect, taskbar: Option<PixelRect>) -> Option<PixelRect> {
    let t = taskbar?;
    let inside = !icon.is_empty()
        && icon.left >= t.left
        && icon.top >= t.top
        && icon.right <= t.right
        && icon.bottom <= t.bottom;
    inside.then_some(icon)
}

/// File name of a full image path (for `WindowInfo::process_name`).
pub fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

/// `SystemUsesLightTheme`: light unless the value is 0. Errors default to dark.
pub fn taskbar_light(value: Option<u32>) -> bool {
    value.is_some_and(|v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(gdi: &str, left: i32, primary: bool) -> RawDisplay {
        RawDisplay {
            gdi: gdi.into(),
            pixels: PixelRect::new(left, 0, left + 1920, 1080),
            work: PixelRect::new(left, 0, left + 1920, 1040),
            scale: 1.0,
            primary,
        }
    }

    #[test]
    fn lang_codes() {
        assert_eq!(lang2("tr-TR"), "tr");
        assert_eq!(lang2("EN-us"), "en");
        assert_eq!(lang2(""), "en");
        assert_eq!(lang2("x"), "en");
        assert_eq!(lang2("zh-Hans-CN"), "zh");
    }

    #[test]
    fn power_rules() {
        assert_eq!(power_from(false, 0, 0, 1), PowerStatus::default());
        let p = power_from(true, 0, 1, 0);
        assert!(p.on_battery && p.saving && p.has_battery);
        let p = power_from(true, 1, 128, 0);
        assert!(!p.on_battery && !p.saving && !p.has_battery);
        let p = power_from(true, 255, 9, 1);
        assert!(!p.on_battery && p.saving && p.has_battery);
        assert!(!power_from(true, 255, 255, 0).has_battery);
    }

    #[test]
    fn corner() {
        assert_eq!(corner_for_build(19045), 0.0);
        assert_eq!(corner_for_build(22000), 8.0);
        assert_eq!(corner_for_build(26100), 8.0);
    }

    #[test]
    fn latitude() {
        assert_eq!(parse_latitude("-33.5"), Some(-33.5));
        assert_eq!(parse_latitude(" 41.0\0"), Some(41.0));
        assert_eq!(parse_latitude("abc"), None);
        assert_eq!(parse_latitude("NaN"), None);
        assert_eq!(parse_latitude(""), None);
    }

    #[test]
    fn displays_sorted_named_and_deduped() {
        let mut names = HashMap::new();
        names.insert("\\\\.\\display1".to_string(), ("Dell U2720".to_string(), "\\\\?\\DISPLAY#A".to_string()));
        names.insert("\\\\.\\display3".to_string(), ("Dell U2720".to_string(), "\\\\?\\DISPLAY#C".to_string()));
        let v = assemble_displays(
            vec![
                raw("\\\\.\\DISPLAY3", 3840, false),
                raw("\\\\.\\DISPLAY2", 1920, false),
                raw("\\\\.\\DISPLAY1", 0, true),
            ],
            &names,
        );
        assert_eq!(v.len(), 3);
        assert!(v[0].primary);
        assert_eq!(v[0].id, "\\\\?\\DISPLAY#A");
        assert_eq!(v[0].name, "Dell U2720 1");
        // No name info: GDI id and numbered label by sorted index.
        assert_eq!(v[1].id, "\\\\.\\DISPLAY2");
        assert_eq!(v[1].name, "Screen 2");
        assert_eq!(v[2].name, "Dell U2720 3");
    }

    #[test]
    fn displays_blank_friendly_falls_back() {
        let mut names = HashMap::new();
        names.insert("\\\\.\\display1".to_string(), ("  ".to_string(), String::new()));
        let v = assemble_displays(vec![raw("\\\\.\\DISPLAY1", 0, true)], &names);
        assert_eq!(v[0].name, "Screen 1");
        assert_eq!(v[0].id, "\\\\.\\DISPLAY1");
        assert!(assemble_displays(vec![], &names).is_empty());
    }

    #[test]
    fn run_value_and_approval() {
        assert_eq!(run_command(Path::new("C:\\A B\\fp.exe")), "\"C:\\A B\\fp.exe\" --quiet");
        assert!(startup_blocked(true, &[3, 0, 0]));
        assert!(startup_blocked(true, &[1]));
        assert!(!startup_blocked(true, &[2, 0]));
        assert!(!startup_blocked(true, &[]));
        assert!(!startup_blocked(false, &[1]));
    }

    #[test]
    fn hook_mapping() {
        assert_eq!(map_hook(3, 5, 0, 0), Some(HookEvent::Foreground));
        assert_eq!(map_hook(10, 5, 0, 0), Some(HookEvent::MoveSizeStart));
        assert_eq!(map_hook(11, 5, 0, 0), Some(HookEvent::MoveSizeEnd));
        assert_eq!(map_hook(22, 5, 0, 0), Some(HookEvent::MinimizeStart(5)));
        assert_eq!(map_hook(32769, 5, 0, 0), Some(HookEvent::Destroy(5)));
        assert_eq!(map_hook(32771, 5, 0, 0), Some(HookEvent::Hide(5)));
        assert_eq!(map_hook(32770, 5, 0, 0), None);
        assert_eq!(map_hook(3, 0, 0, 0), None);
        assert_eq!(map_hook(3, 5, -4, 0), None);
        assert_eq!(map_hook(3, 5, 0, 1), None);
    }

    #[test]
    fn shelves_and_urls() {
        assert!(is_shelf(&PixelRect::new(0, 1040, 1920, 1080)));
        assert!(!is_shelf(&PixelRect::new(0, 0, 48, 1080)));
        assert!(!is_shelf(&PixelRect::new(0, 0, 40, 40)));
        assert!(url_allowed("https://example.org/x"));
        assert!(url_allowed(" MS-SETTINGS:startupapps"));
        assert!(!url_allowed("http://example.org"));
        assert!(!url_allowed("file:///c:/x.exe"));
        assert!(!url_allowed("calc.exe"));
    }

    #[test]
    fn tray_icon_spot_inside_taskbar_only() {
        let bar = PixelRect::new(0, 1040, 1920, 1080);
        let icon = PixelRect::new(1700, 1048, 1724, 1072);
        assert_eq!(icon_spot(icon, Some(bar)), Some(icon));
        assert_eq!(icon_spot(icon, None), None);
        // Overflow flyout above the taskbar.
        assert_eq!(icon_spot(PixelRect::new(1700, 900, 1724, 924), Some(bar)), None);
        // Straddling the edge is not inside.
        assert_eq!(icon_spot(PixelRect::new(1910, 1048, 1934, 1072), Some(bar)), None);
        assert_eq!(icon_spot(PixelRect::new(5, 1050, 5, 1050), Some(bar)), None);
    }

    #[test]
    fn event_mapping() {
        assert_eq!(session_event(7), Some(SystemEvent::Locked));
        assert_eq!(session_event(2), Some(SystemEvent::Locked));
        assert_eq!(session_event(4), Some(SystemEvent::Locked));
        assert_eq!(session_event(8), Some(SystemEvent::Unlocked));
        assert_eq!(session_event(1), Some(SystemEvent::Unlocked));
        assert_eq!(session_event(3), Some(SystemEvent::Unlocked));
        assert_eq!(session_event(5), None);
        assert_eq!(power_mode_event(4), Some(SystemEvent::Suspend));
        assert_eq!(power_mode_event(7), Some(SystemEvent::Resume));
        assert_eq!(power_mode_event(0x12), Some(SystemEvent::Resume));
        assert_eq!(power_mode_event(0xA), Some(SystemEvent::PowerChanged));
        assert_eq!(power_mode_event(0x8013), None);
        assert_eq!(power_mode_event(0x9), None);
        assert_eq!(display_state_event(0), SystemEvent::DisplayOff);
        assert_eq!(display_state_event(1), SystemEvent::DisplayOn);
        assert_eq!(display_state_event(2), SystemEvent::DisplayOn);
    }

    fn mon(work_bottom: i32) -> MonitorInfo {
        MonitorInfo {
            id: "a".into(),
            name: "a".into(),
            pixels: PixelRect::new(0, 0, 1920, 1080),
            work: PixelRect::new(0, 0, 1920, work_bottom),
            scale: 1.0,
            primary: true,
        }
    }

    #[test]
    fn taskbar_area_parts() {
        assert_eq!(taskbar_areas(&mon(1040)), vec![PixelRect::new(0, 1040, 1920, 1080)]);
        assert!(taskbar_areas(&mon(1080)).is_empty());
        let mut m = mon(1080);
        m.work = PixelRect::new(60, 30, 1900, 1080);
        assert_eq!(
            taskbar_areas(&m),
            vec![PixelRect::new(0, 0, 1920, 30), PixelRect::new(0, 0, 60, 1080), PixelRect::new(1900, 0, 1920, 1080)]
        );
    }

    #[test]
    fn fullscreen() {
        let ms = [mon(1040)];
        let full = PixelRect::new(-5, -5, 1925, 1085);
        assert_eq!(fullscreen_monitors(&full, false, "App", &ms).len(), 1);
        assert!(fullscreen_monitors(&full, true, "App", &ms).is_empty());
        assert!(fullscreen_monitors(&full, false, "Progman", &ms).is_empty());
        assert!(fullscreen_monitors(&PixelRect::new(0, 0, 1900, 1080), false, "App", &ms).is_empty());
    }

    fn win() -> WindowInfo {
        WindowInfo {
            hwnd: 1,
            visible: true,
            class: "App".into(),
            bounds: Some(PixelRect::new(0, 0, 10, 10)),
            ..Default::default()
        }
    }

    #[test]
    fn snap_frames() {
        assert!(snap_frame(&win()).is_some());
        let mut w = win();
        w.iconic = true;
        assert!(snap_frame(&w).is_none());
        let mut w = win();
        w.ex_style = 0x80;
        assert!(snap_frame(&w).is_none());
        w.ex_style = 0x80 | 0x4_0000;
        assert!(snap_frame(&w).is_some());
        w.ex_style = WS_EX_TOPMOST;
        assert!(snap_frame(&w).is_none());
        let mut w = win();
        w.own_process = true;
        assert!(snap_frame(&w).is_none());
        let mut w = win();
        w.class = "WorkerW".into();
        assert!(snap_frame(&w).is_none());
        let mut w = win();
        w.bounds = None;
        assert!(snap_frame(&w).is_none());
        w.bounds = Some(PixelRect::new(0, 0, 0, 5));
        assert!(snap_frame(&w).is_none());
        let mut w = win();
        w.visible = false;
        assert!(snap_frame(&w).is_none());
        let mut w = win();
        w.cloaked = true;
        assert!(snap_frame(&w).is_none());
    }

    #[test]
    fn taskbar_above_walk() {
        let c = |h, k: &str, t| (h, k.to_string(), t);
        assert!(taskbar_above(&[c(1, "Shell_TrayWnd", true), c(2, "X", false)], 2));
        assert!(!taskbar_above(&[c(3, "X", false), c(1, "Shell_TrayWnd", true)], 2));
        assert!(!taskbar_above(&[c(2, "X", false)], 2));
        assert!(!taskbar_above(&[c(4, "Y", true), c(2, "X", false)], 2));
        assert!(!taskbar_above(&[], 2));
    }

    #[test]
    fn misc() {
        assert_eq!(file_name("C:\\a\\b\\App.exe"), "App.exe");
        assert_eq!(file_name("App.exe"), "App.exe");
        assert!(taskbar_light(Some(1)));
        assert!(!taskbar_light(Some(0)));
        assert!(!taskbar_light(None));
    }
}
