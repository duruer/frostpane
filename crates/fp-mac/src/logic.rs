//! Platform-free parts of the macOS backend, compiled on every target so the
//! unit tests run anywhere: the pixel space of porting design 2.4 (screens,
//! work areas, sprite frames, cursor), `CGWindowListCopyWindowInfo` entries ->
//! `fp_tracker::PortableWindow`, the Dock shelf, full-screen and cover checks,
//! window levels, notification names -> `SystemEvent`, and small text helpers
//! (language code, launch-agent plist, single-instance socket path).

use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_platform::coords::{cocoa_to_pixels, global_scale, pixels_to_cocoa, points_to_pixels};
use fp_platform::{PowerStatus, SystemEvent};
use fp_tracker::PortableWindow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---- window levels (CGWindowLevel.h) ---------------------------------------

/// `kCGMinimumWindowLevel`.
const MIN_LEVEL: i64 = i32::MIN as i64;
/// `kCGDesktopIconWindowLevel`: Finder's desktop icons.
pub const DESKTOP_ICON_LEVEL: i64 = MIN_LEVEL + 40;
/// `kCGNormalWindowLevel`.
pub const NORMAL_LEVEL: i64 = 0;
/// `kCGUtilityWindowLevel`: right below the Dock (`ZBand::UnderDock`).
pub const UNDER_DOCK_LEVEL: i64 = 19;
/// `kCGDockWindowLevel`. Foreign windows at or above it are system chrome.
pub const DOCK_LEVEL: i64 = 20;
/// `kCGStatusWindowLevel`: front overlay panels (above the Dock, so snow lies
/// on it) and the "above" panel of behind mode.
pub const PANEL_LEVEL: i64 = 25;
/// Our own topmost windows (cards, front sprites) stay above the panels.
pub const FRONT_LEVEL: i64 = PANEL_LEVEL + 1;
/// Behind mode: the main panel right above the desktop icons.
pub const BEHIND_LEVEL: i64 = DESKTOP_ICON_LEVEL + 1;
/// Desktop-band sprites, one above the behind panel (Windows puts them right
/// above the shell after the panel, so they end above it too).
pub const DESKTOP_SPRITE_LEVEL: i64 = DESKTOP_ICON_LEVEL + 2;

/// Sentinel `WinId` of the Dock (`SpriteBackend::lowest_taskbar`), so
/// `fp-layered`'s band logic needs no change: `Place::After(DOCK)` = level 19.
pub const DOCK_ID: isize = -0x0D0C;

/// The window at `level` sits below the Dock (`taskbar_above`). The Dock
/// window always exists, auto-hidden or not.
pub fn dock_above(level: i64) -> bool {
    level < DOCK_LEVEL
}

/// Corner radius of macOS app windows in points (porting design 3.1).
pub const WINDOW_CORNER: f64 = 10.0;

// ---- pixel space (porting design 2.4) ---------------------------------------

/// A Cocoa rect in points: `x`, `y` = bottom-left corner, Y up from the
/// primary screen's bottom edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CocoaRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl CocoaRect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
}

/// The synthetic pixel space: global scale `S` and primary height `H0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Space {
    pub scale: f64,
    pub primary_h: f64,
}

impl Default for Space {
    fn default() -> Self {
        Self { scale: 1.0, primary_h: 0.0 }
    }
}

impl Space {
    /// A Cocoa rect in pixels.
    pub fn rect_px(&self, r: CocoaRect) -> PixelRect {
        cocoa_to_pixels(r.x, r.y, r.w, r.h, self.primary_h, self.scale)
    }

    /// A pixel rect back to a Cocoa rect (window frames).
    pub fn rect_cocoa(&self, r: PixelRect) -> CocoaRect {
        let (x, y) = pixels_to_cocoa(r.left, r.bottom, self.primary_h, self.scale);
        let s = self.safe_scale();
        CocoaRect::new(x, y, f64::from(r.width()) / s, f64::from(r.height()) / s)
    }

    /// A Quartz point (Y down from the primary top-left: CGEvent,
    /// CGWindowList) in pixels.
    pub fn quartz_px(&self, x: f64, y: f64) -> (i32, i32) {
        let s = self.safe_scale();
        (px_floor(x * s), px_floor(y * s))
    }

    /// A Cocoa point (`NSEvent.mouseLocation`) in pixels.
    pub fn cocoa_px(&self, x: f64, y: f64) -> (i32, i32) {
        self.quartz_px(x, self.primary_h - y)
    }

    /// A pixel position as a Cocoa point.
    pub fn px_cocoa(&self, x: i32, y: i32) -> (f64, f64) {
        pixels_to_cocoa(x, y, self.primary_h, self.scale)
    }

