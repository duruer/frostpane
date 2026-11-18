//! Platform-free parts of the Wayland backend (porting design 3.3, 2.4):
//! output layout in the synthetic pixel space, sprite placement by layer
//! surface margins, input-region rectangles from an alpha mask, and the
//! parsers of the window sources (sway IPC tree, Hyprland `j/clients` +
//! `j/monitors`, the KWin script's reports). Compiled and tested on every
//! target.

use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_platform::coords::{global_scale, points_to_pixels};
use fp_tracker::{PortableWindow, WindowInfo};
use serde_json::Value;

// ---- outputs -------------------------------------------------------------

/// One `wl_output` as the backend knows it, in compositor logical
/// coordinates (xdg-output), with its effective (possibly fractional) scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// Connector name (`DP-1`, `HEADLESS-1`), else a numbered fallback.
    pub name: String,
    pub description: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub scale: f64,
}

impl Output {
    /// The output's rect in the synthetic pixel space of scale `s`.
    pub fn pixels(&self, s: f64) -> PixelRect {
        points_to_pixels(f64::from(self.x), f64::from(self.y), f64::from(self.w), f64::from(self.h), s)
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= f64::from(self.x)
            && y >= f64::from(self.y)
            && x < f64::from(self.x + self.w)
            && y < f64::from(self.y + self.h)
    }
}

/// Effective scale of an output: physical mode size over logical size when
/// both are known (fractional scaling), else the integer `wl_output.scale`.
/// `rotated`: the transform swaps width and height (90/270).
pub fn output_scale(int_scale: i32, mode: Option<(i32, i32)>, logical: Option<(i32, i32)>, rotated: bool) -> f64 {
    if let (Some((mw, mh)), Some((lw, _))) = (mode, logical)
        && lw > 0
    {
        let phys_w = if rotated { mh } else { mw };
        let s = f64::from(phys_w) / f64::from(lw);
        if s.is_finite() && s > 0.0 {
            // Round to 1/120 like wp_fractional_scale_v1 does.
            return (s * 120.0).round() / 120.0;
        }
    }
    f64::from(int_scale.max(1))
}

/// Logical space reserved by panels on one output (exclusive zones).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reserved {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Reserved {
    /// From an output rect and its usable area (both logical).
    pub fn from_area(out: (i32, i32, i32, i32), area: (i32, i32, i32, i32)) -> Reserved {
        let (ox, oy, ow, oh) = out;
        let (ax, ay, aw, ah) = area;
        if aw <= 0 || ah <= 0 {
            return Reserved::default();
        }
        Reserved {
            left: (ax - ox).max(0),
            top: (ay - oy).max(0),
            right: ((ox + ow) - (ax + aw)).max(0),
            bottom: ((oy + oh) - (ay + ah)).max(0),
        }
    }
}

/// The core's monitor list (design 2.4): logical rects times the global
/// scale `S` = largest output scale; every monitor reports scale `S` so the
/// core's points equal compositor logical coordinates. Work areas subtract
/// the panels in `reserved` (by output name). The output at the origin, else
/// the first one, is primary.
pub fn monitors(outputs: &[Output], reserved: &[(String, Reserved)]) -> (f64, Vec<MonitorInfo>) {
    let s = global_scale(&outputs.iter().map(|o| o.scale).collect::<Vec<_>>());
    let primary = outputs.iter().position(|o| o.x == 0 && o.y == 0).unwrap_or(0);
    let list = outputs
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let r = reserved.iter().find(|(n, _)| *n == o.name).map(|(_, r)| *r).unwrap_or_default();
            let work = points_to_pixels(
                f64::from(o.x + r.left),
                f64::from(o.y + r.top),
                f64::from((o.w - r.left - r.right).max(1)),
                f64::from((o.h - r.top - r.bottom).max(1)),
                s,
            );
            MonitorInfo {
                id: o.name.clone(),
                name: if o.description.is_empty() { o.name.clone() } else { o.description.clone() },
                pixels: o.pixels(s),
                work,
                scale: s,
                primary: i == primary,
            }
        })
        .collect();
    (s, list)
}

/// Bottom panel strips (taskbar shelves, desktop-native 3.5.6) in pixels:
/// the bottom reserved zone of every output that has one.
pub fn shelves(outputs: &[Output], reserved: &[(String, Reserved)], s: f64) -> Vec<PixelRect> {
    outputs
        .iter()
        .filter_map(|o| {
            let r = reserved.iter().find(|(n, _)| *n == o.name)?.1;
            (r.bottom > 0 && r.bottom < o.h).then(|| {
                points_to_pixels(
                    f64::from(o.x),
                    f64::from(o.y + o.h - r.bottom),
                    f64::from(o.w),
                    f64::from(r.bottom),
                    s,
                )
            })
        })
        .collect()
}

/// Index of the output whose pixel rect matches `pixels` (panel creation).
pub fn output_for_pixels(outputs: &[Output], s: f64, pixels: PixelRect) -> Option<usize> {
    outputs.iter().position(|o| o.pixels(s) == pixels)
}

