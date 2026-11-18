//! Window-tracking *policy*, free of OS calls.
//!
//! Spec: desktop-native 3.4 (which windows to re-raise; decisions only),
//! 3.5.1 (hook event effects), 3.5.2 (adaptive poll cadence), 3.5.3 (filter
//! predicate over plain window attributes), 3.5.4 (fading/leaving),
//! 3.5.5 (per-poll surfaces, hidden, ground, dock), 3.5.6 (taskbar shelves),
//! 3.6 (screen selection by `screensOff`, main screen frame),
//! 3.7 (asleep and battery-saving rules), 3.10 (decoration hover candidate),
//! 3.11 (outside-click edge detection); weather-sources 3.
//!
//! `fp-win` gathers [`WindowInfo`] from the OS and executes the decisions.

pub mod synth;

use std::collections::{BTreeMap, BTreeSet};

use fp_core::desktop::{Layout, Screen};
use fp_core::geom::{Frame, PixelRect, Rect4};
use fp_core::settings::Settings;
use fp_core::surface::{ScreenSnap, Surface, SurfaceId};

const WS_CAPTION: u32 = 0x00C0_0000;
const EX_TOPMOST: u32 = 0x0000_0008;
const EX_TRANSPARENT: u32 = 0x0000_0020;
const EX_TOOLWINDOW: u32 = 0x0000_0080;
const EX_APPWINDOW: u32 = 0x0004_0000;
const EX_LAYERED: u32 = 0x0008_0000;
const EX_NOACTIVATE: u32 = 0x0800_0000;

/// Shell window classes that never count as windows (3.5.3 rule 7).
const SHELL_CLASSES: [&str; 7] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
    "#32768",
];

const LEAVING_TIME: f64 = 1.0;
const SPACE_HOLD: f64 = 1.5;
const FAST: f64 = 1.0 / 60.0;
const CALM: f64 = 0.2;
const CALM_SAVING: f64 = 0.5;
const ALERT_HOLD: f64 = 0.5;
const MOVE_HOLD: f64 = 1.0;
const RUN_THRESHOLD: f64 = 0.9;
const INTERVAL_CAP: f64 = 0.25;
const HEADROOM: f64 = 30.0;
const MIN_SURFACE_W: f64 = 80.0;
const MIN_SURFACE_H: f64 = 40.0;
const MIN_LISTED_ALPHA: f64 = 0.3;
const FADING_ALPHA: f64 = 0.98;
const FADING_DROP: f64 = 0.01;
const CLOAKS_IN_A_SWITCH: usize = 2;
/// Bounds beyond this many pixels from the origin are treated as a failed
/// bounds query, so pixel arithmetic downstream can never overflow `i32`.
const MAX_COORD: i32 = 1 << 24;

/// Raw facts about one top-level window, in EnumWindows z-order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowInfo {
    pub hwnd: i64,
    pub visible: bool,
    pub iconic: bool,
    pub cloaked: bool,
    pub style: u32,
    pub ex_style: u32,
    pub own_process: bool,
    pub class: String,
    /// `Some(alpha)` when layered with LWA_ALPHA.
    pub layered_alpha: Option<u8>,
    /// DWM extended frame bounds; `None` when the query failed.
    pub bounds: Option<PixelRect>,
    pub process_name: String,
}

/// Class name given to [`WindowInfo::portable`] windows; never a shell class.
pub const PORTABLE_CLASS: &str = "portable";

/// What a non-Windows backend knows about one top-level window (porting
/// design 1.6). [`WindowInfo::portable`] turns it into Win32-flavoured bits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PortableWindow {
    pub id: i64,
    /// Frame in pixels; `None` when the query failed.
    pub bounds: Option<PixelRect>,
    /// Minimized / hidden.
    pub iconic: bool,
    /// On another desktop or space.
    pub cloaked: bool,
    /// Kept above normal windows (occluder only).
    pub topmost: bool,
    pub own_process: bool,
    /// Window opacity; `None` = opaque.
    pub alpha: Option<u8>,
    pub owner: String,
}

impl WindowInfo {
    /// A visible app window with a caption, so [`filter`] treats it like a
    /// normal (or, with `topmost`, an occluder) Windows window: `EX_TOPMOST`
    /// when topmost, `EX_LAYERED` with `layered_alpha` when an alpha is set.
    /// Own-process windows follow the Windows rule: listed unless topmost.
    pub fn portable(p: PortableWindow) -> Self {
        let mut ex_style = 0;
        if p.topmost {
            ex_style |= EX_TOPMOST;
        }
        if p.alpha.is_some() {
            ex_style |= EX_LAYERED;
        }
        WindowInfo {
            hwnd: p.id,
            visible: true,
            iconic: p.iconic,
            cloaked: p.cloaked,
            style: WS_CAPTION,
            ex_style,
            own_process: p.own_process,
            class: PORTABLE_CLASS.into(),
            layered_alpha: p.alpha,
            bounds: p.bounds,
            process_name: p.owner,
        }
    }
}

/// A window that passed the filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub id: i64,
    pub bounds: PixelRect,
    /// 1 = topmost (occluder only), 0 = normal.
    pub layer: u8,
    pub alpha: f64,
    pub owner: String,
}