    /// A pixel position as a Quartz point.
    pub fn px_quartz(&self, x: i32, y: i32) -> (f64, f64) {
        let s = self.safe_scale();
        (f64::from(x) / s, f64::from(y) / s)
    }

    fn safe_scale(&self) -> f64 {
        if self.scale.is_finite() && self.scale > 0.0 { self.scale } else { 1.0 }
    }
}

fn px_floor(v: f64) -> i32 {
    if v.is_nan() { 0 } else { v.floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32 }
}

/// One `NSScreen`, as read on the main thread. `NSScreen.screens[0]` is the
/// primary (menu bar) screen; its frame origin is (0, 0).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawScreen {
    /// Stable id ([`display_key`]).
    pub key: String,
    /// `NSScreen.localizedName`.
    pub name: String,
    pub frame: CocoaRect,
    pub visible: CocoaRect,
    /// `backingScaleFactor`.
    pub backing: f64,
}

/// The pixel space of a screen list.
pub fn space(screens: &[RawScreen]) -> Space {
    let scales: Vec<f64> = screens.iter().map(|s| s.backing).collect();
    Space { scale: global_scale(&scales), primary_h: screens.first().map_or(0.0, |s| s.frame.h) }
}

/// desktop-native 3.6 `Displays.All` on macOS: primary first, then by left
/// edge; every monitor gets the global scale; unique ids and names.
pub fn monitors(screens: &[RawScreen]) -> (Vec<MonitorInfo>, Space) {
    let sp = space(screens);
    let mut list: Vec<(usize, &RawScreen)> = screens.iter().enumerate().collect();
    list.sort_by(|a, b| (a.0 != 0).cmp(&(b.0 != 0)).then(a.1.frame.x.total_cmp(&b.1.frame.x)));
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<MonitorInfo> = list
        .into_iter()
        .enumerate()
        .map(|(i, (orig, s))| {
            let base = if s.key.is_empty() { format!("screen-{}", i + 1) } else { s.key.clone() };
            let n = ids.entry(base.clone()).or_default();
            *n += 1;
            let id = if *n > 1 { format!("{base}#{n}") } else { base };
            let name = if s.name.trim().is_empty() { format!("Screen {}", i + 1) } else { s.name.clone() };
            MonitorInfo {
                id,
                name,
                pixels: sp.rect_px(s.frame),
                work: sp.rect_px(s.visible),
                scale: sp.scale,
                primary: orig == 0,
            }
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
    (out, sp)
}

/// Stable display id from `CGDisplayVendorNumber` / `ModelNumber` /
/// `SerialNumber`; the `CGDirectDisplayID` when all three are unknown.
pub fn display_key(vendor: u32, model: u32, serial: u32, display: u32) -> String {
    if vendor == 0 && model == 0 && serial == 0 {
        format!("display-{display}")
    } else {
        format!("{vendor:x}-{model:x}-{serial:x}")
    }
}

/// The Dock strip of one screen: the part of `frame` below `visibleFrame`,
/// across the whole screen width (desktop-native 3.5.6 uses bottom taskbars
/// only; a left/right Dock or a hidden one gives none).
pub fn dock_shelf(s: &RawScreen, sp: Space) -> Option<PixelRect> {
    let gap = s.visible.y - s.frame.y;
    (gap.is_finite() && gap >= 1.0 && s.frame.w > 0.0)
        .then(|| sp.rect_px(CocoaRect::new(s.frame.x, s.frame.y, s.frame.w, gap.min(s.frame.h))))
}

/// Every screen's Dock strip.
pub fn shelves(screens: &[RawScreen], sp: Space) -> Vec<PixelRect> {
    screens.iter().filter_map(|s| dock_shelf(s, sp)).collect()
}

/// Cocoa frame of a sprite window whose top-left is at pixel `(x, y)`.
pub fn sprite_frame(x: i32, y: i32, w_px: u32, h_px: u32, sp: Space) -> CocoaRect {
    let h = i32::try_from(h_px).unwrap_or(i32::MAX);
    let w = i32::try_from(w_px).unwrap_or(i32::MAX);
    sp.rect_cocoa(PixelRect::new(x, y, x.saturating_add(w), y.saturating_add(h)))
}

/// `NSEvent.pressedMouseButtons` -> (left, right).
pub fn buttons(mask: usize) -> (bool, bool) {
    (mask & 1 != 0, mask & 2 != 0)
}

// ---- window list (CGWindowListCopyWindowInfo) -------------------------------

/// Keys of one `CGWindowListCopyWindowInfo` entry that the backend reads.
/// None of them needs Screen Recording permission (`kCGWindowName` does,
/// and is never read).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// `kCGWindowNumber`.
    Number,
    /// `kCGWindowLayer`.
    Layer,
    /// `kCGWindowAlpha`.
    Alpha,
    /// `kCGWindowOwnerPID`.
    OwnerPid,
    /// `kCGWindowOwnerName`.
    OwnerName,
}

/// Read access to one window dictionary.
pub trait WindowDict {
    fn number(&self, k: Key) -> Option<f64>;
    fn text(&self, k: Key) -> Option<String>;
    /// `kCGWindowBounds` as `(x, y, w, h)` Quartz points.
    fn bounds(&self) -> Option<[f64; 4]>;
}

/// One window of the list, front to back.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RawWindow {
    pub id: i64,
    pub layer: i64,
    /// Quartz points `(x, y, w, h)`; `None` when missing or not finite.
    pub bounds: Option<[f64; 4]>,
    /// 0..=1; 1 when missing.
    pub alpha: f64,
    pub pid: i64,
    pub owner: String,
}