/// A surface-local logical position on a surface with logical origin
/// `origin`, in the synthetic pixel space.
pub fn local_to_pixels(origin: (f64, f64), local: (f64, f64), s: f64) -> (i32, i32) {
    let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    (((origin.0 + local.0) * s).floor() as i32, ((origin.1 + local.1) * s).floor() as i32)
}

// ---- sprites -------------------------------------------------------------

/// Where one sprite layer surface goes: its output and top-left margins plus
/// its logical size (layer surfaces live on one output, anchored top-left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub output: usize,
    pub left: i32,
    pub top: i32,
    pub w: i32,
    pub h: i32,
}

impl Placement {
    /// Logical origin of the surface in compositor space.
    pub fn origin(&self, outputs: &[Output]) -> (f64, f64) {
        outputs.get(self.output).map_or((0.0, 0.0), |o| (f64::from(o.x + self.left), f64::from(o.y + self.top)))
    }
}

/// Logical size of a `w`x`h` pixel buffer at scale `s` (never below 1).
pub fn logical_size(w: i32, h: i32, s: f64) -> (i32, i32) {
    let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    (((f64::from(w) / s).round() as i32).max(1), ((f64::from(h) / s).round() as i32).max(1))
}

/// Placement of a sprite at pixel `(x, y)` with a `w`x`h` pixel bitmap: the
/// output under the sprite's centre (else the nearest one), margins
/// relative to its top-left. Margins may be negative (partly off-output).
pub fn place(outputs: &[Output], s: f64, x: i32, y: i32, w: i32, h: i32) -> Option<Placement> {
    let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    let (lx, ly) = (f64::from(x) / s, f64::from(y) / s);
    let (lw, lh) = logical_size(w, h, s);
    let (cx, cy) = (lx + f64::from(lw) / 2.0, ly + f64::from(lh) / 2.0);
    let dist = |o: &Output| {
        let dx = (f64::from(o.x) - cx).max(cx - f64::from(o.x + o.w)).max(0.0);
        let dy = (f64::from(o.y) - cy).max(cy - f64::from(o.y + o.h)).max(0.0);
        dx * dx + dy * dy
    };
    let i = outputs
        .iter()
        .position(|o| o.contains(cx, cy))
        .or_else(|| outputs.iter().enumerate().min_by(|a, b| dist(a.1).total_cmp(&dist(b.1))).map(|(i, _)| i))?;
    let o = &outputs[i];
    Some(Placement {
        output: i,
        left: (lx - f64::from(o.x)).round() as i32,
        top: (ly - f64::from(o.y)).round() as i32,
        w: lw,
        h: lh,
    })
}

/// Input-region rectangles `(x, y, w, h)` in buffer pixels from a
/// premultiplied RGBA buffer: runs of alpha > 0, sampled every `step` rows
/// (each row band `step` high), vertically merged where consecutive bands
/// have the same runs (design 3.3 "Hit test").
pub fn mask_rects(rgba: &[u8], w: u32, h: u32, step: u32) -> Vec<(i32, i32, i32, i32)> {
    let step = step.max(1);
    let (w, h) = (w as usize, h as usize);
    if rgba.len() < w * h * 4 {
        return Vec::new();
    }
    let mut out: Vec<(i32, i32, i32, i32)> = Vec::new();
    // Rect indices of the previous band, by run.
    let mut open: Vec<usize> = Vec::new();
    let mut prev_runs: Vec<(usize, usize)> = Vec::new();
    let mut y = 0;
    while y < h {
        let band = (step as usize).min(h - y);
        let mut runs = Vec::new();
        let mut x = 0;
        while x < w {
            let opaque = |x: usize| (0..band).any(|dy| rgba[((y + dy) * w + x) * 4 + 3] > 0);
            if opaque(x) {
                let start = x;
                while x < w && opaque(x) {
                    x += 1;
                }
                runs.push((start, x));
            } else {
                x += 1;
            }
        }
        if runs == prev_runs && !runs.is_empty() {
            for &i in &open {
                out[i].3 += band as i32;
            }
        } else {
            open.clear();
            for &(a, b) in &runs {
                open.push(out.len());
                out.push((a as i32, y as i32, (b - a) as i32, band as i32));
            }
        }
        prev_runs = runs;
        y += band;
    }
    out
}