/// desktop-native 3.5.3 filter. `None` = skip.
pub fn filter(w: &WindowInfo) -> Option<Listed> {
    if !w.visible || w.iconic || w.cloaked {
        return None;
    }
    let ex = w.ex_style;
    if ex & EX_TOOLWINDOW != 0 && ex & EX_APPWINDOW == 0 {
        return None;
    }
    if ex & EX_NOACTIVATE != 0 {
        return None;
    }
    if ex & EX_LAYERED != 0 && ex & EX_TRANSPARENT != 0 {
        return None;
    }
    let topmost = ex & EX_TOPMOST != 0;
    if w.own_process && (topmost || w.style & WS_CAPTION != WS_CAPTION) {
        return None;
    }
    if SHELL_CLASSES.contains(&w.class.as_str()) {
        return None;
    }
    let alpha = w.layered_alpha.map_or(1.0, |a| f64::from(a) / 255.0);
    if alpha <= MIN_LISTED_ALPHA {
        return None;
    }
    let bounds = w.bounds?;
    let sane = [bounds.left, bounds.top, bounds.right, bounds.bottom]
        .iter()
        .all(|c| c.unsigned_abs() <= MAX_COORD.unsigned_abs());
    if !sane || bounds.width() <= 0 || bounds.height() <= 0 {
        return None;
    }
    Some(Listed { id: w.hwnd, bounds, layer: u8::from(topmost), alpha, owner: w.process_name.clone() })
}

/// WinEvent hook callbacks mapped to tracker input (3.5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    Foreground,
    MoveSizeStart,
    MoveSizeEnd,
    MinimizeStart(i64),
    Destroy(i64),
    Hide(i64),
}

/// Output of one poll.
#[derive(Clone, Debug, PartialEq)]
pub struct PollResult {
    /// One per selected screen, same order as the input screens.
    pub screens: Vec<ScreenSnap>,
    /// Interval passed to the simulations (`min(now - polledAt, 0.25)`).
    pub interval: f64,
    pub to_other_space: bool,
}

/// Everything one listing needs.
pub struct PollInput<'a> {
    /// Filtered windows, z top to bottom.
    pub listed: Vec<Listed>,
    /// Windows from the previous listing that are now cloaked (virtual desktop switch when >= 2).
    pub cloaked_known: usize,
    /// Global frames of the screens that have an overlay.
    pub screens: &'a [Frame],
    /// Taskbar rects in pixels.
    pub shelves: &'a [PixelRect],
    pub layout: &'a Layout,
    /// OS window corner radius (8 on Windows 11, else 0).
    pub corner: f64,
}

/// Tracker state that survives overlay rebuilds.
#[derive(Debug)]
pub struct Tracker {
    known: BTreeSet<i64>,
    leaving: BTreeMap<i64, f64>,
    alphas: BTreeMap<i64, f64>,
    moves: BTreeMap<i64, PixelRect>,
    dragging: bool,
    polled_at: f64,
    moved_at: f64,
    space_changed_at: f64,
    alert_at: f64,
}

impl Default for Tracker {
    fn default() -> Self {
        Self {
            known: BTreeSet::new(),
            leaving: BTreeMap::new(),
            alphas: BTreeMap::new(),
            moves: BTreeMap::new(),
            dragging: false,
            polled_at: 0.0,
            moved_at: -10.0,
            space_changed_at: -10.0,
            alert_at: -10.0,
        }
    }
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_event(&mut self, ev: HookEvent, now: f64) {
        match ev {
            HookEvent::Foreground => self.alert_at = now,
            HookEvent::MoveSizeStart => self.dragging = true,
            HookEvent::MoveSizeEnd => {
                self.dragging = false;
                self.alert_at = now;
            }
            HookEvent::MinimizeStart(h) | HookEvent::Destroy(h) | HookEvent::Hide(h) => {
                if self.known.contains(&h) {
                    self.leaving.insert(h, now + LEAVING_TIME);
                    self.alert_at = now;
                }
            }
        }
    }

    fn interval(&self, now: f64, saving: bool) -> f64 {
        let fast = self.dragging
            || now - self.alert_at < ALERT_HOLD
            || !self.leaving.is_empty()
            || now - self.moved_at < MOVE_HOLD;
        if fast {
            FAST
        } else if saving {
            CALM_SAVING
        } else {
            CALM
        }
    }

    /// Should this tick list windows? (3.5.2 cadence; `saving` = battery saver.)
    pub fn due(&self, now: f64, force: bool, saving: bool) -> bool {
        force || now - self.polled_at >= self.interval(now, saving) * RUN_THRESHOLD
    }

    fn note_fading(&mut self, now: f64, listed: &[Listed]) {
        self.leaving.retain(|_, expiry| *expiry > now);
        let mut alphas = BTreeMap::new();
        for w in listed.iter().filter(|w| w.layer == 0) {
            let prev = self.alphas.get(&w.id).copied().unwrap_or(1.0);
            if w.alpha < FADING_ALPHA && w.alpha < prev - FADING_DROP {
                self.leaving.insert(w.id, now + LEAVING_TIME);
            }
            alphas.insert(w.id, w.alpha);
        }
        self.alphas = alphas;
    }

