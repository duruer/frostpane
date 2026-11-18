//! Shell services (porting design 3.1 "Other services"): links and folders,
//! launch at login, wallpaper, power, theme, locale, context menu, uptime.

use super::screens;
use crate::logic;
use fp_platform::{PowerStatus, WallpaperScreen};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSImageScaling, NSMenu, NSMenuItem, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{
    NSDictionary, NSLocale, NSNumber, NSPoint, NSProcessInfo, NSString, NSTimeZone, NSURL, NSUserDefaults, ns_string,
};
use objc2_io_kit::{IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList, IOPSGetProvidingPowerSourceType};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::path::{Path, PathBuf};

// ---- links ---------------------------------------------------------------------

pub fn open_url(url: &str) {
    if !logic::url_allowed(url) {
        return;
    }
    if let Some(u) = NSURL::URLWithString(&NSString::from_str(url.trim())) {
        NSWorkspace::sharedWorkspace().openURL(&u);
    }
}

fn file_url(p: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))
}

pub fn open_folder(dir: &Path) {
    NSWorkspace::sharedWorkspace().openURL(&file_url(dir));
}

// ---- launch at login (core-app 2.10) ------------------------------------------

fn exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// `SMAppService` needs the app bundle; a bare binary uses a launch agent.
fn bundled() -> bool {
    exe().is_some_and(|e| logic::in_app_bundle(&e))
}

fn service() -> Retained<SMAppService> {
    // SAFETY: plain class method (macOS 13+).
    unsafe { SMAppService::mainAppService() }
}

fn status() -> SMAppServiceStatus {
    // SAFETY: plain getter.
    unsafe { service().status() }
}

fn agent() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| logic::agent_path(Path::new(&h)))
}

pub fn login_is_on() -> bool {
    if bundled() { logic::login_state(status().0).0 } else { agent().is_some_and(|p| p.exists()) }
}

/// Registered but waiting for the user's approval in System Settings.
pub fn login_blocked() -> bool {
    bundled() && logic::login_state(status().0).1
}

pub fn login_set(on: bool) -> bool {
    if bundled() {
        let s = service();
        // SAFETY: plain SMAppService calls; errors are returned, not thrown.
        let r = unsafe { if on { s.registerAndReturnError() } else { s.unregisterAndReturnError() } };
        if let Err(e) = &r {
            log::warn!("login item: {}", e.localizedDescription());
        }
        return if on { login_is_on() } else { !login_is_on() };
    }
    let Some(path) = agent() else { return false };
    if !on {
        return std::fs::remove_file(&path).is_ok() || !path.exists();
    }
    let Some(exe) = exe() else { return false };
    path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && std::fs::write(&path, logic::agent_plist(&exe)).is_ok()
}

pub fn show_login_items() {
    if bundled() {
        // SAFETY: plain class method (macOS 13+).
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    } else {
        open_url(logic::LOGIN_ITEMS_URL);
    }
}

// ---- wallpaper (desktop-native 3.12) -------------------------------------------

pub fn wallpaper_screens(mtm: MainThreadMarker) -> Vec<WallpaperScreen> {
    screens::all(mtm)
        .iter()
        .map(|s| {
            let f = s.frame();
            let b = s.backingScaleFactor();
            WallpaperScreen {
                id: screens::key(s),
                width: (f.size.width * b).round().max(0.0) as u32,
                height: (f.size.height * b).round().max(0.0) as u32,
            }
        })
        .collect()
}

/// Fill the screen: scale proportionally, clip the overflow.
fn fill_options() -> Retained<NSDictionary<NSString, AnyObject>> {
    let scaling = NSNumber::new_usize(NSImageScaling::ScaleProportionallyUpOrDown.0);
    let clip = NSNumber::new_bool(true);
    // SAFETY: reading immutable framework constants.
    let keys: [&NSString; 2] = unsafe { [NSWorkspaceDesktopImageScalingKey, NSWorkspaceDesktopImageAllowClippingKey] };
    NSDictionary::from_slices(&keys, &[scaling.as_ref() as &AnyObject, clip.as_ref() as &AnyObject])
}

