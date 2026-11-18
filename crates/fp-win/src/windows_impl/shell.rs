//! Shell-facing services: links, login item, wallpaper, theme, locale, power.

use super::reg;
use crate::logic;
use crate::{PowerStatus, WallpaperScreen};
use std::path::{Path, PathBuf};
use windows::Win32::Globalization::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Power::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Time::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, PWSTR, w};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const THEME_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const NOTIFY_KEY: &str = "Control Panel\\NotifyIconSettings";
const VALUE_NAME: &str = "Frostpane";

/// Opens an https or `ms-settings:` link in the default handler; anything else
/// and all errors are ignored.
pub fn open_url(url: &str) {
    if !logic::url_allowed(url) {
        return;
    }
    let u = reg::wide(url.trim());
    // SAFETY: NUL-terminated string lives across the call.
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Run value exists (any type) and Windows has not disabled it.
pub fn login_is_on() -> bool {
    reg::read(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME).is_some() && !login_blocked()
}

/// StartupApproved marks the item as disabled.
pub fn login_blocked() -> bool {
    match reg::read(HKEY_CURRENT_USER, APPROVED_KEY, VALUE_NAME) {
        Some((ty, data)) => logic::startup_blocked(ty == REG_BINARY, &data),
        None => false,
    }
}

/// Writes or deletes the Run value. Returns `false` when Windows blocks the item
/// or the registry write fails (an absent value counts as deleted).
pub fn login_set(on: bool) -> bool {
    if !on {
        return reg::delete_value(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME);
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    if !reg::write_string(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME, &logic::run_command(&exe)) {
        return false;
    }
    !login_blocked()
}

/// Windows taskbar uses the light theme (default dark on any error).
pub fn taskbar_light() -> bool {
    logic::taskbar_light(reg::read_dword(HKEY_CURRENT_USER, THEME_KEY, "SystemUsesLightTheme"))
}

/// Windows 11 notification-area setting for this exe: `Some(true)` when the icon
/// is shown on the taskbar (promoted), `Some(false)` when in the overflow,
/// `None` when Windows has no entry for it (callers treat that as shown, per
/// core-app 2.6 `IconOnTaskbar`).
pub fn tray_icon_promoted(exe: &Path) -> Option<bool> {
    let want = exe.to_string_lossy().to_lowercase();
    for k in reg::subkeys(HKEY_CURRENT_USER, NOTIFY_KEY) {
        let path = format!("{NOTIFY_KEY}\\{k}");
        let Some(p) = reg::read_string(HKEY_CURRENT_USER, &path, "ExecutablePath") else { continue };
        if p.to_lowercase() == want {
            return Some(reg::read_dword(HKEY_CURRENT_USER, &path, "IsPromoted") == Some(1));
        }
    }
    None
}

/// Screen rect of a tray icon (core-app 2.6 `IconSpot`), given the hidden owner
/// window and icon id the tray layer registered it with. `None` when the
/// icon is in the overflow flyout or the query fails; callers then use the
/// cursor position.
pub fn tray_icon_spot(owner: i64, icon_id: u32) -> Option<fp_core::geom::PixelRect> {
    use fp_core::geom::PixelRect;
    use windows::Win32::Foundation::{HWND, RECT};
    let ident = NOTIFYICONIDENTIFIER {
        cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
        hWnd: HWND(owner as isize as *mut _),
        uID: icon_id,
        ..Default::default()
    };
    // SAFETY: the identifier is fully initialised; the RECT outputs are local.
    unsafe {
        let r = Shell_NotifyIconGetRect(&ident).ok()?;
        let icon = PixelRect::new(r.left, r.top, r.right, r.bottom);
        let bar = FindWindowW(w!("Shell_TrayWnd"), None).ok().and_then(|h| {
            let mut t = RECT::default();
            GetWindowRect(h, &mut t).ok().map(|_| PixelRect::new(t.left, t.top, t.right, t.bottom))
        });
        logic::icon_spot(icon, bar)
    }
}

/// `icon` when it lies inside the `Shell_TrayWnd` rect (core-app 2.6 `IconSpot`).
pub fn tray_icon_spot_in_taskbar(icon: fp_core::geom::PixelRect) -> Option<fp_core::geom::PixelRect> {
    use fp_core::geom::PixelRect;
    use windows::Win32::Foundation::RECT;
    // SAFETY: plain window queries; the RECT output is local.
    let bar = unsafe {
        FindWindowW(w!("Shell_TrayWnd"), None).ok().and_then(|h| {
            let mut t = RECT::default();
            GetWindowRect(h, &mut t).ok().map(|_| PixelRect::new(t.left, t.top, t.right, t.bottom))
        })
    };
    logic::icon_spot(icon, bar)
}

/// Small-icon size in pixels (tray icon size).
pub fn small_icon_size() -> u32 {
    // SAFETY: plain query.
    let v = unsafe { GetSystemMetrics(SM_CXSMICON) };
    if v > 0 { v as u32 } else { 16 }
}

pub fn os_language() -> String {
    let mut b = [0u16; 85];
    // SAFETY: buffer of the documented maximum locale-name length.
    let n = unsafe { GetUserDefaultLocaleName(&mut b) };
    if n <= 0 {
        return "en".into();
    }
    logic::lang2(&String::from_utf16_lossy(&b[..(n as usize).saturating_sub(1)]))
}

pub fn power() -> PowerStatus {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: valid out pointer.
    let ok = unsafe { GetSystemPowerStatus(&mut s) }.is_ok();
    logic::power_from(ok, s.ACLineStatus, s.BatteryFlag, s.SystemStatusFlag)
}

pub fn reduced_motion() -> bool {
    let mut on = BOOL(1);
    // SAFETY: out pointer is a BOOL as the action requires.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut BOOL as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok();
    ok && !on.as_bool()
}

pub fn time_zone_id() -> String {
    let mut t = DYNAMIC_TIME_ZONE_INFORMATION::default();
    // SAFETY: valid out pointer.
    unsafe { GetDynamicTimeZoneInformation(&mut t) };
    let n = t.TimeZoneKeyName.iter().position(|c| *c == 0).unwrap_or(t.TimeZoneKeyName.len());
    String::from_utf16_lossy(&t.TimeZoneKeyName[..n])
}

/// Nation-level latitude of the user's region (weather-sources 2.5).
pub fn region_latitude() -> Option<f64> {
    // SAFETY: plain query then a buffer of 32 chars as documented by the spec.
    unsafe {
        let geo = GetUserGeoID(GEOCLASS_NATION);
        if geo == -1 {
            return None;
        }
        let mut b = [0u16; 32];
        let n = GetGeoInfoW(geo, GEO_LATITUDE, Some(&mut b), 0);
        if n == 0 {
            return None;
        }
        logic::parse_latitude(&String::from_utf16_lossy(&b[..(n as usize).min(32)]))
    }
}

/// Windows build number from the registry (HKLM CurrentBuildNumber).
pub fn build_number() -> u32 {
    reg::read_string(HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "CurrentBuildNumber")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

pub fn window_corner() -> f64 {
    static CORNER: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *CORNER.get_or_init(|| logic::corner_for_build(build_number()))
}

// ---- wallpaper (desktop-native 3.12) ----

struct Com(bool);

impl Com {
    fn init() -> Com {
        // SAFETY: balanced with CoUninitialize in Drop when this call succeeded.
        Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: matches the successful CoInitializeEx.
            unsafe { CoUninitialize() };
        }
    }
}

fn desktop_wallpaper() -> Option<IDesktopWallpaper> {
    // SAFETY: COM was initialised by the caller.
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }.ok()
}

fn take_pwstr(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: COM-allocated NUL-terminated string, freed once here.
    unsafe {
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

fn monitor_ids(dw: &IDesktopWallpaper) -> Option<Vec<String>> {
    // SAFETY: plain COM calls on a live interface.
    unsafe {
        let n = dw.GetMonitorDevicePathCount().ok()?;
        let mut v = Vec::new();
        for i in 0..n {
            if let Ok(p) = dw.GetMonitorDevicePathAt(i)
                && let Some(s) = take_pwstr(p)
            {
                v.push(s);
            }
        }
        Some(v)
    }
}

/// Monitor id and pixel size for every monitor path whose rectangle is readable.
pub fn wallpaper_screens() -> Vec<WallpaperScreen> {
    let _com = Com::init();
    let Some(dw) = desktop_wallpaper() else { return Vec::new() };
    let mut out = Vec::new();
    for id in monitor_ids(&dw).unwrap_or_default() {
        let w = reg::wide(&id);
        // SAFETY: valid NUL-terminated id.
        if let Ok(r) = unsafe { dw.GetMonitorRECT(PCWSTR(w.as_ptr())) } {
            out.push(WallpaperScreen {
                id,
                width: (r.right - r.left).max(0) as u32,
                height: (r.bottom - r.top).max(0) as u32,
            });
        }
    }
    out
}

/// One wallpaper on every usable monitor. Falls back to the classic desktop
/// parameter when COM is unavailable or no monitor is usable.
pub fn wallpaper_set(path: &Path) -> bool {
    let _com = Com::init();
    let p = reg::wide(&path.to_string_lossy());
    if let Some(dw) = desktop_wallpaper()
        && let Some(ids) = monitor_ids(&dw)
    {
        // SAFETY: valid interface and NUL-terminated strings.
        unsafe {
            let _ = dw.SetPosition(DWPOS_FILL);
            let mut attempted = 0;
            let mut all = true;
            for id in ids {
                let w = reg::wide(&id);
                if dw.GetMonitorRECT(PCWSTR(w.as_ptr())).is_err() {
                    continue;
                }
                attempted += 1;
                if dw.SetWallpaper(PCWSTR(w.as_ptr()), PCWSTR(p.as_ptr())).is_err() {
                    all = false;
                }
            }
            if attempted > 0 {
                return all;
            }
        }
    }
    wallpaper_fallback(&p)
}

/// Per-monitor wallpapers `(monitor id, file)`. No fallback; empty list is `true`.
pub fn wallpaper_set_each(items: &[(String, PathBuf)]) -> bool {
    let _com = Com::init();
    let Some(dw) = desktop_wallpaper() else { return false };
    // SAFETY: valid interface and NUL-terminated strings.
    unsafe {
        let _ = dw.SetPosition(DWPOS_FILL);
        let mut all = true;
        for (id, file) in items {
            let (i, f) = (reg::wide(id), reg::wide(&file.to_string_lossy()));
            if dw.SetWallpaper(PCWSTR(i.as_ptr()), PCWSTR(f.as_ptr())).is_err() {
                all = false;
            }
        }
        all
    }
}

/// Wallpaper of monitor path index 0; `None` when that path or its wallpaper
/// cannot be read, or Windows reports no wallpaper file.
pub fn wallpaper_current() -> Option<PathBuf> {
    let _com = Com::init();
    let dw = desktop_wallpaper()?;
    // SAFETY: plain COM calls on a live interface; strings freed by `take_pwstr`.
    unsafe {
        if dw.GetMonitorDevicePathCount().ok()? == 0 {
            return None;
        }
        let id = take_pwstr(dw.GetMonitorDevicePathAt(0).ok()?)?;
        let w = reg::wide(&id);
        let p = take_pwstr(dw.GetWallpaper(PCWSTR(w.as_ptr())).ok()?)?;
        (!p.is_empty()).then(|| PathBuf::from(p))
    }
}

fn wallpaper_fallback(wide_path: &[u16]) -> bool {
    let key = "Control Panel\\Desktop";
    reg::write_string(HKEY_CURRENT_USER, key, "WallpaperStyle", "10");
    reg::write_string(HKEY_CURRENT_USER, key, "TileWallpaper", "0");
    // SAFETY: NUL-terminated path lives across the call.
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(wide_path.as_ptr() as *mut _),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    }
    .is_ok()
}

/// Shows a one-item popup at `at` (pixels) and returns true when it is chosen.
/// Built and destroyed per call, owned by a throw-away hidden window.
pub fn context_menu(label: &str, at: (i32, i32)) -> bool {
    const ID: usize = 1;
    // SAFETY: plain Win32 calls on handles created and destroyed here.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return false };
        let wide: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
        let added = AppendMenuW(menu, MF_STRING, ID, PCWSTR(wide.as_ptr())).is_ok();
        let owner =
            CreateWindowExW(WS_EX_TOOLWINDOW, w!("STATIC"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None);
        let chosen = match owner {
            Ok(hwnd) if added => {
                let _ = SetForegroundWindow(hwnd);
                let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, at.0, at.1, None, hwnd, None);
                let _ = PostMessageW(Some(hwnd), WM_NULL, Default::default(), Default::default());
                let _ = DestroyWindow(hwnd);
                cmd.0 as usize == ID
            }
            Ok(hwnd) => {
                let _ = DestroyWindow(hwnd);
                false
            }
            Err(_) => false,
        };
        let _ = DestroyMenu(menu);
        chosen
    }
}