/// Buffer-pixel rectangles to surface-local logical rectangles (rounded
/// outwards so nothing opaque is lost).
pub fn scale_rects(rects: &[(i32, i32, i32, i32)], s: f64) -> Vec<(i32, i32, i32, i32)> {
    let s = if s.is_finite() && s > 0.0 { s } else { 1.0 };
    rects
        .iter()
        .map(|&(x, y, w, h)| {
            let x0 = (f64::from(x) / s).floor() as i32;
            let y0 = (f64::from(y) / s).floor() as i32;
            let x1 = (f64::from(x + w) / s).ceil() as i32;
            let y1 = (f64::from(y + h) / s).ceil() as i32;
            (x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
        })
        .collect()
}

/// Premultiplied RGBA to `wl_shm` ARGB8888 (little-endian B, G, R, A).
pub fn rgba_to_argb(src: &[u8], dst: &mut [u8]) {
    for (s, d) in src.as_chunks::<4>().0.iter().zip(dst.as_chunks_mut::<4>().0) {
        *d = [s[2], s[1], s[0], s[3]];
    }
}

// ---- foreign windows -----------------------------------------------------

/// One foreign top-level window from a window source, logical coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Foreign {
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Minimized / scratchpad / not mapped.
    pub iconic: bool,
    /// On a workspace that is not shown, or a hidden tab.
    pub cloaked: bool,
    /// Kept above (occluder only).
    pub topmost: bool,
    pub fullscreen: bool,
    pub focused: bool,
    pub pid: Option<u32>,
    pub owner: String,
    pub alpha: Option<u8>,
}

/// What one source listing says: windows top first, plus panel space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub windows: Vec<Foreign>,
    /// Reserved panel space by output name.
    pub reserved: Vec<(String, Reserved)>,
}

impl Snapshot {
    /// Pixel rects of visible fullscreen windows.
    pub fn fullscreen(&self, s: f64) -> Vec<PixelRect> {
        self.windows.iter().filter(|w| w.fullscreen && !w.iconic && !w.cloaked).map(|w| rect_px(w, s)).collect()
    }

    /// The pixel lies on a visible window of process `pid`.
    pub fn own_at(&self, pid: u32, s: f64, at: (i32, i32)) -> bool {
        self.windows
            .iter()
            .filter(|w| w.pid == Some(pid) && !w.iconic && !w.cloaked)
            .any(|w| rect_px(w, s).contains(f64::from(at.0), f64::from(at.1)))
    }
}

fn rect_px(w: &Foreign, s: f64) -> PixelRect {
    points_to_pixels(f64::from(w.x), f64::from(w.y), f64::from(w.w), f64::from(w.h), s)
}