fn set_on(screen: &NSScreen, file: &Path) -> bool {
    let ws = NSWorkspace::sharedWorkspace();
    // SAFETY: the options dictionary holds the documented key/value types.
    match unsafe { ws.setDesktopImageURL_forScreen_options_error(&file_url(file), screen, &fill_options()) } {
        Ok(()) => true,
        Err(e) => {
            log::warn!("wallpaper: {}", e.localizedDescription());
            false
        }
    }
}

/// Applies to the current Space of each screen only.
pub fn wallpaper_set(mtm: MainThreadMarker, path: &Path) -> bool {
    let all = screens::all(mtm);
    // Every screen is tried, also after a failure.
    let done: Vec<bool> = all.iter().map(|s| set_on(s, path)).collect();
    !done.is_empty() && done.iter().all(|ok| *ok)
}

pub fn wallpaper_set_each(mtm: MainThreadMarker, items: &[(String, PathBuf)]) -> bool {
    let all = screens::all(mtm);
    items.iter().fold(true, |ok, (id, file)| {
        let hit = all.iter().find(|s| screens::key(s) == *id).is_some_and(|s| set_on(s, file));
        hit && ok
    })
}

pub fn wallpaper_current(mtm: MainThreadMarker) -> Option<PathBuf> {
    let main = screens::all(mtm).into_iter().next()?;
    let url = NSWorkspace::sharedWorkspace().desktopImageURLForScreen(&main)?;
    url.path().map(|p| PathBuf::from(p.to_string()))
}

// ---- power, theme, locale -------------------------------------------------------

pub fn power() -> PowerStatus {
    let info = IOPSCopyPowerSourcesInfo();
    // SAFETY: `info` is the blob these functions expect (or None).
    let (source, sources) = unsafe {
        (
            IOPSGetProvidingPowerSourceType(info.as_deref()).map(|s| s.to_string()),
            IOPSCopyPowerSourcesList(info.as_deref()).map_or(0, |l| l.count()),
        )
    };
    let low = NSProcessInfo::processInfo().isLowPowerModeEnabled();
    logic::power_from(source.as_deref(), sources > 0, low)
}

pub fn reduced_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// The menu bar follows the system appearance.
pub fn light_theme() -> bool {
    let style = NSUserDefaults::standardUserDefaults().stringForKey(ns_string!("AppleInterfaceStyle"));
    logic::light_theme(style.map(|s| s.to_string()).as_deref())
}

pub fn language() -> Option<String> {
    let first = NSLocale::preferredLanguages().firstObject()?;
    logic::lang_code(&first.to_string())
}

/// IANA id (e.g. `Europe/Istanbul`).
pub fn time_zone() -> String {
    NSTimeZone::localTimeZone().name().to_string()
}

/// Local time minus UTC in seconds, from `localtime_r`.
pub fn utc_offset_s() -> i32 {
    // SAFETY: `time(NULL)` has no preconditions; `localtime_r` writes into
    // the zeroed `tm` we own and returns null on failure.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i32
    }
}

/// Milliseconds since boot (`kern.boottime`), sleep included like Windows'
/// `GetTickCount64`; `u64::MAX` (never "at login") when unknown.
pub fn uptime_ms() -> u64 {
    let mut tv: libc::timeval = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let mut len = std::mem::size_of::<libc::timeval>();
    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    // SAFETY: `tv`/`len` describe a writable buffer of the size sysctl expects.
    let r = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 2, (&mut tv as *mut libc::timeval).cast(), &mut len, std::ptr::null_mut(), 0)
    };
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else { return u64::MAX };
    if r != 0 || tv.tv_sec <= 0 {
        return u64::MAX;
    }
    let boot = std::time::Duration::new(tv.tv_sec as u64, (tv.tv_usec.max(0) as u32).saturating_mul(1000));
    logic::uptime_ms(boot, now)
}

// ---- context menu -----------------------------------------------------------------

/// One-item menu at `at` (pixels); blocks while it tracks. True when the
/// item was chosen.
pub fn context_menu(mtm: MainThreadMarker, label: &str, at: (i32, i32)) -> bool {
    let (x, y) = screens::space().px_cocoa(at.0, at.1);
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    // SAFETY: an item without action; it is enabled explicitly below.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(label),
            None,
            ns_string!(""),
        )
    };
    item.setEnabled(true);
    menu.addItem(&item);
    menu.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(x, y), None)
}
