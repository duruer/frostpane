//! Platform-free parts of the X11 backend (porting design 3.2): EWMH
//! property parsing, frame-extent math, the client -> `WindowInfo` mapping,
//! struts -> work areas and taskbar shelves, monitor scale (winit's rule),
//! stacking-order decisions and sprite input regions. Compiled and tested on
//! every target; the X calls live in the Linux-only modules.

use fp_core::geom::PixelRect;
use fp_tracker::{HookEvent, PortableWindow, WindowInfo};

/// `_NET_WM_DESKTOP` value of a window shown on every desktop.
pub const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

/// Left, right, top, bottom margins (`_NET_FRAME_EXTENTS`, `_GTK_FRAME_EXTENTS`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extents {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

impl Extents {
    /// The first four CARDINALs, `None` when fewer are present.
    pub fn parse(v: &[u32]) -> Option<Extents> {
        match v {
            [l, r, t, b, ..] => Some(Extents { left: *l, right: *r, top: *t, bottom: *b }),
            _ => None,
        }
    }
}

fn clamp_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The visible frame of a client: its root-relative client rect grown by the
/// WM's decorations (`_NET_FRAME_EXTENTS`) and shrunk by client-side shadow
/// margins (`_GTK_FRAME_EXTENTS`, the DWM extended-frame-bounds equivalent).
/// `None` when nothing visible is left.
pub fn frame_rect(x: i32, y: i32, w: u32, h: u32, net: Option<Extents>, gtk: Option<Extents>) -> Option<PixelRect> {
    let n = net.unwrap_or_default();
    let g = gtk.unwrap_or_default();
    let (x, y, w, h) = (i64::from(x), i64::from(y), i64::from(w), i64::from(h));
    let left = x - i64::from(n.left) + i64::from(g.left);
    let top = y - i64::from(n.top) + i64::from(g.top);
    let right = x + w + i64::from(n.right) - i64::from(g.right);
    let bottom = y + h + i64::from(n.bottom) - i64::from(g.bottom);
    (right > left && bottom > top)
        .then(|| PixelRect::new(clamp_i32(left), clamp_i32(top), clamp_i32(right), clamp_i32(bottom)))
}

/// `_NET_WM_WINDOW_TYPE` values the backend tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinType {
    Normal,
    Dialog,
    Utility,
    Dock,
    Desktop,
    Menu,
    Tooltip,
    Notification,
    Splash,
    Dnd,
    PopupMenu,
    DropdownMenu,
    Combo,
    Toolbar,
    Other,
}

impl WinType {
    /// From the atom name (`_NET_WM_WINDOW_TYPE_DOCK` -> `Dock`).
    pub fn from_name(name: &str) -> WinType {
        match name.strip_prefix("_NET_WM_WINDOW_TYPE_").unwrap_or("") {
            "NORMAL" => WinType::Normal,
            "DIALOG" => WinType::Dialog,
            "UTILITY" => WinType::Utility,
            "DOCK" => WinType::Dock,
            "DESKTOP" => WinType::Desktop,
            "MENU" => WinType::Menu,
            "TOOLTIP" => WinType::Tooltip,
            "NOTIFICATION" => WinType::Notification,
            "SPLASH" => WinType::Splash,
            "DND" => WinType::Dnd,
            "POPUP_MENU" => WinType::PopupMenu,
            "DROPDOWN_MENU" => WinType::DropdownMenu,
            "COMBO" => WinType::Combo,
            "TOOLBAR" => WinType::Toolbar,
            _ => WinType::Other,
        }
    }

    /// Never a landing surface nor an occluder (the TOOLWINDOW / shell-class
    /// rules of the Windows filter).
    pub fn skipped(self) -> bool {
        matches!(
            self,
            WinType::Dock
                | WinType::Desktop
                | WinType::Menu
                | WinType::Tooltip
                | WinType::Notification
                | WinType::Splash
                | WinType::Dnd
                | WinType::PopupMenu
                | WinType::DropdownMenu
                | WinType::Combo
        )
    }
}

/// The first listed type wins (EWMH: most preferred first); none = normal.
pub fn primary_type(types: &[WinType]) -> WinType {
    types.iter().copied().find(|t| *t != WinType::Other).unwrap_or(WinType::Normal)
}

/// `_NET_WM_STATE` flags the backend reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WmState {
    pub hidden: bool,
    pub above: bool,
    pub fullscreen: bool,
}