    /// Feed one listing and compute per-screen surfaces.
    pub fn poll(&mut self, now: f64, input: PollInput<'_>) -> PollResult {
        // NaN and a clock that went backwards give 0.
        let since = now - self.polled_at;
        let interval = if since > 0.0 { since.min(INTERVAL_CAP) } else { 0.0 };
        self.polled_at = now;
        if input.cloaked_known >= CLOAKS_IN_A_SWITCH {
            self.space_changed_at = now;
        }
        self.note_fading(now, &input.listed);
        self.known = input.listed.iter().map(|w| w.id).collect();
        let windows: Vec<&Listed> = input.listed.iter().filter(|w| !self.leaving.contains_key(&w.id)).collect();

        let mut moves = BTreeMap::new();
        let mut moved = false;
        for w in windows.iter().filter(|w| w.layer == 0) {
            if self.moves.get(&w.id).is_some_and(|old| *old != w.bounds) {
                moved = true;
            }
            moves.insert(w.id, w.bounds);
        }
        if moved {
            self.moved_at = now;
        }
        self.moves = moves;

        let to_other_space = now - self.space_changed_at < SPACE_HOLD;
        let screens = input.screens.iter().map(|f| snap_screen(*f, &windows, &input)).collect();
        PollResult { screens, interval, to_other_space }
    }

    /// Ids from the last listing (for the cloak count and hook filtering).
    pub fn known(&self, id: i64) -> bool {
        self.known.contains(&id)
    }

    /// Forget per-rebuild state (move map, drag flag), keep the rest (3.6).
    pub fn rebuilt(&mut self) {
        self.moves.clear();
        self.dragging = false;
    }
}

fn find_screen<'a>(layout: &'a Layout, frame: &Frame) -> Option<&'a Screen> {
    layout.screens().iter().find(|s| s.points == *frame).or_else(|| layout.at_point(frame.center()))
}

fn hits(higher: &[Rect4], area: &Frame) -> Vec<Rect4> {
    higher.iter().copied().filter(|h| h.to_frame().intersects(area)).collect()
}

fn snap_screen(frame: Frame, windows: &[&Listed], input: &PollInput<'_>) -> ScreenSnap {
    let Some(screen) = find_screen(input.layout, &frame) else {
        return ScreenSnap { frame, shown: true, ground: true, surfaces: Vec::new() };
    };
    let (sw, sh) = (frame.w, frame.h);
    let local_screen = Frame::new(0.0, 0.0, sw, sh);
    let mut shown = true;
    let mut filled = false;
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut higher: Vec<Rect4> = Vec::new();
    for w in windows {
        let global = screen.map(w.bounds);
        if global.inset(-1.0, -1.0).contains_frame(&frame) {
            shown = false;
        }
        if w.layer == 0
            && global.min_x() <= frame.min_x() + 1.0
            && global.max_x() >= frame.max_x() - 1.0
            && global.min_y() <= frame.min_y() + 1.0
            && global.max_y() >= frame.max_y() - HEADROOM - 2.0
        {
            filled = true;
        }
        let local = global.offset(-frame.x, -frame.y);
        if !local.intersects(&local_screen) {
            continue;
        }
        let fullscreenish = local.w >= sw - 1.0 && local.h >= sh - HEADROOM;
        if w.layer == 0
            && !fullscreenish
            && local.max_y() <= sh - HEADROOM
            && local.w >= MIN_SURFACE_W
            && local.h >= MIN_SURFACE_H
        {
            let r = input.corner.max(0.0).min(local.w / 2.0).min(local.h / 2.0);
            let reach = Frame::new(local.x - 1.0, local.max_y() - r - 1.0, local.w + 2.0, r + HEADROOM);
            let above = Frame::new(local.x - 1.0, local.max_y() - r - 1.0, local.w + 2.0, sh);
            let mut s = Surface::new(SurfaceId::Window(w.id), local);
            s.blockers = hits(&higher, &reach);
            s.covers = hits(&higher, &above);
            s.corner_radius = r;
            s.owner = w.owner.clone();
            surfaces.push(s);
        }
        higher.push(local.to_rect());
    }
    let ground = !filled;
    if ground {
        let dock = input.shelves.iter().map(|p| screen.local(*p)).find(|l| l.intersects(&local_screen));
        if let Some(l) = dock {
            surfaces.push(Surface::new(SurfaceId::Dock, l));
        }
    }
    ScreenSnap { frame, shown, ground, surfaces }
}

/// Indices of `layout` screens that get an overlay (3.6: `screensOff`,
/// empty result -> all) and the main screen frame if any.
pub fn select_screens(layout: &Layout, settings: &Settings) -> (Vec<usize>, Option<Frame>) {
    let all = layout.screens();
    let mut picked: Vec<usize> =
        all.iter().enumerate().filter(|(_, s)| settings.screen_on(&s.info.id)).map(|(i, _)| i).collect();
    if picked.is_empty() {
        picked = (0..all.len()).collect();
    }
    let main = settings
        .main_screen
        .as_deref()
        .and_then(|id| picked.iter().map(|&i| &all[i]).find(|s| s.info.id == id))
        .map(|s| s.points);
    (picked, main)
}

/// Session/power state (3.7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Power {
    pub locked: bool,
    pub suspended: bool,
    pub display_off: bool,
}