/// Opacity 0..1 to a layered alpha; opaque gives `None`.
fn alpha_of(op: Option<f64>) -> Option<u8> {
    let op = op.filter(|o| o.is_finite())?;
    (op < 0.999).then(|| (op.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The tracker's raw window list (`fp_tracker::filter` input), top first.
pub fn to_infos(list: &[Foreign], s: f64, own_pid: u32) -> Vec<WindowInfo> {
    list.iter()
        .map(|w| {
            WindowInfo::portable(PortableWindow {
                id: w.id,
                bounds: (w.w > 0 && w.h > 0).then(|| rect_px(w, s)),
                iconic: w.iconic,
                cloaked: w.cloaked,
                topmost: w.topmost,
                own_process: w.pid == Some(own_pid),
                alpha: w.alpha,
                owner: w.owner.clone(),
            })
        })
        .collect()
}

fn int(v: &Value, k: &str) -> i32 {
    v.get(k).and_then(Value::as_i64).unwrap_or(0) as i32
}

fn rect_of(v: &Value) -> (i32, i32, i32, i32) {
    (int(v, "x"), int(v, "y"), int(v, "width"), int(v, "height"))
}

fn text(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn pid(v: &Value) -> Option<u32> {
    v.get("pid").and_then(Value::as_i64).filter(|p| *p > 0).map(|p| p as u32)
}

/// FNV-1a of a string id (KWin uuids) as a positive i64.
pub fn hash_id(s: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    (h >> 1) as i64
}

// ---- sway / i3 IPC -------------------------------------------------------

/// i3 IPC message type `GET_TREE`.
pub const SWAY_GET_TREE: u32 = 4;
/// i3 IPC message type `SUBSCRIBE`.
pub const SWAY_SUBSCRIBE: u32 = 2;
/// i3 IPC magic.
pub const SWAY_MAGIC: &[u8; 6] = b"i3-ipc";

/// One i3 IPC message: magic, payload length, type (native endian), payload.
pub fn ipc_encode(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(14 + payload.len());
    v.extend_from_slice(SWAY_MAGIC);
    v.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    v.extend_from_slice(&kind.to_ne_bytes());
    v.extend_from_slice(payload);
    v
}

/// `(payload length, type)` of a 14-byte i3 IPC header; `None` on bad magic.
pub fn ipc_header(h: &[u8; 14]) -> Option<(u32, u32)> {
    if &h[..6] != SWAY_MAGIC {
        return None;
    }
    let len = u32::from_ne_bytes([h[6], h[7], h[8], h[9]]);
    let kind = u32::from_ne_bytes([h[10], h[11], h[12], h[13]]);
    Some((len, kind))
}

fn sway_children(n: &Value) -> impl Iterator<Item = &Value> {
    let a = n.get("nodes").and_then(Value::as_array).into_iter().flatten();
    a.chain(n.get("floating_nodes").and_then(Value::as_array).into_iter().flatten())
}

/// Views of a container subtree, top first: floating (later = above) over
/// tiled.
fn sway_views<'a>(n: &'a Value, floating: bool, out: &mut Vec<(&'a Value, bool)>) {
    let is_view = n.get("pid").is_some() || n.get("app_id").is_some_and(|a| !a.is_null());
    let has_kids = sway_children(n).next().is_some();
    if is_view && !has_kids {
        out.push((n, floating));
        return;
    }
    if let Some(f) = n.get("floating_nodes").and_then(Value::as_array) {
        for c in f.iter().rev() {
            sway_views(c, true, out);
        }
    }
    if let Some(t) = n.get("nodes").and_then(Value::as_array) {
        for c in t {
            sway_views(c, floating, out);
        }
    }
}

/// Flattens a sway `get_tree` reply: views of shown workspaces first
/// (floating above tiled), then the rest as cloaked; the scratchpad
/// (`__i3`) is iconic. Reserved panel space per output from the output
/// rect versus its workspaces' rect.
pub fn sway_tree(root: &Value) -> Snapshot {
    let mut shown = Vec::new();
    let mut hidden = Vec::new();
    let mut reserved = Vec::new();
    for out in root.get("nodes").and_then(Value::as_array).into_iter().flatten() {
        let name = text(out, "name");
        let scratch = name == "__i3";
        let current = text(out, "current_workspace");
        let orect = out.get("rect").map(rect_of).unwrap_or_default();
        for ws in out.get("nodes").and_then(Value::as_array).into_iter().flatten() {
            let visible = !scratch && text(ws, "name") == current;
            if visible && let Some(r) = ws.get("rect") {
                reserved.push((name.clone(), Reserved::from_area(orect, rect_of(r))));
            }
            let mut views = Vec::new();
            sway_views(ws, false, &mut views);
            for (v, floating) in views {
                let (x, y, w, h) = v.get("rect").map(rect_of).unwrap_or_default();
                let fullscreen = v.get("fullscreen_mode").and_then(Value::as_i64).unwrap_or(0) > 0;
                let view_visible = v.get("visible").and_then(Value::as_bool).unwrap_or(visible);
                let owner = v
                    .get("app_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| v.pointer("/window_properties/class").and_then(Value::as_str).map(str::to_string))
                    .unwrap_or_default();
                let f = Foreign {
                    id: v.get("id").and_then(Value::as_i64).unwrap_or(0),
                    x,
                    y,
                    w,
                    h,
                    iconic: scratch,
                    cloaked: !scratch && !(visible && view_visible),
                    topmost: fullscreen && floating,
                    fullscreen,
                    focused: v.get("focused").and_then(Value::as_bool).unwrap_or(false),
                    pid: pid(v),
                    owner,
                    alpha: None,
                };
                if visible { shown.push(f) } else { hidden.push(f) }
            }
        }
    }
    // A fullscreen view covers everything else on its workspace.
    shown.sort_by_key(|f| !f.fullscreen);
    shown.extend(hidden);
    Snapshot { windows: shown, reserved }
}

/// The sway event types worth a re-list (`window`, `workspace`, `output`):
/// i3 IPC event bit 31 set plus the low type.
pub fn sway_event_relevant(kind: u32) -> bool {
    kind & 0x8000_0000 != 0 && matches!(kind & 0x7fff_ffff, 0 | 1 | 3)
}

// ---- Hyprland IPC --------------------------------------------------------

fn pair(v: &Value, k: &str) -> (i32, i32) {
    let a = v.get(k).and_then(Value::as_array);
    let at = |i: usize| a.and_then(|a| a.get(i)).and_then(Value::as_i64).unwrap_or(0) as i32;
    (at(0), at(1))
}

/// Shown workspace ids and reserved space per monitor from `j/monitors`.
pub fn hypr_monitors(v: &Value) -> (Vec<i64>, Vec<(String, Reserved)>) {
    let mut shown = Vec::new();
    let mut reserved = Vec::new();
    for m in v.as_array().into_iter().flatten() {
        for k in ["activeWorkspace", "specialWorkspace"] {
            if let Some(id) = m.pointer(&format!("/{k}/id")).and_then(Value::as_i64).filter(|id| *id != 0) {
                shown.push(id);
            }
        }
        let r = m.get("reserved").and_then(Value::as_array);
        let at = |i: usize| r.and_then(|r| r.get(i)).and_then(Value::as_i64).unwrap_or(0) as i32;
        reserved.push((text(m, "name"), Reserved { left: at(0), top: at(1), right: at(2), bottom: at(3) }));
    }
    (shown, reserved)
}

/// Windows from Hyprland `j/clients`, top first. Z-order is approximated by
/// `focusHistoryID` (0 = last focused) with floating above tiled; a client
/// is cloaked unless its workspace is in `shown`.
pub fn hypr_clients(v: &Value, shown: &[i64]) -> Vec<Foreign> {
    let mut list: Vec<(bool, bool, i64, Foreign)> = Vec::new();
    for c in v.as_array().into_iter().flatten() {
        let addr = text(c, "address");
        let id = i64::from_str_radix(addr.trim_start_matches("0x"), 16).unwrap_or_else(|_| hash_id(&addr));
        let (x, y) = pair(c, "at");
        let (w, h) = pair(c, "size");
        let ws = c.pointer("/workspace/id").and_then(Value::as_i64).unwrap_or(0);
        let mapped = c.get("mapped").and_then(Value::as_bool).unwrap_or(true);
        let hidden = c.get("hidden").and_then(Value::as_bool).unwrap_or(false);
        let fullscreen = match c.get("fullscreen") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0) > 0,
            _ => false,
        };
        let floating = c.get("floating").and_then(Value::as_bool).unwrap_or(false);
        let pinned = c.get("pinned").and_then(Value::as_bool).unwrap_or(false);
        let history = c.get("focusHistoryID").and_then(Value::as_i64).unwrap_or(i64::MAX);
        let f = Foreign {
            id,
            x,
            y,
            w,
            h,
            iconic: !mapped || hidden,
            cloaked: !pinned && !shown.contains(&ws),
            topmost: pinned,
            fullscreen,
            focused: history == 0,
            pid: pid(c),
            owner: text(c, "class"),
            alpha: None,
        };
        list.push((!fullscreen, !floating, history, f));
    }
    list.sort_by_key(|(nf, nfl, hist, _)| (*nf, *nfl, *hist));
    list.into_iter().map(|t| t.3).collect()
}

/// A Hyprland event-socket line worth a re-list (`event>>data`).
pub fn hypr_event_relevant(line: &str) -> bool {
    let ev = line.split(">>").next().unwrap_or("");
    matches!(
        ev,
        "openwindow"
            | "closewindow"
            | "movewindow"
            | "movewindowv2"
            | "activewindow"
            | "activewindowv2"
            | "workspace"
            | "workspacev2"
            | "focusedmon"
            | "focusedmonv2"
            | "changefloatingmode"
            | "fullscreen"
            | "minimized"
            | "monitoradded"
            | "monitorremoved"
            | "pin"
    )
}

// ---- KWin script ---------------------------------------------------------

/// The KWin script, with `%SERVICE%` to be replaced by our bus name.
pub const KWIN_SCRIPT: &str = include_str!("kwin.js");
/// D-Bus object path the script reports to.
pub const KWIN_PATH: &str = "/geometry";
/// D-Bus interface of our report receiver.
pub const KWIN_IFACE: &str = "org.frostpane.Geometry";

/// The script text for bus name `service`.
pub fn kwin_script(service: &str) -> String {
    let safe: String =
        service.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-')).collect();
    KWIN_SCRIPT.replace("%SERVICE%", &safe)
}

/// One report of the KWin script:
/// `{"windows":[{"id","g":[x,y,w,h],"min","desk","above","full","op","pid","cls","act"}],
///   "areas":[{"name","g":[..],"a":[..]}]}`, windows top first.
pub fn kwin_report(json: &str) -> Option<Snapshot> {
    let v: Value = serde_json::from_str(json).ok()?;
    let four = |w: &Value, k: &str| {
        let a = w.get(k).and_then(Value::as_array);
        let at = |i: usize| a.and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or(0.0).round() as i32;
        (at(0), at(1), at(2), at(3))
    };
    let flag = |w: &Value, k: &str| w.get(k).and_then(Value::as_bool).unwrap_or(false);
    let windows = v
        .get("windows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|w| {
            let (x, y, ww, hh) = four(w, "g");
            Foreign {
                id: hash_id(&text(w, "id")),
                x,
                y,
                w: ww,
                h: hh,
                iconic: flag(w, "min"),
                cloaked: !w.get("desk").and_then(Value::as_bool).unwrap_or(true),
                topmost: flag(w, "above"),
                fullscreen: flag(w, "full"),
                focused: flag(w, "act"),
                pid: pid(w),
                owner: text(w, "cls"),
                alpha: alpha_of(w.get("op").and_then(Value::as_f64)),
            }
        })
        .collect();
    let reserved = v
        .get("areas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|a| (text(a, "name"), Reserved::from_area(four(a, "g"), four(a, "a"))))
        .collect();
    Some(Snapshot { windows, reserved })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn out(name: &str, x: i32, y: i32, w: i32, h: i32, scale: f64) -> Output {
        Output { name: name.into(), description: String::new(), x, y, w, h, scale }
    }

    #[test]
    fn scales() {
        assert_eq!(output_scale(2, Some((2880, 1800)), Some((1920, 1200)), false), 1.5);
        assert_eq!(output_scale(1, Some((3840, 2160)), Some((1080, 1920)), true), 2.0);
        assert_eq!(output_scale(2, None, Some((1, 1)), false), 2.0);
        assert_eq!(output_scale(0, Some((10, 10)), Some((0, 0)), false), 1.0);
        assert_eq!(output_scale(3, Some((0, 0)), Some((10, 10)), false), 3.0);
    }

    #[test]
    fn layout_and_work() {
        let outs = [out("A", 0, 0, 1920, 1080, 1.0), out("B", 1920, 0, 1280, 720, 2.0)];
        let res = [("A".to_string(), Reserved { bottom: 40, ..Default::default() })];
        let (s, m) = monitors(&outs, &res);
        assert_eq!(s, 2.0);
        assert_eq!(m[0].pixels, PixelRect::new(0, 0, 3840, 2160));
        assert_eq!(m[0].work, PixelRect::new(0, 0, 3840, 2080));
        assert_eq!(m[1].pixels, PixelRect::new(3840, 0, 6400, 1440));
        assert_eq!(m[1].work, m[1].pixels);
        assert!(m[0].primary && !m[1].primary);
        assert_eq!(m[1].scale, 2.0);
        assert_eq!(m[0].name, "A");
        let d = Output { description: "Desk".into(), ..outs[0].clone() };
        assert_eq!(monitors(&[d], &[]).1[0].name, "Desk");
        // No output at the origin: the first is primary.
        let (_, m) = monitors(&[out("X", 10, 0, 5, 5, 1.0), out("Y", 20, 0, 5, 5, 1.0)], &[]);
        assert!(m[0].primary);
        assert_eq!(shelves(&outs, &res, s), vec![PixelRect::new(0, 2080, 3840, 2160)]);
        let top = [("B".to_string(), Reserved { top: 30, ..Default::default() })];
        assert!(shelves(&outs, &top, s).is_empty());
        assert_eq!(output_for_pixels(&outs, s, PixelRect::new(3840, 0, 6400, 1440)), Some(1));
        assert_eq!(output_for_pixels(&outs, s, PixelRect::new(0, 0, 1, 1)), None);
    }

    #[test]
    fn reserved_from_area() {
        assert_eq!(
            Reserved::from_area((0, 0, 100, 100), (10, 20, 80, 50)),
            Reserved { left: 10, top: 20, right: 10, bottom: 30 }
        );
        assert_eq!(Reserved::from_area((0, 0, 100, 100), (0, 0, 0, 0)), Reserved::default());
    }

    #[test]
    fn placement() {
        let outs = [out("A", 0, 0, 1000, 800, 1.0), out("B", 1000, 0, 1000, 800, 1.0)];
        let p = place(&outs, 2.0, 400, 200, 100, 60).unwrap();
        assert_eq!(p, Placement { output: 0, left: 200, top: 100, w: 50, h: 30 });
        assert_eq!(p.origin(&outs), (200.0, 100.0));
        // Centre on B.
        let p = place(&outs, 1.0, 990, 10, 40, 40).unwrap();
        assert_eq!((p.output, p.left, p.top), (1, -10, 10));
        // Off every output: nearest one.
        let p = place(&outs, 1.0, 2500, -300, 10, 10).unwrap();
        assert_eq!((p.output, p.left, p.top), (1, 1500, -300));
        assert!(place(&[], 1.0, 0, 0, 1, 1).is_none());
        assert_eq!(place(&outs, f64::NAN, 10, 10, 4, 4).map(|p| (p.left, p.w)), Some((10, 4)));
        assert_eq!(logical_size(3, 3, f64::NAN), (3, 3));
        assert_eq!(logical_size(1, 1, 4.0), (1, 1));
        assert_eq!(Placement { output: 9, left: 0, top: 0, w: 1, h: 1 }.origin(&outs), (0.0, 0.0));
        assert_eq!(local_to_pixels((10.0, 20.0), (1.5, 2.5), 2.0), (23, 45));
        assert_eq!(local_to_pixels((1.0, 1.0), (0.0, 0.0), 0.0), (1, 1));
    }

    fn mask(w: u32, h: u32, on: &[(u32, u32)]) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 4) as usize];
        for &(x, y) in on {
            v[((y * w + x) * 4 + 3) as usize] = 255;
        }
        v
    }

    #[test]
    fn mask_runs() {
        // Two-pixel wide column at x=1..3 over 4 rows: one merged rect.
        let m = mask(4, 4, &[(1, 0), (2, 0), (1, 1), (2, 1), (1, 2), (2, 2), (1, 3), (2, 3)]);
        assert_eq!(mask_rects(&m, 4, 4, 2), vec![(1, 0, 2, 4)]);
        // Different runs per band; odd height ends with a short band.
        let m = mask(4, 3, &[(0, 0), (3, 1), (2, 2)]);
        assert_eq!(mask_rects(&m, 4, 3, 2), vec![(0, 0, 1, 2), (3, 0, 1, 2), (2, 2, 1, 1)]);
        assert!(mask_rects(&mask(2, 2, &[]), 2, 2, 0).is_empty());
        assert!(mask_rects(&[0; 3], 2, 2, 1).is_empty());
        // Empty band between equal bands does not merge across it.
        let m = mask(1, 3, &[(0, 0), (0, 2)]);
        assert_eq!(mask_rects(&m, 1, 3, 1), vec![(0, 0, 1, 1), (0, 2, 1, 1)]);
        assert_eq!(scale_rects(&[(1, 1, 3, 3)], 2.0), vec![(0, 0, 2, 2)]);
        assert_eq!(scale_rects(&[(0, 0, 1, 1)], f64::NAN), vec![(0, 0, 1, 1)]);
        let mut d = [0u8; 4];
        rgba_to_argb(&[1, 2, 3, 4], &mut d);
        assert_eq!(d, [3, 2, 1, 4]);
    }

    #[test]
    fn ipc_frames() {
        let m = ipc_encode(SWAY_GET_TREE, b"{}");
        assert_eq!(m.len(), 16);
        let h: [u8; 14] = m[..14].try_into().unwrap();
        assert_eq!(ipc_header(&h), Some((2, SWAY_GET_TREE)));
        let mut bad = h;
        bad[0] = b'x';
        assert_eq!(ipc_header(&bad), None);
        assert!(sway_event_relevant(0x8000_0003));
        assert!(sway_event_relevant(0x8000_0000));
        assert!(!sway_event_relevant(0x8000_0004));
        assert!(!sway_event_relevant(3));
    }

    fn sway_sample() -> Value {
        json!({"nodes": [
            {"name": "__i3", "rect": {"x":0,"y":0,"width":0,"height":0}, "nodes": [
                {"name": "__i3_scratch", "nodes": [], "floating_nodes": [
                    {"id": 9, "pid": 5, "app_id": "scratch", "rect": {"x":0,"y":0,"width":10,"height":10}, "nodes": [], "floating_nodes": []}
                ]}
            ]},
            {"name": "HEADLESS-1", "current_workspace": "1", "rect": {"x":0,"y":0,"width":1920,"height":1080}, "nodes": [
                {"name": "1", "rect": {"x":0,"y":30,"width":1920,"height":1050}, "nodes": [
                    {"id": 3, "pid": 7, "app_id": "foot", "visible": true, "focused": true,
                     "rect": {"x":0,"y":30,"width":960,"height":1050}, "nodes": [], "floating_nodes": []},
                    {"id": 10, "layout": "tabbed", "nodes": [
                        {"id": 11, "pid": 8, "app_id": null, "window_properties": {"class": "XTerm"}, "visible": false,
                         "rect": {"x":960,"y":30,"width":960,"height":1050}, "nodes": []}
                    ]}
                ], "floating_nodes": [
                    {"id": 4, "pid": 7, "app_id": "a", "rect": {"x":100,"y":100,"width":300,"height":200}, "nodes": [], "floating_nodes": []},
                    {"id": 5, "pid": 7, "app_id": "b", "fullscreen_mode": 1, "rect": {"x":0,"y":0,"width":1920,"height":1080}, "nodes": [], "floating_nodes": []}
                ]},
                {"name": "2", "rect": {"x":0,"y":30,"width":1920,"height":1050}, "nodes": [
                    {"id": 6, "pid": 7, "app_id": "c", "rect": {"x":0,"y":30,"width":1920,"height":1050}, "nodes": [], "floating_nodes": []}
                ], "floating_nodes": []}
            ]}
        ]})
    }

    #[test]
    fn sway_flatten() {
        let s = sway_tree(&sway_sample());
        let ids: Vec<i64> = s.windows.iter().map(|w| w.id).collect();
        // fullscreen 5 first, then floating 4, tiled 3 and hidden tab 11, then
        // the scratchpad (iconic) and workspace 2 (cloaked).
        assert_eq!(ids, vec![5, 4, 3, 11, 9, 6]);
        let w = |id| s.windows.iter().find(|w| w.id == id).unwrap();
        assert!(w(5).fullscreen && w(5).topmost);
        assert!(w(3).focused && !w(3).cloaked && w(3).owner == "foot");
        assert!(w(11).cloaked && w(11).owner == "XTerm");
        assert!(w(9).iconic && !w(9).cloaked);
        assert!(w(6).cloaked);
        assert_eq!((w(4).x, w(4).y, w(4).w, w(4).h, w(4).pid), (100, 100, 300, 200, Some(7)));
        assert_eq!(s.reserved, vec![("HEADLESS-1".to_string(), Reserved { top: 30, ..Default::default() })]);
        assert_eq!(s.fullscreen(1.0), vec![PixelRect::new(0, 0, 1920, 1080)]);
        assert!(s.own_at(7, 1.0, (150, 150)));
        assert!(!s.own_at(8, 1.0, (1000, 100)));
        assert!(sway_tree(&json!({})).windows.is_empty());
        // A workspace without node arrays and a bare container: no views.
        let bare = json!({"nodes": [{"name": "O", "current_workspace": "1", "nodes": [
            {"name": "1"}, {"name": "1", "nodes": [{"id": 1}, {"id": 2, "app_id": "x"}]}
        ]}]});
        let s = sway_tree(&bare);
        // Only the app_id view (no pid) is listed.
        assert_eq!(s.windows.iter().map(|w| w.id).collect::<Vec<_>>(), vec![2]);
        assert_eq!(s.windows[0].pid, None);
        assert_eq!(s.reserved.len(), 0);
    }

    #[test]
    fn infos() {
        let f = Foreign {
            id: 3,
            x: 10,
            y: 10,
            w: 100,
            h: 50,
            pid: Some(42),
            alpha: Some(128),
            owner: "foot".into(),
            ..Default::default()
        };
        let g = Foreign { id: 4, topmost: true, ..Default::default() };
        let i = to_infos(&[f, g], 2.0, 42);
        assert_eq!(i[0].bounds, Some(PixelRect::new(20, 20, 220, 120)));
        assert!(i[0].own_process && i[0].layered_alpha == Some(128));
        assert_eq!(i[1].bounds, None);
        assert!(i[1].ex_style != 0 && !i[1].own_process);
        assert_eq!(i[0].process_name, "foot");
    }

    #[test]
    fn hyprland() {
        let mons = json!([
            {"name": "DP-1", "activeWorkspace": {"id": 1}, "specialWorkspace": {"id": 0}, "reserved": [0, 0, 0, 32]},
            {"name": "DP-2", "activeWorkspace": {"id": 3}, "specialWorkspace": {"id": -98}, "reserved": [10, 20]}
        ]);
        let (shown, res) = hypr_monitors(&mons);
        assert_eq!(shown, vec![1, 3, -98]);
        assert_eq!(res[0].1, Reserved { bottom: 32, ..Default::default() });
        assert_eq!(res[1].1, Reserved { left: 10, top: 20, ..Default::default() });
        let clients = json!([
            {"address": "0x1a", "mapped": true, "hidden": false, "at": [0, 0], "size": [800, 600],
             "workspace": {"id": 1}, "floating": false, "pid": 11, "class": "kitty", "fullscreen": 0, "focusHistoryID": 1},
            {"address": "0x2b", "at": [50, 60], "size": [300, 200], "workspace": {"id": 1}, "floating": true,
             "pid": 12, "class": "pavucontrol", "fullscreen": false, "focusHistoryID": 2},
            {"address": "0x3c", "at": [0, 0], "size": [800, 600], "workspace": {"id": 2}, "focusHistoryID": 0},
            {"address": "zz", "at": [0, 0], "size": [1920, 1080], "workspace": {"id": 3}, "fullscreen": 2,
             "focusHistoryID": 5, "hidden": true},
            {"address": "0x4d", "workspace": {"id": 7}, "pinned": true, "fullscreen": true, "focusHistoryID": 4}
        ]);
        let list = hypr_clients(&clients, &shown);
        let ids: Vec<i64> = list.iter().map(|w| w.id).collect();
        assert_eq!(ids, vec![0x4d, hash_id("zz"), 0x2b, 0x3c, 0x1a]);
        assert!(list[0].topmost && !list[0].cloaked && list[0].fullscreen);
        assert!(list[1].iconic && list[1].fullscreen);
        assert!(list[3].cloaked && list[3].focused);
        assert_eq!((list[4].w, list[4].h, list[4].owner.as_str(), list[4].pid), (800, 600, "kitty", Some(11)));
        assert!(hypr_event_relevant("openwindow>>a,b,c,d"));
        assert!(hypr_event_relevant("workspacev2>>1,1"));
        assert!(!hypr_event_relevant("submap>>x"));
        assert!(!hypr_event_relevant(""));
    }

    #[test]
    fn kwin() {
        let s = kwin_script("org.x-1:2'); evil(");
        assert!(s.contains("org.x-1:2evil"));
        assert!(!s.contains("%SERVICE%"));
        let r = kwin_report(
            r#"{"windows":[
                {"id":"{a}","g":[10.4,20,300,200],"min":false,"desk":true,"above":true,"full":false,"op":0.5,"pid":9,"cls":"dolphin","act":true},
                {"id":"{b}","g":[0,0,10,10],"min":true,"desk":false,"op":1.0}
            ],"areas":[{"name":"DP-1","g":[0,0,1920,1080],"a":[0,0,1920,1036]}]}"#,
        )
        .unwrap();
        assert_eq!(r.windows.len(), 2);
        let a = &r.windows[0];
        assert_eq!((a.x, a.y, a.w, a.h), (10, 20, 300, 200));
        assert!(a.topmost && a.focused && !a.cloaked && a.alpha == Some(128) && a.pid == Some(9));
        assert_eq!(a.id, hash_id("{a}"));
        assert!(r.windows[1].iconic && r.windows[1].cloaked && r.windows[1].alpha.is_none());
        assert_eq!(r.reserved, vec![("DP-1".to_string(), Reserved { bottom: 44, ..Default::default() })]);
        assert!(kwin_report("nope").is_none());
        assert_eq!(kwin_report("{}"), Some(Snapshot::default()));
        assert_eq!(alpha_of(Some(f64::NAN)), None);
        assert_ne!(hash_id("a"), hash_id("b"));
        assert!(hash_id("x") >= 0);
    }
}