impl WmState {
    /// From the state atom names.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> WmState {
        let mut s = WmState::default();
        for n in names {
            match n {
                "_NET_WM_STATE_HIDDEN" => s.hidden = true,
                "_NET_WM_STATE_ABOVE" => s.above = true,
                "_NET_WM_STATE_FULLSCREEN" => s.fullscreen = true,
                _ => {}
            }
        }
        s
    }
}

/// On another virtual desktop (the DWM "cloaked" equivalent).
pub fn cloaked(desktop: Option<u32>, current: Option<u32>) -> bool {
    match (desktop, current) {
        (Some(d), Some(c)) => d != ALL_DESKTOPS && d != c,
        _ => false,
    }
}

/// `_NET_WM_WINDOW_OPACITY` (0..=0xFFFFFFFF) as a layered alpha; fully opaque
/// or absent = `None`.
pub fn opacity_alpha(v: Option<u32>) -> Option<u8> {
    v.filter(|v| *v != u32::MAX).map(|v| (v >> 24) as u8)
}

/// `WM_CLASS` (instance NUL class NUL) -> the instance name, else the class.
pub fn wm_class_owner(raw: &[u8]) -> String {
    let mut parts = raw.split(|b| *b == 0).filter(|p| !p.is_empty());
    let inst = parts.next().unwrap_or_default();
    String::from_utf8_lossy(inst).into_owned()
}

/// Everything read from the server about one managed client.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientFacts {
    pub id: u32,
    /// Root-relative client origin and size; `None` when the query failed.
    pub geometry: Option<(i32, i32, u32, u32)>,
    pub net_extents: Option<Extents>,
    pub gtk_extents: Option<Extents>,
    /// The client (or its frame) is viewable.
    pub mapped: bool,
    /// ICCCM `WM_STATE` is IconicState.
    pub iconic_state: bool,
    pub state: WmState,
    pub types: Vec<WinType>,
    pub desktop: Option<u32>,
    pub pid: Option<u32>,
    pub opacity: Option<u32>,
    pub owner: String,
}

/// One client as the tracker sees it (`WindowInfo::portable`), `None` for
/// docks, desktops, menus and other non-app types.
pub fn client_info(f: &ClientFacts, current_desktop: Option<u32>, own_pid: u32) -> Option<WindowInfo> {
    if primary_type(&f.types).skipped() {
        return None;
    }
    let cloaked = cloaked(f.desktop, current_desktop);
    // A WM unmaps windows of other desktops too: that is a cloak, not a minimize.
    let iconic = f.state.hidden || f.iconic_state || (!f.mapped && !cloaked);
    let bounds = f.geometry.and_then(|(x, y, w, h)| frame_rect(x, y, w, h, f.net_extents, f.gtk_extents));
    Some(WindowInfo::portable(PortableWindow {
        id: i64::from(f.id),
        bounds,
        iconic,
        cloaked,
        topmost: f.state.above,
        own_process: f.pid == Some(own_pid),
        alpha: opacity_alpha(f.opacity),
        owner: f.owner.clone(),
    }))
}

/// One direct child of the root as `QueryTree` + attributes report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeChild {
    pub id: u32,
    pub viewable: bool,
    pub override_redirect: bool,
    pub input_output: bool,
}

/// Window source without a EWMH WM: viewable, managed-looking (not override
/// redirect) InputOutput children of the root, bottom to top.
pub fn tree_clients(children: &[TreeChild]) -> Vec<u32> {
    children.iter().filter(|c| c.viewable && !c.override_redirect && c.input_output).map(|c| c.id).collect()
}

/// Bottom-to-top stacking (`_NET_CLIENT_LIST_STACKING`, `QueryTree`) to the
/// top-down order `EnumWindows` gives, without duplicates or `None` (0) ids.
pub fn top_down(bottom_top: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(bottom_top.len());
    for id in bottom_top.iter().rev() {
        if *id != 0 && !out.contains(id) {
            out.push(*id);
        }
    }
    out
}

// ---- struts, work areas, shelves ----------------------------------------