impl Power {
    pub fn asleep(&self) -> bool {
        self.locked || self.suspended || self.display_off
    }
}

/// Battery saver rule from `GetSystemPowerStatus` fields (3.7).
pub fn saving_power(ac_line_status: u8, system_status_flag: u8) -> bool {
    ac_line_status == 0 || system_status_flag == 1
}

/// One window the host may re-raise, in z-order from the top (3.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopWindow {
    pub hwnd: i64,
    pub topmost: bool,
    /// One of the weather overlay panels.
    pub panel: bool,
    /// Deliberately kept under the taskbar.
    pub under_dock: bool,
    pub visible: bool,
    /// Owned by the UI thread (our own object windows).
    pub own_thread: bool,
}

/// Own topmost windows to re-apply TOPMOST to (3.4 `RaisePanels`), in the
/// order to apply them (bottom-most first, so relative order is preserved).
/// `top_down` is the z-order from the top; the walk ends at the first
/// non-topmost window.
pub fn raise_list(top_down: &[TopWindow]) -> Vec<i64> {
    let mut out: Vec<i64> = top_down
        .iter()
        .take_while(|w| w.topmost)
        .filter(|w| !w.panel && !w.under_dock && w.visible && w.own_thread)
        .map(|w| w.hwnd)
        .collect();
    out.reverse();
    out
}

/// A decoration panel as the hover tracker sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverItem {
    pub id: i64,
    pub frame: PixelRect,
    pub dragging: bool,
}

/// A change of the hot (clickable) panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverChange {
    pub old: Option<i64>,
    pub new: Option<i64>,
}

/// Is the candidate covered by a foreign window? (3.10 covered test.)
/// `root` = root window under the cursor (0 = none); `panels` = decoration
/// panels; `above` = windows above the candidate in z-order.
pub fn is_covered(root: i64, candidate: i64, panels: &[i64], above: &[i64]) -> bool {
    if root == 0 || root == candidate || panels.contains(&root) {
        return false;
    }
    above.contains(&root)
}

/// Decoration hover state machine (3.10 `Track`).
#[derive(Debug, Default)]
pub struct Hover {
    hot: Option<i64>,
}

impl Hover {
    pub fn new() -> Self {
        Self::default()
    }

    /// The panel that currently accepts the mouse.
    pub fn hot(&self) -> Option<i64> {
        self.hot
    }

    /// One `Track()` run. `items` is in z-order, topmost last. `solid(id, x,
    /// y)` is the artwork alpha test, `covered(id)` the covered test. Skipped
    /// (no change) while any panel is dragged.
    pub fn track(
        &mut self,
        cursor: (f64, f64),
        items: &[HoverItem],
        solid: &dyn Fn(i64, f64, f64) -> bool,
        covered: &dyn Fn(i64) -> bool,
    ) -> Option<HoverChange> {
        if items.iter().any(|i| i.dragging) {
            return None;
        }
        let (x, y) = cursor;
        let keep = self.hot.filter(|h| items.iter().any(|i| i.id == *h && i.frame.contains(x, y)));
        let candidate =
            keep.or_else(|| items.iter().rev().find(|i| i.frame.contains(x, y) && solid(i.id, x, y)).map(|i| i.id));
        let candidate = candidate.filter(|c| !covered(*c));
        if candidate == self.hot {
            return None;
        }
        let old = self.hot;
        self.hot = candidate;
        Some(HoverChange { old, new: candidate })
    }
}

/// Outside-click detector (3.11): rising edge of left/right button.
/// One-shot: after it fires once (or after [`OutsideClick::stop`]) it never
/// fires again; the host stops its 15 ms timer when [`OutsideClick::active`]
/// turns false.
#[derive(Debug, Default)]
pub struct OutsideClick {
    was_down: bool,
    stopped: bool,
}

impl OutsideClick {
    /// Record the initial button state (a held button does not fire).
    pub fn new(buttons_down: bool) -> Self {
        Self { was_down: buttons_down, stopped: false }
    }

    /// Returns true on a rising edge that is not over an own window. An edge
    /// over an own window is consumed and the detector keeps running. The
    /// host passes `over_own_window = false` for "anywhere" detectors, and
    /// `true` when the cursor position could not be read (edge consumed).
    pub fn poll(&mut self, buttons_down: bool, over_own_window: bool) -> bool {
        let edge = buttons_down && !self.was_down;
        self.was_down = buttons_down;
        let fire = edge && !over_own_window && !self.stopped;
        if fire {
            self.stopped = true;
        }
        fire
    }