/// Parses one entry; `None` without a window number.
pub fn raw_window(d: &impl WindowDict) -> Option<RawWindow> {
    let id = d.number(Key::Number).filter(|n| n.is_finite())? as i64;
    let int = |k| d.number(k).filter(|n| n.is_finite()).map_or(0, |n| n as i64);
    let bounds = d.bounds().filter(|b| b.iter().all(|v| v.is_finite()) && b[2] >= 0.0 && b[3] >= 0.0);
    Some(RawWindow {
        id,
        layer: int(Key::Layer),
        bounds,
        alpha: d.number(Key::Alpha).filter(|a| a.is_finite()).map_or(1.0, |a| a.clamp(0.0, 1.0)),
        pid: int(Key::OwnerPid),
        owner: d.text(Key::OwnerName).unwrap_or_default(),
    })
}

/// Owners whose layer-0 windows are system chrome, not app windows.
const SKIP_OWNERS: [&str; 2] = ["Window Server", "Dock"];

impl RawWindow {
    /// Bounds in pixels.
    pub fn rect_px(&self, sp: Space) -> Option<PixelRect> {
        self.bounds.map(|[x, y, w, h]| points_to_pixels(x, y, w, h, sp.scale))
    }

    /// An app window (layer 0) or an app's floating window (0 < layer < 20).
    fn is_app(&self) -> bool {
        (NORMAL_LEVEL..DOCK_LEVEL).contains(&self.layer) && !SKIP_OWNERS.contains(&self.owner.as_str())
    }
}

/// Porting design 3.1 mapping: layer 0 -> normal window, 0 < layer < 20 ->
/// topmost (occluder only), anything else (Dock, menu bar, Control Center,
/// notifications, desktop) -> skipped. Own windows keep the Windows rule
/// (`fp_tracker::filter` lists them unless topmost).
pub fn portable(w: &RawWindow, own_pid: i64, sp: Space) -> Option<PortableWindow> {
    if !w.is_app() {
        return None;
    }
    let alpha = (w.alpha < 1.0).then(|| (w.alpha * 255.0).round() as u8);
    Some(PortableWindow {
        id: w.id,
        bounds: w.rect_px(sp),
        iconic: false,
        cloaked: false,
        topmost: w.layer > NORMAL_LEVEL,
        own_process: w.pid == own_pid,
        alpha,
        owner: w.owner.clone(),
    })
}

/// A window that went to another Space (`HookSynth::take_cloaked`).
pub fn cloaked(id: i64) -> PortableWindow {
    PortableWindow { id, cloaked: true, ..Default::default() }
}

/// `SpriteBackend::fullscreen`: per screen, the frontmost foreign app window
/// on it when that window covers the whole screen (a full-screen Space or a
/// screen-sized window).
pub fn fullscreen(list: &[RawWindow], own_pid: i64, screens: &[PixelRect], sp: Space) -> Vec<PixelRect> {
    let mut out = Vec::new();
    for s in screens {
        let front = list
            .iter()
            .filter(|w| w.layer == NORMAL_LEVEL && w.is_app() && w.pid != own_pid && w.alpha > 0.0)
            .filter_map(|w| w.rect_px(sp))
            .find(|r| intersects(*r, *s));
        if let Some(r) = front
            && covers(r, *s)
            && !out.contains(&r)
        {
            out.push(r);
        }
    }
    out
}

/// `SpriteBackend::covered`: a visible foreign window from `above` (the
/// windows in front of ours) contains the Quartz point `at`.
pub fn covered(above: &[RawWindow], own_pid: i64, at: (f64, f64)) -> bool {
    above.iter().any(|w| {
        w.pid != own_pid
            && w.alpha > 0.01
            && w.owner != "Window Server"
            && w.bounds.is_some_and(|[x, y, bw, bh]| at.0 >= x && at.0 < x + bw && at.1 >= y && at.1 < y + bh)
    })
}