/// Screen edge a strut reserves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Root-pixel rects a dock reserves, from `_NET_WM_STRUT_PARTIAL` (12
/// CARDINALs) or else the legacy `_NET_WM_STRUT` (4, full edge).
pub fn strut_rects(partial: Option<&[u32]>, legacy: Option<&[u32]>, root: (u32, u32)) -> Vec<(Edge, PixelRect)> {
    let (rw, rh) = (i64::from(root.0), i64::from(root.1));
    // [left, right, top, bottom, left_start_y, left_end_y, right_start_y,
    //  right_end_y, top_start_x, top_end_x, bottom_start_x, bottom_end_x]
    let v: [i64; 12] = match (partial, legacy) {
        (Some(p), _) if p.len() >= 12 => std::array::from_fn(|i| i64::from(p[i])),
        (_, Some(l)) if l.len() >= 4 => {
            let m = |i: usize| i64::from(l[i]);
            [m(0), m(1), m(2), m(3), 0, rh - 1, 0, rh - 1, 0, rw - 1, 0, rw - 1]
        }
        _ => return Vec::new(),
    };
    let r = |l: i64, t: i64, rr: i64, b: i64| PixelRect::new(clamp_i32(l), clamp_i32(t), clamp_i32(rr), clamp_i32(b));
    let mut out = Vec::new();
    if v[0] > 0 && v[5] >= v[4] {
        out.push((Edge::Left, r(0, v[4], v[0], v[5] + 1)));
    }
    if v[1] > 0 && v[7] >= v[6] {
        out.push((Edge::Right, r(rw - v[1], v[6], rw, v[7] + 1)));
    }
    if v[2] > 0 && v[9] >= v[8] {
        out.push((Edge::Top, r(v[8], 0, v[9] + 1, v[2])));
    }
    if v[3] > 0 && v[11] >= v[10] {
        out.push((Edge::Bottom, r(v[10], rh - v[3], v[11] + 1, rh)));
    }
    out
}

fn overlaps(a: &PixelRect, b: &PixelRect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// A monitor minus the struts that touch it (EWMH work area, per monitor).
pub fn work_area(monitor: PixelRect, reserved: &[(Edge, PixelRect)]) -> PixelRect {
    let mut w = monitor;
    for (edge, r) in reserved {
        if !overlaps(&monitor, r) {
            continue;
        }
        match edge {
            Edge::Left => w.left = w.left.max(r.right.min(monitor.right)),
            Edge::Right => w.right = w.right.min(r.left.max(monitor.left)),
            Edge::Top => w.top = w.top.max(r.bottom.min(monitor.bottom)),
            Edge::Bottom => w.bottom = w.bottom.min(r.top.max(monitor.top)),
        }
    }
    if w.right <= w.left || w.bottom <= w.top { monitor } else { w }
}

/// `_NET_WORKAREA` entry of a desktop (x, y, w, h per desktop).
pub fn net_workarea(v: &[u32], desktop: u32) -> Option<PixelRect> {
    let i = desktop as usize * 4;
    let s = v.get(i..i + 4)?;
    let (x, y, w, h) = (i64::from(s[0]), i64::from(s[1]), i64::from(s[2]), i64::from(s[3]));
    (w > 0 && h > 0).then(|| PixelRect::new(clamp_i32(x), clamp_i32(y), clamp_i32(x + w), clamp_i32(y + h)))
}

/// Bottom struts wider than tall, clipped to the monitor they sit on: the
/// taskbar shelves (desktop-native 3.5.6). Auto-hide panels reserve no strut
/// and so give none, like an auto-hide taskbar on Windows.
pub fn shelves(reserved: &[(Edge, PixelRect)], monitors: &[PixelRect]) -> Vec<PixelRect> {
    let mut out = Vec::new();
    for (edge, r) in reserved {
        if *edge != Edge::Bottom {
            continue;
        }
        for m in monitors {
            if !overlaps(m, r) {
                continue;
            }
            let c = PixelRect::new(r.left.max(m.left), r.top.max(m.top), r.right.min(m.right), r.bottom.min(m.bottom));
            if c.width() > c.height() && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

// ---- monitors ------------------------------------------------------------

/// `Xft.dpi` from the `RESOURCE_MANAGER` string.
pub fn xft_dpi(resources: &str) -> Option<f64> {
    resources.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == "Xft.dpi").then(|| v.trim().parse::<f64>().ok()).flatten().filter(|d| d.is_finite() && *d > 0.0)
    })
}

/// winit's `calc_dpi_factor`: pixel density in 1/12 steps, at least 1.
pub fn randr_scale(px: (u32, u32), mm: (u32, u32)) -> f64 {
    if mm.0 == 0 || mm.1 == 0 {
        return 1.0;
    }
    let ppmm = ((f64::from(px.0) * f64::from(px.1)) / (f64::from(mm.0) * f64::from(mm.1))).sqrt();
    let f = ((ppmm * (12.0 * 25.4 / 96.0)).round() / 12.0).max(1.0);
    if f <= 20.0 { f } else { 1.0 }
}

/// Monitor scale by winit 0.30's X11 rule, so egui and the core agree:
/// `WINIT_X11_SCALE_FACTOR` (a number, or `randr`), else `Xft.dpi / 96`, else
/// the RandR physical-size density.
pub fn monitor_scale(env: Option<&str>, xft: Option<f64>, px: (u32, u32), mm: (u32, u32)) -> f64 {
    match env.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) if v.eq_ignore_ascii_case("randr") => return randr_scale(px, mm),
        Some(v) => {
            if let Ok(s) = v.parse::<f64>()
                && s.is_finite()
                && s > 0.0
            {
                return s;
            }
        }
        None => {}
    }
    match xft {
        Some(d) => d / 96.0,
        None => randr_scale(px, mm),
    }
}