    /// Cancel the detector (3.11 `Stop()`).
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// False once fired or stopped.
    pub fn active(&self) -> bool {
        !self.stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::desktop::MonitorInfo;

    fn win(hwnd: i64) -> WindowInfo {
        WindowInfo {
            hwnd,
            visible: true,
            style: WS_CAPTION,
            class: "Notepad".into(),
            bounds: Some(PixelRect::new(0, 0, 100, 100)),
            process_name: "app.exe".into(),
            ..Default::default()
        }
    }

    #[test]
    fn filter_rules() {
        let l = filter(&win(1)).unwrap();
        assert_eq!((l.id, l.layer, l.alpha, l.owner.as_str()), (1, 0, 1.0, "app.exe"));
        let skip = |f: fn(&mut WindowInfo)| {
            let mut w = win(1);
            f(&mut w);
            assert!(filter(&w).is_none());
        };
        skip(|w| w.visible = false);
        skip(|w| w.iconic = true);
        skip(|w| w.cloaked = true);
        skip(|w| w.ex_style = EX_TOOLWINDOW);
        skip(|w| w.ex_style = EX_NOACTIVATE);
        skip(|w| w.ex_style = EX_LAYERED | EX_TRANSPARENT);
        skip(|w| {
            w.own_process = true;
            w.ex_style = EX_TOPMOST;
        });
        skip(|w| {
            w.own_process = true;
            w.style = 0x0080_0000;
        });
        skip(|w| w.class = "Progman".into());
        skip(|w| w.layered_alpha = Some(76));
        skip(|w| w.bounds = None);
        skip(|w| w.bounds = Some(PixelRect::new(0, 0, 0, 10)));
        skip(|w| w.bounds = Some(PixelRect::new(0, 0, 10, 0)));
        skip(|w| w.bounds = Some(PixelRect::new(i32::MIN, 0, i32::MAX, 10)));
        skip(|w| w.bounds = Some(PixelRect::new(0, 0, 10, MAX_COORD + 1)));
        let mut edge = win(9);
        edge.bounds = Some(PixelRect::new(-MAX_COORD, -MAX_COORD, MAX_COORD, MAX_COORD));
        assert!(filter(&edge).is_some());
        // just above the 0.3 threshold: 77/255 = 0.302 is kept
        let mut a = win(10);
        a.layered_alpha = Some(77);
        assert!(filter(&a).is_some());

        let mut w = win(2);
        w.ex_style = EX_TOOLWINDOW | EX_APPWINDOW | EX_TOPMOST | EX_LAYERED;
        w.layered_alpha = Some(255);
        w.own_process = true;
        assert!(filter(&w).is_none());
        w.own_process = false;
        let l = filter(&w).unwrap();
        assert_eq!((l.layer, l.alpha), (1, 1.0));
        w.layered_alpha = Some(153);
        assert!((filter(&w).unwrap().alpha - 0.6).abs() < 1e-9);
        let mut o = win(3);
        o.own_process = true;
        assert!(filter(&o).is_some());
    }

    #[test]
    fn portable_windows() {
        assert!(!SHELL_CLASSES.contains(&PORTABLE_CLASS));
        let base = PortableWindow {
            id: 4,
            bounds: Some(PixelRect::new(0, 0, 100, 100)),
            owner: "app".into(),
            ..Default::default()
        };
        let w = WindowInfo::portable(base.clone());
        assert_eq!((w.hwnd, w.visible, w.style, w.ex_style, w.layered_alpha), (4, true, WS_CAPTION, 0, None));
        let l = filter(&w).unwrap();
        assert_eq!((l.id, l.layer, l.alpha, l.owner.as_str()), (4, 0, 1.0, "app"));
        let with = |f: fn(&mut PortableWindow)| {
            let mut p = base.clone();
            f(&mut p);
            filter(&WindowInfo::portable(p))
        };
        let top = WindowInfo::portable(PortableWindow { topmost: true, alpha: Some(153), ..base.clone() });
        assert_eq!(top.ex_style, EX_TOPMOST | EX_LAYERED);
        let l = filter(&top).unwrap();
        assert_eq!(l.layer, 1);
        assert!((l.alpha - 0.6).abs() < 1e-9);
        assert!(with(|p| p.iconic = true).is_none());
        assert!(with(|p| p.cloaked = true).is_none());
        assert!(with(|p| p.bounds = None).is_none());
        assert!(with(|p| p.alpha = Some(76)).is_none());
        assert!(with(|p| p.alpha = Some(77)).is_some());
        assert!(with(|p| p.own_process = true).is_some());
        assert!(
            with(|p| {
                p.own_process = true;
                p.topmost = true;
            })
            .is_none()
        );
    }

    fn mon(id: &str, px: PixelRect, primary: bool) -> MonitorInfo {
        MonitorInfo {
            id: id.into(),
            name: id.into(),
            pixels: px,
            work: PixelRect::new(px.left, px.top, px.right, px.bottom - 40),
            scale: 1.0,
            primary,
        }
    }

    fn layout1() -> Layout {
        Layout::new(vec![mon("a", PixelRect::new(0, 0, 1000, 800), true)])
    }

    fn listed(id: i64, b: PixelRect) -> Listed {
        Listed { id, bounds: b, layer: 0, alpha: 1.0, owner: "o".into() }
    }

    fn run(t: &mut Tracker, now: f64, l: &Layout, w: Vec<Listed>, shelves: &[PixelRect]) -> PollResult {
        let frames: Vec<Frame> = l.screens().iter().map(|s| s.points).collect();
        t.poll(now, PollInput { listed: w, cloaked_known: 0, screens: &frames, shelves, layout: l, corner: 8.0 })
    }

    #[test]
    fn cadence() {
        let mut t = Tracker::new();
        assert!(!t.due(0.1, false, false));
        assert!(t.due(0.19, false, false));
        assert!(t.due(0.01, true, false));
        assert!(!t.due(0.3, false, true));
        assert!(t.due(0.45, false, true));
        t.on_event(HookEvent::Foreground, 5.0);
        assert!(t.due(5.0 + 0.016, false, false));
        t.on_event(HookEvent::MoveSizeStart, 6.0);
        assert!(t.due(100.0, false, false));
        t.on_event(HookEvent::MoveSizeEnd, 7.0);
        assert!(!t.dragging);
        assert!(t.due(7.1, false, false));
        t.polled_at = 20.0;
        assert!(!t.due(20.1, false, false));
    }

    #[test]
    fn hooks_and_leaving() {
        let l = layout1();
        let mut t = Tracker::new();
        t.on_event(HookEvent::Destroy(9), 1.0);
        assert!(t.leaving.is_empty());
        run(
            &mut t,
            2.0,
            &l,
            vec![listed(9, PixelRect::new(100, 100, 400, 400)), listed(8, PixelRect::new(0, 0, 90, 90))],
            &[],
        );
        assert!(t.known(9) && !t.known(5));
        t.on_event(HookEvent::Hide(9), 3.0);
        t.on_event(HookEvent::MinimizeStart(8), 3.0);
        assert_eq!(t.leaving.len(), 2);
        assert_eq!(t.leaving[&9], 4.0);
        assert_eq!(t.alert_at, 3.0);
        let r = run(&mut t, 3.1, &l, vec![listed(9, PixelRect::new(100, 100, 400, 400))], &[]);
        assert!(r.screens[0].surfaces.is_empty());
        // leaving non-empty keeps the fast cadence
        assert!(t.due(3.1 + FAST, false, false));
        assert!(!t.due(3.1 + FAST * 0.8, false, false));
        // a hook for an unknown window changes nothing
        t.on_event(HookEvent::Destroy(77), 3.5);
        assert!(!t.leaving.contains_key(&77));
        assert_eq!(t.alert_at, 3.0);
        // expired exactly at expiry (expiry <= now)
        let r = run(&mut t, 4.0, &l, vec![listed(9, PixelRect::new(100, 100, 400, 400))], &[]);
        assert_eq!(r.screens[0].surfaces.len(), 1);
        assert!(t.leaving.is_empty());
        t.rebuilt();
        t.on_event(HookEvent::MoveSizeStart, 5.0);
        t.rebuilt();
        assert!(!t.dragging);
    }

    #[test]
    fn fading() {
        let l = layout1();
        let mut t = Tracker::new();
        let mut w = listed(1, PixelRect::new(100, 100, 400, 400));
        w.alpha = 0.5;
        let r = run(&mut t, 1.0, &l, vec![w.clone()], &[]);
        assert!(r.screens[0].surfaces.is_empty());
        w.alpha = 0.5;
        // still leaving, not new drop; expires at 2.0
        run(&mut t, 1.5, &l, vec![w.clone()], &[]);
        assert_eq!(t.leaving[&1], 2.0);
        w.alpha = 1.0;
        let r = run(&mut t, 2.5, &l, vec![w.clone()], &[]);
        assert_eq!(r.screens[0].surfaces.len(), 1);
        let mut top = w;
        top.layer = 1;
        top.alpha = 0.1;
        run(&mut t, 3.0, &l, vec![top], &[]);
        assert!(t.leaving.is_empty());
    }

    #[test]
    fn interval_and_space() {
        let l = layout1();
        let mut t = Tracker::new();
        let r = run(&mut t, 5.0, &l, vec![], &[]);
        assert_eq!(r.interval, 0.25);
        assert!(!r.to_other_space);
        let r = run(&mut t, 5.1, &l, vec![], &[]);
        assert!((r.interval - 0.1).abs() < 1e-9);
        let frames = [l.screens()[0].points];
        let r = t.poll(
            6.0,
            PollInput { listed: vec![], cloaked_known: 2, screens: &frames, shelves: &[], layout: &l, corner: 0.0 },
        );
        assert!(r.to_other_space);
        // clock going backwards gives interval 0, never negative
        let r = run(&mut t, 5.5, &l, vec![], &[]);
        assert_eq!(r.interval, 0.0);
        let r = run(&mut t, 7.4, &l, vec![], &[]);
        assert!(r.to_other_space);
        let r = run(&mut t, 7.6, &l, vec![], &[]);
        assert!(!r.to_other_space);
        // a single cloaked known window is not a desktop switch
        let r = t.poll(
            20.0,
            PollInput { listed: vec![], cloaked_known: 1, screens: &frames, shelves: &[], layout: &l, corner: 0.0 },
        );
        assert!(!r.to_other_space);
    }

    #[test]
    fn scaled_second_screen() {
        // b at 200 %: points (500, 0, 1000, 800), Y up from the primary bottom.
        let l = Layout::new(vec![
            mon("a", PixelRect::new(0, 0, 1000, 800), true),
            MonitorInfo { scale: 2.0, ..mon("b", PixelRect::new(1000, 0, 3000, 1600), false) },
        ]);
        let mut t = Tracker::new();
        let w = listed(1, PixelRect::new(1200, 400, 1600, 1000));
        let shelf = [PixelRect::new(1000, 1520, 3000, 1600)];
        let r = run(&mut t, 1.0, &l, vec![w], &shelf);
        assert_eq!(r.screens[1].frame, Frame::new(500.0, 0.0, 1000.0, 800.0));
        // not on a: no surface there, and the b shelf does not intersect a
        assert!(r.screens[0].surfaces.is_empty());
        let s = &r.screens[1].surfaces;
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].frame, Frame::new(100.0, 300.0, 200.0, 300.0));
        assert_eq!(s[1].id, SurfaceId::Dock);
        assert_eq!(s[1].frame, Frame::new(0.0, 0.0, 1000.0, 40.0));
        // negative corner radius input clamps to 0
        let frames = [l.screens()[1].points];
        let r = t.poll(
            2.0,
            PollInput {
                listed: vec![listed(1, PixelRect::new(1200, 400, 1600, 1000))],
                cloaked_known: 0,
                screens: &frames,
                shelves: &[],
                layout: &l,
                corner: -5.0,
            },
        );
        assert_eq!(r.screens[0].surfaces[0].corner_radius, 0.0);
    }

    #[test]
    fn move_detection() {
        let l = layout1();
        let mut t = Tracker::new();
        let a = listed(1, PixelRect::new(100, 100, 400, 400));
        run(&mut t, 10.0, &l, vec![a.clone()], &[]);
        assert!(!t.due(10.05, false, false));
        let mut b = a.clone();
        b.bounds = PixelRect::new(110, 100, 410, 400);
        run(&mut t, 11.0, &l, vec![b.clone(), listed(2, PixelRect::new(0, 0, 50, 50))], &[]);
        assert!(t.due(11.02, false, false));
        run(&mut t, 13.0, &l, vec![b], &[]);
        assert!(!t.due(13.05, false, false));
    }

    #[test]
    fn surfaces_blockers_covers() {
        let l = layout1();
        let mut t = Tracker::new();
        let top = listed(1, PixelRect::new(150, 300, 350, 500));
        let low = listed(2, PixelRect::new(100, 400, 400, 700));
        let tiny = listed(3, PixelRect::new(600, 500, 650, 530));
        let mut topmost = listed(4, PixelRect::new(0, 0, 20, 20));
        topmost.layer = 1;
        let tall = listed(5, PixelRect::new(700, 0, 900, 100));
        let off = listed(6, PixelRect::new(2000, 0, 2100, 100));
        let full = listed(7, PixelRect::new(0, 0, 999, 775));
        let shelves = [PixelRect::new(0, 3000, 100, 3040), PixelRect::new(0, 760, 1000, 800)];
        let r = run(&mut t, 1.0, &l, vec![top, low, tiny, topmost, tall, off, full], &shelves);
        let s = &r.screens[0];
        assert!(s.shown && s.ground);
        let ids: Vec<_> = s.surfaces.iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![SurfaceId::Window(1), SurfaceId::Window(2), SurfaceId::Dock]);
        let top = &s.surfaces[0];
        assert_eq!(top.frame, Frame::new(150.0, 300.0, 200.0, 200.0));
        assert_eq!(top.corner_radius, 8.0);
        assert_eq!(top.owner, "o");
        assert!(top.blockers.is_empty() && top.covers.is_empty());
        let low = &s.surfaces[1];
        assert_eq!(low.frame, Frame::new(100.0, 100.0, 300.0, 300.0));
        assert_eq!(low.blockers, vec![Rect4::new(150.0, 300.0, 350.0, 500.0)]);
        assert_eq!(low.covers, vec![Rect4::new(150.0, 300.0, 350.0, 500.0)]);
        assert_eq!(low.corner_radius, 8.0);
        assert_eq!(s.surfaces[2].frame, Frame::new(0.0, 0.0, 1000.0, 40.0));
        assert_eq!(s.surfaces[2].corner_radius, 0.0);
    }

    #[test]
    fn hidden_and_filled() {
        let l = layout1();
        let mut t = Tracker::new();
        let r = run(&mut t, 1.0, &l, vec![listed(1, PixelRect::new(-1, -1, 1001, 801))], &[]);
        assert!(!r.screens[0].shown);
        assert!(!r.screens[0].ground);
        let r = run(
            &mut t,
            2.0,
            &l,
            vec![listed(1, PixelRect::new(0, 10, 1000, 799))],
            &[PixelRect::new(0, 760, 1000, 800)],
        );
        assert!(r.screens[0].shown && !r.screens[0].ground);
        assert!(r.screens[0].surfaces.is_empty());
        let mut tm = listed(1, PixelRect::new(0, 0, 1000, 800));
        tm.layer = 1;
        let r = run(&mut t, 3.0, &l, vec![tm], &[PixelRect::new(0, 760, 1000, 800)]);
        assert!(!r.screens[0].shown && r.screens[0].ground);
        assert_eq!(r.screens[0].surfaces.len(), 1);
    }

    #[test]
    fn screen_lookup_fallbacks() {
        let l = layout1();
        let mut t = Tracker::new();
        let odd = [Frame::new(5.0, 5.0, 1000.0, 800.0)];
        let r = t.poll(
            1.0,
            PollInput { listed: vec![], cloaked_known: 0, screens: &odd, shelves: &[], layout: &l, corner: 0.0 },
        );
        assert_eq!(r.screens[0].frame, odd[0]);
        assert!(r.screens[0].ground);
        let empty = Layout::default();
        let r = t.poll(
            2.0,
            PollInput { listed: vec![], cloaked_known: 0, screens: &odd, shelves: &[], layout: &empty, corner: 0.0 },
        );
        assert!(r.screens[0].shown && r.screens[0].ground && r.screens[0].surfaces.is_empty());
    }

    #[test]
    fn selection() {
        let l = Layout::new(vec![
            mon("a", PixelRect::new(0, 0, 1000, 800), true),
            mon("b", PixelRect::new(1000, 0, 2000, 800), false),
        ]);
        let mut s = Settings::default();
        assert_eq!(select_screens(&l, &s).0, vec![0, 1]);
        s.screens_off = vec!["a".into()];
        s.main_screen = Some("b".into());
        let (v, f) = select_screens(&l, &s);
        assert_eq!(v, vec![1]);
        assert_eq!(f, Some(l.screens()[1].points));
        s.main_screen = Some("a".into());
        assert_eq!(select_screens(&l, &s).1, None);
        s.screens_off = vec!["a".into(), "b".into()];
        assert_eq!(select_screens(&l, &s).0, vec![0, 1]);
        s.main_screen = None;
        assert_eq!(select_screens(&l, &s).1, None);
        assert_eq!(select_screens(&Layout::default(), &s), (vec![], None));
    }

    #[test]
    fn power_rules() {
        assert!(!Power::default().asleep());
        assert!(Power { locked: true, ..Default::default() }.asleep());
        assert!(Power { suspended: true, ..Default::default() }.asleep());
        assert!(Power { display_off: true, ..Default::default() }.asleep());
        assert!(saving_power(0, 0));
        assert!(saving_power(1, 1));
        assert!(saving_power(255, 1));
        assert!(!saving_power(1, 0));
        assert!(!saving_power(255, 0));
    }

    #[test]
    fn raise() {
        let w = |h, topmost, panel, under_dock, visible, own_thread| TopWindow {
            hwnd: h,
            topmost,
            panel,
            under_dock,
            visible,
            own_thread,
        };
        let list = [
            w(1, true, false, false, true, true),
            w(2, true, true, false, true, true),
            w(3, true, false, true, true, true),
            w(4, true, false, false, false, true),
            w(5, true, false, false, true, false),
            w(6, true, false, false, true, true),
            w(7, false, false, false, true, true),
            w(8, true, false, false, true, true),
        ];
        assert_eq!(raise_list(&list), vec![6, 1]);
        assert!(raise_list(&[]).is_empty());
    }

    #[test]
    fn covered() {
        assert!(!is_covered(0, 1, &[], &[5]));
        assert!(!is_covered(1, 1, &[], &[1]));
        assert!(!is_covered(2, 1, &[2], &[2]));
        assert!(is_covered(5, 1, &[2], &[4, 5]));
        assert!(!is_covered(6, 1, &[2], &[4, 5]));
    }

    #[test]
    fn hover_track() {
        let it = |id, l, dragging| HoverItem { id, frame: PixelRect::new(l, 0, l + 100, 100), dragging };
        let mut h = Hover::new();
        let solid = |id: i64, _x: f64, _y: f64| id != 2;
        let never = |_: i64| false;
        let items = [it(1, 0, false), it(2, 50, false), it(3, 80, false)];
        assert_eq!(h.hot(), None);
        // nothing under cursor
        assert_eq!(h.track((500.0, 5.0), &items, &solid, &never), None);
        // topmost solid wins (3 over 2 over 1)
        assert_eq!(h.track((90.0, 5.0), &items, &solid, &never), Some(HoverChange { old: None, new: Some(3) }));
        // hysteresis: stays hot while inside frame
        assert_eq!(h.track((85.0, 5.0), &items, &|_, _, _| false, &never), None);
        // leaves 3's frame; 2 not solid, 1 solid
        assert_eq!(h.track((60.0, 5.0), &items, &solid, &never), Some(HoverChange { old: Some(3), new: Some(1) }));
        // dragging skips
        let drag = [it(1, 0, true)];
        assert_eq!(h.track((500.0, 5.0), &drag, &solid, &never), None);
        assert_eq!(h.hot(), Some(1));
        // covered candidate dropped
        assert_eq!(h.track((10.0, 5.0), &items, &solid, &|c| c == 1), Some(HoverChange { old: Some(1), new: None }));
        // hot removed from list
        let mut h2 = Hover::new();
        h2.track((10.0, 5.0), &items, &solid, &never);
        assert_eq!(h2.track((10.0, 5.0), &[], &solid, &never), Some(HoverChange { old: Some(1), new: None }));
    }

    #[test]
    fn outside_click() {
        let mut c = OutsideClick::new(true);
        assert!(!c.poll(true, false));
        assert!(!c.poll(false, false));
        // edge over own window is consumed, detector keeps running
        assert!(!c.poll(true, true));
        assert!(c.active());
        assert!(!c.poll(false, false));
        assert!(c.poll(true, false));
        // one-shot
        assert!(!c.active());
        assert!(!c.poll(false, false));
        assert!(!c.poll(true, false));
        let mut d = OutsideClick::default();
        assert!(d.poll(true, false));
        let mut e = OutsideClick::new(false);
        e.stop();
        assert!(!e.active());
        assert!(!e.poll(true, false));
    }
}