/// Own window numbers of a front-to-back list, with their topmost flag
/// (layer > 0), for `fp_platform::overlay::raise_targets`.
pub fn own_seen(list: &[RawWindow], own_pid: i64) -> Vec<fp_platform::overlay::Seen> {
    list.iter()
        .map(|w| fp_platform::overlay::Seen {
            hwnd: w.id as isize,
            topmost: w.layer > NORMAL_LEVEL,
            visible: true,
            own_thread: w.pid == own_pid,
        })
        .collect()
}

fn intersects(a: PixelRect, b: PixelRect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

fn covers(a: PixelRect, b: PixelRect) -> bool {
    a.left <= b.left && a.top <= b.top && a.right >= b.right && a.bottom >= b.bottom
}

// ---- notifications -> system events (desktop-native 3.7) --------------------

/// Notification name -> event. Workspace, distributed and app notifications
/// share one table; their names do not collide.
pub fn system_event(name: &str) -> Option<SystemEvent> {
    Some(match name {
        "NSWorkspaceWillSleepNotification" => SystemEvent::Suspend,
        "NSWorkspaceDidWakeNotification" => SystemEvent::Resume,
        "NSWorkspaceScreensDidSleepNotification" => SystemEvent::DisplayOff,
        "NSWorkspaceScreensDidWakeNotification" => SystemEvent::DisplayOn,
        "NSWorkspaceSessionDidResignActiveNotification" | "com.apple.screenIsLocked" => SystemEvent::Locked,
        "NSWorkspaceSessionDidBecomeActiveNotification" | "com.apple.screenIsUnlocked" => SystemEvent::Unlocked,
        "AppleInterfaceThemeChangedNotification" => SystemEvent::ThemeChanged,
        "NSApplicationDidChangeScreenParametersNotification" => SystemEvent::DisplaysChanged,
        "NSProcessInfoPowerStateDidChangeNotification" => SystemEvent::PowerChanged,
        _ => return None,
    })
}

/// Names [`system_event`] knows, by notification center.
pub const WORKSPACE_EVENTS: [&str; 6] = [
    "NSWorkspaceWillSleepNotification",
    "NSWorkspaceDidWakeNotification",
    "NSWorkspaceScreensDidSleepNotification",
    "NSWorkspaceScreensDidWakeNotification",
    "NSWorkspaceSessionDidResignActiveNotification",
    "NSWorkspaceSessionDidBecomeActiveNotification",
];
pub const DISTRIBUTED_EVENTS: [&str; 3] =
    ["com.apple.screenIsLocked", "com.apple.screenIsUnlocked", "AppleInterfaceThemeChangedNotification"];
pub const APP_EVENTS: [&str; 2] =
    ["NSApplicationDidChangeScreenParametersNotification", "NSProcessInfoPowerStateDidChangeNotification"];

// ---- small services ----------------------------------------------------------

/// Two-letter code of a BCP 47 tag such as `en-US`, `pt_BR` or `zh-Hans-CN`.
pub fn lang_code(tag: &str) -> Option<String> {
    let head = tag.trim().split(['-', '_', '.', '@']).next()?;
    (head.len() == 2 && head.chars().all(|c| c.is_ascii_alphabetic())).then(|| head.to_ascii_lowercase())
}

/// `https:` links and System Settings panes; everything else is ignored.
pub fn url_allowed(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("https://") || l.starts_with("x-apple.systempreferences:")
}

/// System Settings > General > Login Items.
pub const LOGIN_ITEMS_URL: &str = "x-apple.systempreferences:com.apple.LoginItems-Settings.extension";

/// `IOPSGetProvidingPowerSourceType` value, a battery present, Low Power Mode.
pub fn power_from(source: Option<&str>, has_battery: bool, low_power: bool) -> PowerStatus {
    let on_battery = source == Some("Battery Power");
    PowerStatus { on_battery, has_battery: has_battery || on_battery, saving: on_battery || low_power }
}

/// `AppleInterfaceStyle` user default: `Dark` in dark mode, absent in light.
pub fn light_theme(style: Option<&str>) -> bool {
    !style.is_some_and(|s| s.trim().eq_ignore_ascii_case("dark"))
}

/// Milliseconds since boot from `kern.boottime` and the current Unix time.
pub fn uptime_ms(boot: std::time::Duration, now: std::time::Duration) -> u64 {
    u64::try_from(now.saturating_sub(boot).as_millis()).unwrap_or(u64::MAX)
}

/// `SMAppService.status` raw value -> (on, blocked).
pub fn login_state(status: isize) -> (bool, bool) {
    match status {
        1 => (true, false),
        2 => (false, true),
        _ => (false, false),
    }
}

/// The executable runs from inside an `.app` bundle (SMAppService needs one).
pub fn in_app_bundle(exe: &Path) -> bool {
    let s = exe.to_string_lossy();
    s.contains(".app/Contents/MacOS/")
}

/// Label of the fallback launch agent.
pub const AGENT_LABEL: &str = "io.github.duruer.frostpane";

/// `~/Library/LaunchAgents/<label>.plist`.
pub fn agent_path(home: &Path) -> PathBuf {
    home.join("Library/LaunchAgents").join(format!("{AGENT_LABEL}.plist"))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Launch agent that starts `exe --quiet` at login (core-app 2.10), for a
/// binary outside an `.app` bundle.
pub fn agent_plist(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{AGENT_LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{}</string>\n\t\t<string>--quiet</string>\n\t</array>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
         </dict>\n</plist>\n",
        xml_escape(&exe.to_string_lossy())
    )
}

/// `sun_path` holds 104 bytes on macOS, NUL included.
const SUN_PATH_MAX: usize = 103;

/// Single-instance socket: `$TMPDIR/frostpane-<uid>.sock`, else under `/tmp`
/// when `$TMPDIR` is unset or the path would not fit `sun_path`.
pub fn socket_path(tmpdir: Option<&Path>, uid: u32) -> PathBuf {
    let name = format!("frostpane-{uid}.sock");
    match tmpdir.map(|t| t.join(&name)) {
        Some(p) if p.as_os_str().len() <= SUN_PATH_MAX => p,
        _ => Path::new("/tmp").join(name),
    }
}

/// Line a second launch sends to the first one.
pub const OPEN_LINE: &[u8] = b"open\n";

/// A received message asks to open the window.
pub fn is_open(msg: &[u8]) -> bool {
    msg.split(|b| *b == b'\n').any(|l| l.trim_ascii() == b"open")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(key: &str, frame: CocoaRect, visible: CocoaRect, backing: f64) -> RawScreen {
        RawScreen { key: key.into(), name: "Built-in".into(), frame, visible, backing }
    }

    #[test]
    fn levels() {
        assert_eq!(DESKTOP_ICON_LEVEL, -2_147_483_608);
        const { assert!(BEHIND_LEVEL < DESKTOP_SPRITE_LEVEL && DESKTOP_SPRITE_LEVEL < NORMAL_LEVEL) };
        const { assert!(UNDER_DOCK_LEVEL < DOCK_LEVEL && DOCK_LEVEL < PANEL_LEVEL && PANEL_LEVEL < FRONT_LEVEL) };
        assert!(dock_above(UNDER_DOCK_LEVEL) && dock_above(DESKTOP_SPRITE_LEVEL));
        assert!(!dock_above(FRONT_LEVEL) && !dock_above(DOCK_LEVEL));
        assert_eq!(WINDOW_CORNER, 10.0);
    }

    #[test]
    fn space_and_monitors() {
        // Retina primary 1440x900 with the Dock at the bottom (70 pt) and the
        // menu bar (25 pt); a 1x screen to the left, raised by 100 pt.
        let primary = screen(
            "610-a050-0",
            CocoaRect::new(0.0, 0.0, 1440.0, 900.0),
            CocoaRect::new(0.0, 70.0, 1440.0, 805.0),
            2.0,
        );
        let left = RawScreen {
            name: String::new(),
            ..screen(
                "",
                CocoaRect::new(-1920.0, 100.0, 1920.0, 1080.0),
                CocoaRect::new(-1920.0, 100.0, 1920.0, 1055.0),
                1.0,
            )
        };
        let (mons, sp) = monitors(&[primary.clone(), left.clone()]);
        assert_eq!(sp, Space { scale: 2.0, primary_h: 900.0 });
        assert_eq!(mons.len(), 2);
        assert!(mons[0].primary && !mons[1].primary);
        assert_eq!(mons[0].id, "610-a050-0");
        assert_eq!(mons[0].pixels, PixelRect::new(0, 0, 2880, 1800));
        assert_eq!(mons[0].work, PixelRect::new(0, 50, 2880, 1660));
        assert_eq!(mons[0].scale, 2.0);
        assert_eq!(mons[1].id, "screen-2");
        assert_eq!(mons[1].name, "Screen 2");
        assert_eq!(mons[1].pixels, PixelRect::new(-3840, -560, 0, 1600));
        assert_eq!(mons[1].scale, 2.0);
        // Dock strip on the primary only.
        assert_eq!(dock_shelf(&primary, sp), Some(PixelRect::new(0, 1660, 2880, 1800)));
        assert_eq!(dock_shelf(&left, sp), None);
        assert_eq!(shelves(&[primary.clone(), left], sp).len(), 1);
        // Primary sorts first even when another screen is further left;
        // duplicate keys and names are made unique.
        let twin = screen("k", CocoaRect::new(1440.0, 0.0, 100.0, 100.0), CocoaRect::default(), 1.0);
        let (m, _) = monitors(&[RawScreen { key: "k".into(), ..primary }, twin.clone(), twin]);
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["k", "k#2", "k#3"]);
        assert_eq!(m[2].name, "Built-in 3");
        assert_eq!(monitors(&[]).1, Space { scale: 1.0, primary_h: 0.0 });
        assert_eq!(space(&[]), Space::default());
    }

    #[test]
    fn conversions() {
        let sp = Space { scale: 2.0, primary_h: 900.0 };
        assert_eq!(sp.cocoa_px(100.0, 900.0), (200, 0));
        assert_eq!(sp.cocoa_px(0.25, 0.0), (0, 1800));
        assert_eq!(sp.quartz_px(10.6, -5.0), (21, -10));
        assert_eq!(sp.quartz_px(f64::NAN, 1e12), (0, i32::MAX));
        assert_eq!(sp.px_cocoa(200, 1800), (100.0, 0.0));
        assert_eq!(sp.px_quartz(200, 30), (100.0, 15.0));
        let r = sp.rect_cocoa(PixelRect::new(0, 1700, 200, 1800));
        assert_eq!(r, CocoaRect::new(0.0, 0.0, 100.0, 50.0));
        assert_eq!(sprite_frame(20, 0, 40, 60, sp), CocoaRect::new(10.0, 870.0, 20.0, 30.0));
        assert_eq!(sprite_frame(0, 0, u32::MAX, 2, sp).w, f64::from(i32::MAX) / 2.0);
        let bad = Space { scale: 0.0, primary_h: 10.0 };
        assert_eq!(bad.quartz_px(3.0, 4.0), (3, 4));
        assert_eq!(bad.px_quartz(3, 4), (3.0, 4.0));
        assert_eq!(bad.rect_cocoa(PixelRect::new(0, 0, 4, 4)).w, 4.0);
        assert_eq!(buttons(0b11), (true, true));
        assert_eq!(buttons(0b10), (false, true));
        assert_eq!(buttons(0b100), (false, false));
    }

    #[test]
    fn display_keys() {
        assert_eq!(display_key(0x610, 0xa050, 0, 1), "610-a050-0");
        assert_eq!(display_key(0, 0, 0, 69_734_208), "display-69734208");
    }

    struct Dict {
        nums: Vec<(Key, f64)>,
        owner: Option<&'static str>,
        bounds: Option<[f64; 4]>,
    }

    impl WindowDict for Dict {
        fn number(&self, k: Key) -> Option<f64> {
            self.nums.iter().find(|(n, _)| *n == k).map(|(_, v)| *v)
        }
        fn text(&self, k: Key) -> Option<String> {
            (k == Key::OwnerName).then_some(self.owner).flatten().map(String::from)
        }
        fn bounds(&self) -> Option<[f64; 4]> {
            self.bounds
        }
    }

    fn win(id: i64, layer: i64, pid: i64, b: [f64; 4]) -> RawWindow {
        RawWindow { id, layer, bounds: Some(b), alpha: 1.0, pid, owner: "App".into() }
    }

    #[test]
    fn parse_entries() {
        let d = Dict {
            nums: vec![(Key::Number, 42.0), (Key::Layer, 3.0), (Key::Alpha, 0.5), (Key::OwnerPid, 77.0)],
            owner: Some("Safari"),
            bounds: Some([10.0, 20.0, 300.0, 200.0]),
        };
        let w = raw_window(&d).unwrap();
        assert_eq!(
            w,
            RawWindow {
                id: 42,
                layer: 3,
                bounds: Some([10.0, 20.0, 300.0, 200.0]),
                alpha: 0.5,
                pid: 77,
                owner: "Safari".into()
            }
        );
        let bare = Dict {
            nums: vec![(Key::Number, 1.0), (Key::Alpha, f64::NAN)],
            owner: None,
            bounds: Some([0.0, 0.0, -1.0, 1.0]),
        };
        let w = raw_window(&bare).unwrap();
        assert_eq!((w.layer, w.pid, w.alpha, w.bounds, w.owner.as_str()), (0, 0, 1.0, None, ""));
        let inf = Dict {
            nums: vec![(Key::Number, 1.0), (Key::Alpha, 3.0)],
            owner: None,
            bounds: Some([f64::INFINITY, 0.0, 1.0, 1.0]),
        };
        let w = raw_window(&inf).unwrap();
        assert_eq!((w.alpha, w.bounds), (1.0, None));
        assert!(raw_window(&Dict { nums: vec![(Key::Number, f64::NAN)], owner: None, bounds: None }).is_none());
        assert!(raw_window(&Dict { nums: vec![], owner: None, bounds: None }).is_none());
    }

    #[test]
    fn portable_mapping() {
        let sp = Space { scale: 2.0, primary_h: 900.0 };
        let normal = win(1, 0, 10, [10.0, 20.0, 300.0, 200.0]);
        let p = portable(&normal, 99, sp).unwrap();
        assert_eq!(p.bounds, Some(PixelRect::new(20, 40, 620, 440)));
        assert!(!p.topmost && !p.own_process && !p.cloaked && !p.iconic && p.alpha.is_none());
        assert_eq!(p.owner, "App");
        let floating = RawWindow { layer: 3, alpha: 0.5, pid: 99, ..normal.clone() };
        let p = portable(&floating, 99, sp).unwrap();
        assert!(p.topmost && p.own_process);
        assert_eq!(p.alpha, Some(128));
        for layer in [DOCK_LEVEL, PANEL_LEVEL, 1000, -1, DESKTOP_ICON_LEVEL] {
            assert!(portable(&RawWindow { layer, ..normal.clone() }, 99, sp).is_none(), "{layer}");
        }
        assert!(portable(&RawWindow { owner: "Window Server".into(), ..normal.clone() }, 99, sp).is_none());
        assert!(portable(&RawWindow { owner: "Dock".into(), ..normal.clone() }, 99, sp).is_none());
        assert_eq!(portable(&RawWindow { bounds: None, ..normal }, 99, sp).unwrap().bounds, None);
        let c = cloaked(5);
        assert!(c.cloaked && c.id == 5 && c.bounds.is_none());
        // The tracker sees the portable window as a normal app window.
        let info = fp_tracker::WindowInfo::portable(portable(&win(2, 0, 1, [0.0, 0.0, 500.0, 400.0]), 9, sp).unwrap());
        assert!(fp_tracker::filter(&info).is_some());
    }

    #[test]
    fn fullscreen_and_cover() {
        let sp = Space { scale: 1.0, primary_h: 900.0 };
        let screens = [PixelRect::new(0, 0, 1440, 900), PixelRect::new(1440, 0, 3360, 1080)];
        let full = win(1, 0, 5, [0.0, 0.0, 1440.0, 900.0]);
        let small = win(2, 0, 5, [1500.0, 100.0, 300.0, 300.0]);
        let big_behind = win(3, 0, 5, [1440.0, 0.0, 1920.0, 1080.0]);
        let list = [win(9, 25, 5, [0.0, 0.0, 1440.0, 24.0]), full.clone(), small, big_behind.clone()];
        assert_eq!(fullscreen(&list, 7, &screens, sp), vec![PixelRect::new(0, 0, 1440, 900)]);
        // Own windows and transparent ones never count.
        assert!(fullscreen(std::slice::from_ref(&full), 5, &screens, sp).is_empty());
        assert!(fullscreen(&[RawWindow { alpha: 0.0, ..full.clone() }], 7, &screens, sp).is_empty());
        // One window spanning both screens is reported once.
        let span = win(4, 0, 5, [0.0, 0.0, 3360.0, 1080.0]);
        assert_eq!(fullscreen(&[span], 7, &screens, sp).len(), 1);
        assert_eq!(fullscreen(&[big_behind], 7, &screens, sp), vec![PixelRect::new(1440, 0, 3360, 1080)]);

        let above = [
            win(1, 0, 7, [0.0, 0.0, 100.0, 100.0]),
            RawWindow { alpha: 0.0, ..win(2, 0, 5, [0.0, 0.0, 100.0, 100.0]) },
            RawWindow { owner: "Window Server".into(), ..win(3, 0, 5, [0.0, 0.0, 100.0, 100.0]) },
            RawWindow { bounds: None, ..win(4, 0, 5, [0.0; 4]) },
        ];
        assert!(!covered(&above, 7, (50.0, 50.0)));
        let foreign = win(5, 0, 5, [0.0, 0.0, 100.0, 100.0]);
        assert!(covered(std::slice::from_ref(&foreign), 7, (0.0, 99.5)));
        assert!(!covered(std::slice::from_ref(&foreign), 7, (100.0, 50.0)));
        assert!(!covered(&[foreign], 7, (50.0, -1.0)));

        let seen = own_seen(&[win(1, 26, 7, [0.0; 4]), win(2, 0, 5, [0.0; 4])], 7);
        assert_eq!(seen[0], fp_platform::overlay::Seen { hwnd: 1, topmost: true, visible: true, own_thread: true });
        assert!(!seen[1].topmost && !seen[1].own_thread);
    }

    #[test]
    fn events() {
        let all: Vec<&str> = WORKSPACE_EVENTS.iter().chain(&DISTRIBUTED_EVENTS).chain(&APP_EVENTS).copied().collect();
        for n in &all {
            assert!(system_event(n).is_some(), "{n}");
        }
        assert_eq!(system_event("NSWorkspaceWillSleepNotification"), Some(SystemEvent::Suspend));
        assert_eq!(system_event("NSWorkspaceDidWakeNotification"), Some(SystemEvent::Resume));
        assert_eq!(system_event("NSWorkspaceScreensDidSleepNotification"), Some(SystemEvent::DisplayOff));
        assert_eq!(system_event("NSWorkspaceScreensDidWakeNotification"), Some(SystemEvent::DisplayOn));
        assert_eq!(system_event("com.apple.screenIsLocked"), Some(SystemEvent::Locked));
        assert_eq!(system_event("NSWorkspaceSessionDidBecomeActiveNotification"), Some(SystemEvent::Unlocked));
        assert_eq!(system_event("AppleInterfaceThemeChangedNotification"), Some(SystemEvent::ThemeChanged));
        assert_eq!(system_event("NSProcessInfoPowerStateDidChangeNotification"), Some(SystemEvent::PowerChanged));
        assert_eq!(system_event("other"), None);
    }

    #[test]
    fn services() {
        assert_eq!(lang_code("en-US").as_deref(), Some("en"));
        assert_eq!(lang_code("pt_BR").as_deref(), Some("pt"));
        assert_eq!(lang_code("DE").as_deref(), Some("de"));
        assert_eq!(lang_code("zh-Hans-CN").as_deref(), Some("zh"));
        assert_eq!(lang_code("tr_TR.UTF-8").as_deref(), Some("tr"));
        assert_eq!(lang_code("yue-HK"), None);
        assert_eq!(lang_code("C"), None);
        assert_eq!(lang_code("1a"), None);
        assert!(url_allowed(" HTTPS://example.org"));
        assert!(url_allowed(LOGIN_ITEMS_URL));
        assert!(!url_allowed("file:///etc"));
        assert!(!url_allowed("ms-settings:startupapps"));
        assert_eq!(
            power_from(Some("Battery Power"), false, false),
            PowerStatus { on_battery: true, has_battery: true, saving: true }
        );
        assert_eq!(
            power_from(Some("AC Power"), true, false),
            PowerStatus { on_battery: false, has_battery: true, saving: false }
        );
        assert_eq!(power_from(None, false, true), PowerStatus { on_battery: false, has_battery: false, saving: true });
        assert!(light_theme(None));
        assert!(!light_theme(Some("Dark")));
        assert!(light_theme(Some("Light")));
        use std::time::Duration;
        assert_eq!(uptime_ms(Duration::from_secs(100), Duration::from_millis(100_250)), 250);
        assert_eq!(uptime_ms(Duration::from_secs(100), Duration::from_secs(1)), 0);
        assert_eq!(login_state(1), (true, false));
        assert_eq!(login_state(2), (false, true));
        assert_eq!(login_state(0), (false, false));
        assert_eq!(login_state(3), (false, false));
        assert!(in_app_bundle(Path::new("/Applications/Frostpane.app/Contents/MacOS/frostpane")));
        assert!(!in_app_bundle(Path::new("/usr/local/bin/frostpane")));
    }

    #[test]
    fn launch_agent() {
        let p = agent_path(Path::new("/Users/u"));
        assert!(p.ends_with("Library/LaunchAgents/io.github.duruer.frostpane.plist"));
        let text = agent_plist(Path::new("/opt/a&b/<frost>\"pane\""));
        assert!(text.contains("<string>/opt/a&amp;b/&lt;frost&gt;&quot;pane&quot;</string>"));
        assert!(text.contains("<string>--quiet</string>"));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(text.contains(AGENT_LABEL));
    }

    #[test]
    fn instance_socket() {
        assert_eq!(
            socket_path(Some(Path::new("/var/folders/x/T")), 501),
            PathBuf::from("/var/folders/x/T/frostpane-501.sock")
        );
        assert_eq!(socket_path(None, 501), PathBuf::from("/tmp/frostpane-501.sock"));
        let long = "/x".repeat(60);
        assert_eq!(socket_path(Some(Path::new(&long)), 7), PathBuf::from("/tmp/frostpane-7.sock"));
        assert!(is_open(OPEN_LINE));
        assert!(is_open(b"ping\n open \n"));
        assert!(!is_open(b"opened"));
        assert!(!is_open(b""));
    }
}