/// Primary first, then by left edge (desktop-native 3.6 display order).
pub fn monitor_order<T>(items: &mut [T], key: impl Fn(&T) -> (bool, i32)) {
    items.sort_by_key(|t| {
        let (primary, left) = key(t);
        (!primary, left)
    });
}

// ---- stacking ------------------------------------------------------------

/// True when `a` sits above `b` in a bottom-to-top stack (both present).
pub fn is_above(stack: &[u32], a: u32, b: u32) -> bool {
    match (stack.iter().position(|w| *w == a), stack.iter().position(|w| *w == b)) {
        (Some(ia), Some(ib)) => ia > ib,
        _ => false,
    }
}

/// `Stack::Front` needs a raise: some window that is not ours lies above `me`.
pub fn needs_front(stack: &[u32], me: u32, ours: impl Fn(u32) -> bool) -> bool {
    match stack.iter().position(|w| *w == me) {
        Some(i) => stack[i + 1..].iter().any(|w| !ours(*w)),
        None => true,
    }
}

/// What `RaisePanels` does on X11 (desktop-native 3.4): our own front
/// windows (topmost sprites, cards) must end above every panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RaisePlan {
    /// Panels to restack right below `below` (a managed card frame we cannot
    /// raise without the WM), in this order.
    pub lower: Vec<u32>,
    pub below: Option<u32>,
    /// Override-redirect windows to raise to the top, bottom first (keeps
    /// their relative order).
    pub raise: Vec<u32>,
}

/// `stack` bottom to top; `sprites` our topmost, mapped override-redirect
/// windows; `cards` our managed always-on-top frames.
pub fn raise_plan(stack: &[u32], panels: &[u32], under_dock: &[u32], sprites: &[u32], cards: &[u32]) -> RaisePlan {
    let pos = |w: &u32| stack.iter().position(|s| s == w);
    let top_panel = panels.iter().filter_map(pos).max();
    let Some(top_panel) = top_panel else { return RaisePlan::default() };
    let mut plan = RaisePlan::default();
    let low_card = cards.iter().filter(|c| !under_dock.contains(c)).filter_map(|c| pos(c).map(|i| (i, *c))).min();
    if let Some((ci, card)) = low_card {
        let mut lower: Vec<(usize, u32)> =
            panels.iter().filter_map(|p| pos(p).map(|i| (i, *p))).filter(|(i, _)| *i > ci).collect();
        lower.sort_unstable();
        if !lower.is_empty() {
            plan.lower = lower.into_iter().map(|(_, p)| p).collect();
            plan.below = Some(card);
        }
    }
    let mut mine: Vec<(usize, u32)> =
        sprites.iter().filter(|s| !under_dock.contains(s)).filter_map(|s| pos(s).map(|i| (i, *s))).collect();
    mine.sort_unstable();
    if mine.iter().any(|(i, _)| *i < top_panel) {
        plan.raise = mine.into_iter().map(|(_, s)| s).collect();
    }
    plan
}

/// Unmap of a client as a hook event: a window leaving for another desktop
/// is a cloak (no event), a hidden one minimizes, anything else hides.
pub fn unmap_event(id: u32, desktop: Option<u32>, current: Option<u32>, hidden: bool) -> Option<HookEvent> {
    if cloaked(desktop, current) {
        return None;
    }
    Some(if hidden { HookEvent::MinimizeStart(i64::from(id)) } else { HookEvent::Hide(i64::from(id)) })
}

// ---- sprite pixels -------------------------------------------------------

/// One input/bounding rectangle in window pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect16 {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

/// Rectangles covering every pixel with alpha >= `min` (row runs, equal runs
/// of consecutive rows merged). `alpha` is row-major, `w * h` long.
pub fn alpha_rects(w: u32, h: u32, alpha: &[u8], min: u8) -> Vec<Rect16> {
    let (w, h) = (w.min(i16::MAX as u32) as usize, h.min(i16::MAX as u32) as usize);
    let mut done: Vec<Rect16> = Vec::new();
    // Runs of the previous row still growing: index into `done`.
    let mut open: Vec<usize> = Vec::new();
    for y in 0..h {
        let row = alpha.get(y * w..(y + 1) * w).unwrap_or(&[]);
        let mut runs = Vec::new();
        let mut x = 0;
        while x < row.len() {
            if row[x] >= min {
                let s = x;
                while x < row.len() && row[x] >= min {
                    x += 1;
                }
                runs.push((s, x - s));
            } else {
                x += 1;
            }
        }
        let mut next_open = Vec::new();
        for (s, len) in runs {
            let grown = open.iter().copied().find(|&i| {
                let r = done[i];
                r.x as usize == s && r.w as usize == len && (r.y as usize + r.h as usize) == y
            });
            match grown {
                Some(i) => {
                    done[i].h += 1;
                    next_open.push(i);
                }
                None => {
                    done.push(Rect16 { x: s as i16, y: y as i16, w: len as u16, h: 1 });
                    next_open.push(done.len() - 1);
                }
            }
        }
        open = next_open;
    }
    done
}

/// Premultiplied RGBA (tiny-skia) -> premultiplied BGRA (X ZPixmap, 32 bpp,
/// LSBFirst), with the constant `alpha` applied on top (fades).
pub fn to_bgra(rgba: &[u8], alpha: u8, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(rgba.len());
    let a = u16::from(alpha);
    let mul = |v: u8| if a == 255 { v } else { ((u16::from(v) * a + 127) / 255) as u8 };
    for p in rgba.as_chunks::<4>().0 {
        out.extend_from_slice(&[mul(p[2]), mul(p[1]), mul(p[0]), mul(p[3])]);
    }
}

/// Row bands `(first_row, rows)` so each `PutImage` stays under `max_bytes`.
pub fn row_bands(h: u32, stride: usize, max_bytes: usize) -> Vec<(u32, u32)> {
    let per = (max_bytes / stride.max(1)).clamp(1, u32::MAX as usize) as u32;
    let mut out = Vec::new();
    let mut y = 0;
    while y < h {
        let n = per.min(h - y);
        out.push((y, n));
        y += n;
    }
    out
}

/// Order back-to-front `ids` right above `base` is already right: they form
/// the bottom of what lies above it, in order (decorations 3.7 step 4).
/// `stack` is bottom to top; `base` `None` = the bottom of the stack.
pub fn desktop_run_ok(stack: &[u32], base: Option<u32>, ids: &[u32]) -> bool {
    let start = match base {
        Some(b) => match stack.iter().position(|w| *w == b) {
            Some(i) => i + 1,
            None => return false,
        },
        None => 0,
    };
    stack.get(start..start + ids.len()).is_some_and(|run| run == ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extents_parse() {
        assert_eq!(Extents::parse(&[1, 2, 3, 4, 9]), Some(Extents { left: 1, right: 2, top: 3, bottom: 4 }));
        assert_eq!(Extents::parse(&[1, 2, 3]), None);
    }

    #[test]
    fn frames() {
        let net = Extents { left: 2, right: 2, top: 30, bottom: 2 };
        assert_eq!(frame_rect(100, 100, 400, 300, Some(net), None), Some(PixelRect::new(98, 70, 502, 402)));
        // CSD: shadows inside the client rect are removed.
        let gtk = Extents { left: 20, right: 20, top: 10, bottom: 30 };
        assert_eq!(frame_rect(0, 0, 440, 340, None, Some(gtk)), Some(PixelRect::new(20, 10, 420, 310)));
        assert_eq!(frame_rect(0, 0, 10, 10, None, Some(Extents { left: 6, right: 6, top: 0, bottom: 0 })), None);
        assert_eq!(frame_rect(0, 0, 0, 10, None, None), None);
        assert_eq!(frame_rect(i32::MAX, 0, u32::MAX, 10, None, None).map(|r| r.right), Some(i32::MAX));
    }

    #[test]
    fn types() {
        assert_eq!(WinType::from_name("_NET_WM_WINDOW_TYPE_DOCK"), WinType::Dock);
        assert_eq!(WinType::from_name("_KDE_NET_WM_WINDOW_TYPE_OVERRIDE"), WinType::Other);
        for (n, t) in [
            ("NORMAL", WinType::Normal),
            ("DIALOG", WinType::Dialog),
            ("UTILITY", WinType::Utility),
            ("DESKTOP", WinType::Desktop),
            ("MENU", WinType::Menu),
            ("TOOLTIP", WinType::Tooltip),
            ("NOTIFICATION", WinType::Notification),
            ("SPLASH", WinType::Splash),
            ("DND", WinType::Dnd),
            ("POPUP_MENU", WinType::PopupMenu),
            ("DROPDOWN_MENU", WinType::DropdownMenu),
            ("COMBO", WinType::Combo),
            ("TOOLBAR", WinType::Toolbar),
        ] {
            assert_eq!(WinType::from_name(&format!("_NET_WM_WINDOW_TYPE_{n}")), t);
        }
        assert!(WinType::Dock.skipped() && WinType::PopupMenu.skipped());
        assert!(!WinType::Normal.skipped() && !WinType::Dialog.skipped() && !WinType::Utility.skipped());
        assert_eq!(primary_type(&[WinType::Other, WinType::Dock, WinType::Normal]), WinType::Dock);
        assert_eq!(primary_type(&[]), WinType::Normal);
    }

    #[test]
    fn states_desktops_opacity() {
        let s = WmState::from_names(["_NET_WM_STATE_HIDDEN", "_NET_WM_STATE_ABOVE", "X"]);
        assert!(s.hidden && s.above && !s.fullscreen);
        assert!(WmState::from_names(["_NET_WM_STATE_FULLSCREEN"]).fullscreen);
        assert!(cloaked(Some(1), Some(0)));
        assert!(!cloaked(Some(ALL_DESKTOPS), Some(0)));
        assert!(!cloaked(Some(0), Some(0)));
        assert!(!cloaked(None, Some(0)) && !cloaked(Some(2), None));
        assert_eq!(opacity_alpha(None), None);
        assert_eq!(opacity_alpha(Some(u32::MAX)), None);
        assert_eq!(opacity_alpha(Some(0x8000_0000)), Some(128));
        assert_eq!(wm_class_owner(b"xterm\0XTerm\0"), "xterm");
        assert_eq!(wm_class_owner(b"\0Only\0"), "Only");
        assert_eq!(wm_class_owner(b""), "");
    }

    fn facts() -> ClientFacts {
        ClientFacts {
            id: 7,
            geometry: Some((10, 40, 300, 200)),
            net_extents: Some(Extents { left: 1, right: 1, top: 20, bottom: 1 }),
            mapped: true,
            desktop: Some(0),
            pid: Some(99),
            owner: "xterm".into(),
            ..Default::default()
        }
    }

    #[test]
    fn client_mapping() {
        let w = client_info(&facts(), Some(0), 1).unwrap();
        assert_eq!(w.hwnd, 7);
        assert_eq!(w.bounds, Some(PixelRect::new(9, 20, 311, 241)));
        assert!(!w.iconic && !w.cloaked && !w.own_process);
        assert_eq!(w.process_name, "xterm");
        let l = fp_tracker::filter(&w).unwrap();
        assert_eq!(l.layer, 0);
        // Own topmost (a card) drops out in the filter, own normal stays.
        let own =
            client_info(&ClientFacts { state: WmState { above: true, ..Default::default() }, ..facts() }, Some(0), 99);
        assert!(fp_tracker::filter(&own.unwrap()).is_none());
        assert!(client_info(&facts(), Some(0), 99).unwrap().own_process);
        // Other desktop: cloaked even though unmapped, not iconic.
        let away = client_info(&ClientFacts { desktop: Some(3), mapped: false, ..facts() }, Some(0), 1).unwrap();
        assert!(away.cloaked && !away.iconic);
        let min = client_info(&ClientFacts { mapped: false, ..facts() }, Some(0), 1).unwrap();
        assert!(min.iconic);
        assert!(client_info(&ClientFacts { iconic_state: true, ..facts() }, Some(0), 1).unwrap().iconic);
        assert!(client_info(&ClientFacts { types: vec![WinType::Dock], ..facts() }, Some(0), 1).is_none());
        let faded = client_info(&ClientFacts { opacity: Some(0x2000_0000), ..facts() }, None, 1).unwrap();
        assert!(fp_tracker::filter(&faded).is_none());
        assert!(client_info(&ClientFacts { geometry: None, ..facts() }, None, 1).unwrap().bounds.is_none());
    }

    #[test]
    fn tree_and_order() {
        let c =
            |id, viewable, override_redirect, input_output| TreeChild { id, viewable, override_redirect, input_output };
        let kids =
            [c(1, true, false, true), c(2, false, false, true), c(3, true, true, true), c(4, true, false, false)];
        assert_eq!(tree_clients(&kids), vec![1]);
        assert_eq!(top_down(&[1, 2, 0, 3, 2]), vec![2, 3, 1]);
    }

    #[test]
    fn struts() {
        let partial = [0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 1919];
        let r = strut_rects(Some(&partial), None, (3840, 1080));
        assert_eq!(r, vec![(Edge::Bottom, PixelRect::new(0, 1040, 1920, 1080))]);
        let legacy = [30, 20, 25, 0];
        let r = strut_rects(None, Some(&legacy), (1920, 1080));
        assert_eq!(
            r,
            vec![
                (Edge::Left, PixelRect::new(0, 0, 30, 1080)),
                (Edge::Right, PixelRect::new(1900, 0, 1920, 1080)),
                (Edge::Top, PixelRect::new(0, 0, 1920, 25)),
            ]
        );
        assert!(strut_rects(Some(&[1, 2]), None, (10, 10)).is_empty());
        assert!(strut_rects(None, None, (10, 10)).is_empty());
        // Empty range: ignored.
        assert!(strut_rects(Some(&[0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 5, 1]), None, (10, 10)).is_empty());
    }

    #[test]
    fn work_areas_and_shelves() {
        let left = PixelRect::new(0, 0, 1920, 1080);
        let right = PixelRect::new(1920, 0, 3840, 1080);
        let bottom = (Edge::Bottom, PixelRect::new(0, 1040, 1920, 1080));
        let top = (Edge::Top, PixelRect::new(1920, 0, 3840, 30));
        let side = (Edge::Left, PixelRect::new(0, 0, 50, 1080));
        let side_r = (Edge::Right, PixelRect::new(3800, 0, 3840, 1080));
        let all = [bottom, top, side, side_r];
        assert_eq!(work_area(left, &all), PixelRect::new(50, 0, 1920, 1040));
        assert_eq!(work_area(right, &all), PixelRect::new(1920, 30, 3800, 1080));
        // A strut covering everything leaves the monitor as it is.
        let huge = [(Edge::Bottom, PixelRect::new(0, 0, 1920, 1080))];
        assert_eq!(work_area(left, &huge), left);
        assert_eq!(shelves(&all, &[left, right]), vec![PixelRect::new(0, 1040, 1920, 1080)]);
        // A full-width legacy strut is split per monitor; a tall one is not a shelf.
        let wide =
            [(Edge::Bottom, PixelRect::new(0, 1000, 3840, 1080)), (Edge::Bottom, PixelRect::new(0, 0, 10, 1080))];
        assert_eq!(
            shelves(&wide, &[left, right]),
            vec![PixelRect::new(0, 1000, 1920, 1080), PixelRect::new(1920, 1000, 3840, 1080)]
        );
        assert_eq!(net_workarea(&[0, 0, 100, 90, 0, 24, 100, 66], 1), Some(PixelRect::new(0, 24, 100, 90)));
        assert_eq!(net_workarea(&[0, 0, 100, 90], 1), None);
        assert_eq!(net_workarea(&[0, 0, 0, 90], 0), None);
    }

    #[test]
    fn scales() {
        assert_eq!(xft_dpi("Xft.antialias:\t1\nXft.dpi:\t144\n"), Some(144.0));
        assert_eq!(xft_dpi("Xft.dpi: nope"), None);
        assert_eq!(xft_dpi("Xft.dpi: -3"), None);
        assert_eq!(xft_dpi("Xcursor.size: 24"), None);
        // 1920x1080 on a 527x296 mm panel (~96 dpi) -> 1.
        assert_eq!(randr_scale((1920, 1080), (527, 296)), 1.0);
        assert_eq!(randr_scale((3840, 2160), (344, 194)), 35.0 / 12.0);
        assert_eq!(randr_scale((1920, 1080), (0, 0)), 1.0);
        assert_eq!(randr_scale((60000, 60000), (1, 1)), 1.0);
        assert_eq!(monitor_scale(Some("1.5"), Some(192.0), (1, 1), (0, 0)), 1.5);
        assert_eq!(monitor_scale(Some("randr"), Some(192.0), (3840, 2160), (344, 194)), 35.0 / 12.0);
        assert_eq!(monitor_scale(Some("bad"), Some(192.0), (1, 1), (0, 0)), 2.0);
        assert_eq!(monitor_scale(Some(" "), None, (1920, 1080), (0, 0)), 1.0);
        let mut v = vec![(false, 100), (true, 1920), (false, -50)];
        monitor_order(&mut v, |m| *m);
        assert_eq!(v, vec![(true, 1920), (false, -50), (false, 100)]);
    }

    #[test]
    fn stacking() {
        let stack = [1, 2, 3, 4, 5];
        assert!(is_above(&stack, 4, 2));
        assert!(!is_above(&stack, 2, 4) && !is_above(&stack, 9, 1));
        assert!(needs_front(&stack, 3, |w| w == 5));
        assert!(!needs_front(&stack, 3, |w| w >= 4));
        assert!(needs_front(&stack, 9, |_| true));
        assert!(desktop_run_ok(&stack, Some(1), &[2, 3]));
        assert!(!desktop_run_ok(&stack, Some(1), &[3, 2]));
        assert!(desktop_run_ok(&stack, None, &[1]));
        assert!(!desktop_run_ok(&stack, Some(9), &[1]));
        assert!(!desktop_run_ok(&stack, Some(5), &[1]));
    }

    #[test]
    fn raise_plans() {
        // bottom..top: other(1) sprite(2) card(3) panel(4) panel(5) sprite(6) dock-sprite(7)
        let stack = [1, 2, 3, 4, 5, 6, 7];
        let p = raise_plan(&stack, &[4, 5], &[7], &[2, 6, 7], &[3]);
        assert_eq!(p, RaisePlan { lower: vec![4, 5], below: Some(3), raise: vec![2, 6] });
        // Already fine: nothing to do.
        let ok = [1, 4, 5, 3, 2, 6];
        assert_eq!(raise_plan(&ok, &[4, 5], &[], &[2, 6], &[3]), RaisePlan::default());
        assert_eq!(raise_plan(&ok, &[], &[], &[2], &[3]), RaisePlan::default());
    }

    #[test]
    fn unmaps() {
        assert_eq!(unmap_event(5, Some(1), Some(0), false), None);
        assert_eq!(unmap_event(5, Some(0), Some(0), true), Some(HookEvent::MinimizeStart(5)));
        assert_eq!(unmap_event(5, None, Some(0), false), Some(HookEvent::Hide(5)));
    }

    #[test]
    fn input_rects() {
        // 4x3:  ##..
        //       ##.#
        //       ....
        let a = [255, 255, 0, 0, 255, 200, 0, 9, 0, 0, 0, 0];
        let r = alpha_rects(4, 3, &a, 1);
        assert_eq!(r, vec![Rect16 { x: 0, y: 0, w: 2, h: 2 }, Rect16 { x: 3, y: 1, w: 1, h: 1 }]);
        assert_eq!(alpha_rects(4, 3, &a, 10), vec![Rect16 { x: 0, y: 0, w: 2, h: 2 }]);
        assert!(alpha_rects(2, 2, &[], 1).is_empty());
        assert!(alpha_rects(0, 0, &[], 1).is_empty());
    }

    #[test]
    fn pixels() {
        let mut out = Vec::new();
        to_bgra(&[10, 20, 30, 40, 255, 0, 0, 255], 255, &mut out);
        assert_eq!(out, vec![30, 20, 10, 40, 0, 0, 255, 255]);
        to_bgra(&[255, 255, 255, 255], 128, &mut out);
        assert_eq!(out, vec![128, 128, 128, 128]);
        assert_eq!(row_bands(10, 400, 1600), vec![(0, 4), (4, 4), (8, 2)]);
        assert_eq!(row_bands(2, 4000, 10), vec![(0, 1), (1, 1)]);
        assert!(row_bands(0, 4, 4).is_empty());
    }
}
