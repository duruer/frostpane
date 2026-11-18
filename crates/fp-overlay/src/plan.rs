//! Pure overlay logic: DIP sizing, cursor mapping, screen matching, the
//! behind-windows staging machine and GPU buffer sizing. No OS or GPU calls,
//! so everything here is unit-tested.

use fp_core::desktop::MonitorInfo;
use fp_core::{Frame, PixelRect, Vec2};

/// One overlay screen as the effect factory sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelInfo {
    /// Index in the monitor list the host was built from.
    pub index: usize,
    /// Display id of the monitor.
    pub id: String,
    /// Monitor size in points (what the simulation is built with).
    pub width: f64,
    pub height: f64,
    /// Effective DPI / 96 (never <= 0).
    pub scale: f64,
    /// Monitor frame in global points.
    pub frame: Frame,
}

/// Panel infos for a monitor list, in the same order (shared `Layout` rules).
pub fn panel_infos(monitors: &[MonitorInfo]) -> Vec<PanelInfo> {
    let layout = fp_core::desktop::Layout::new(monitors.to_vec());
    layout
        .screens()
        .iter()
        .enumerate()
        .map(|(index, s)| PanelInfo {
            index,
            id: s.info.id.clone(),
            width: s.points.w,
            height: s.points.h,
            scale: s.info.scale,
            frame: s.points,
        })
        .collect()
}

/// Swap chain size in pixels: `ceil` of the monitor size in points, so a 150%
/// 3840x2160 monitor renders at 2560x1440 (desktop-native 3.2).
pub fn swap_size(width_pts: f64, height_pts: f64) -> (u32, u32) {
    let f = |v: f64| if v.is_finite() && v > 1.0 { v.ceil().min(16384.0) as u32 } else { 1 };
    (f(width_pts), f(height_pts))
}

/// Visual scale that stretches the DIP-sized swap chain over the monitor's
/// pixels (desktop-native 3.2 `SetTransform`).
pub fn visual_scale(pixels: PixelRect, swap: (u32, u32)) -> (f32, f32) {
    (pixels.width().max(1) as f32 / swap.0.max(1) as f32, pixels.height().max(1) as f32 / swap.1.max(1) as f32)
}

/// DPI the panel was created at, for the "rescaled" check (desktop-native 3.1).
pub fn creation_dpi(scale: f64) -> u32 {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    (s * 96.0).round() as u32
}

/// Cursor in screen-local points (Y up) when it lies inside the monitor's
/// pixel rect, else `None` (desktop-native 3.8 step 4).
pub fn cursor_local(pixels: PixelRect, scale: f64, x: i32, y: i32) -> Option<Vec2> {
    if x < pixels.left || x >= pixels.right || y < pixels.top || y >= pixels.bottom {
        return None;
    }
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    Some(Vec2::new((x - pixels.left) as f64 / s, (pixels.bottom - y) as f64 / s))
}

/// Index of the frame closest to `target` (same corner and size within 2 pt),
/// used to pair tracker snapshots with panels.
pub fn match_frame(frames: &[Frame], target: &Frame) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, f) in frames.iter().enumerate() {
        let d = (f.x - target.x).abs() + (f.y - target.y).abs() + (f.w - target.w).abs() + (f.h - target.h).abs();
        if d <= 2.0 && best.is_none_or(|(_, b)| d < b) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// Capacity (bytes) of a storage buffer that must hold `len` bytes: next power
/// of two, at least 256, so buffers grow rarely.
pub fn buffer_capacity(len: usize) -> u64 {
    (len.max(256) as u64).next_power_of_two()
}

/// Bytes padded with zeros up to a multiple of 4 (wgpu copy alignment).
pub fn padded_len(len: usize) -> usize {
    len.next_multiple_of(4)
}

/// The bytes of one frame packed for a single upload: every binding at an
/// aligned offset, each followed by its reserved slack, so one
/// `write_buffer` per frame replaces one per binding (each write allocates a
/// staging buffer, the main CPU cost of a frame on DX12).
#[derive(Debug, Default)]
pub struct Packer {
    bytes: Vec<u8>,
}

impl Packer {
    /// Starts a new frame (keeps the allocation).
    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    /// Places `data` at the next multiple of `align` and reserves `reserve`
    /// bytes for it (at least the data, padded to 4). Returns the binding's
    /// `(offset, size)`; the size is the reservation.
    pub fn place(&mut self, data: &[u8], reserve: u64, align: u64) -> (u64, u64) {
        let offset = (self.bytes.len() as u64).next_multiple_of(align.max(4));
        let size = reserve.max(padded_len(data.len()) as u64);
        self.bytes.resize(offset as usize, 0);
        self.bytes.extend_from_slice(data);
        self.bytes.resize((offset + size) as usize, 0);
        (offset, size)
    }

    /// The packed frame (a multiple of 4 bytes long).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Floats of the per-pass uniform block (4 x vec4).
pub const UNIFORM_FLOATS: usize = 16;

/// Uniform bytes for a pass: the supplied floats, zero padded (or cut) to the
/// fixed block. Second value is true when floats had to be dropped.
pub fn uniform_block(uniforms: &[f32]) -> ([f32; UNIFORM_FLOATS], bool) {
    let mut out = [0.0; UNIFORM_FLOATS];
    let n = uniforms.len().min(UNIFORM_FLOATS);
    out[..n].copy_from_slice(&uniforms[..n]);
    (out, uniforms.len() > UNIFORM_FLOATS)
}

/// Delay between the steps of a `Sit` transition (desktop-native 3.3 "Handover").
pub const HANDOVER: f64 = 0.1;

/// One staged step of a behind-windows transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    /// Attach the above panel to the render loop (`Split` on).
    AttachAbove,
    /// Move the main panel behind the application windows.
    PlaceBehind,
}

/// What `Sit(behind)` does for a screen (desktop-native 3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SitPlan {
    /// Going behind with an effect that has an above layer and a panel for it:
    /// attach after 0.1 s, place behind after 0.2 s.
    Staged,
    /// Place at once, detach the above panel (it is released after 0.1 s).
    Immediate,
}

pub fn sit_plan(behind: bool, draws_above: bool, above_ready: bool) -> SitPlan {
    if behind && draws_above && above_ready { SitPlan::Staged } else { SitPlan::Immediate }
}

/// `KeepOnTop` step 3 gate (3.4): re-raise the app's own topmost windows
/// whenever the screen is shown and the overlay is not behind, or an above
/// panel exists.
pub fn needs_raise(shown: bool, behind: bool, has_above: bool) -> bool {
    shown && (!behind || has_above)
}

/// One window met while walking the topmost z-order from the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub hwnd: isize,
    pub topmost: bool,
    pub visible: bool,
    pub own_thread: bool,
}

/// `RaisePanels` (3.4): windows to re-apply TOPMOST to, in apply order.
/// `panels` are the overlay's own panels, `under_dock` the host's windows
/// that stay below the taskbar.
pub fn raise_targets(seen: &[Seen], panels: &[isize], under_dock: &[isize]) -> Vec<isize> {
    let list: Vec<fp_tracker::TopWindow> = seen
        .iter()
        .map(|w| fp_tracker::TopWindow {
            hwnd: w.hwnd as i64,
            topmost: w.topmost,
            panel: panels.contains(&w.hwnd),
            under_dock: under_dock.contains(&w.hwnd),
            visible: w.visible,
            own_thread: w.own_thread,
        })
        .collect();
    fp_tracker::raise_list(&list).into_iter().map(|h| h as isize).collect()
}

#[derive(Clone, Copy, Debug)]
struct Item {
    due: f64,
    turn: u64,
    act: Act,
}

/// Turn-counter driven staging queue. Every `begin` supersedes the pending
/// steps of earlier transitions.
#[derive(Clone, Debug, Default)]
pub struct Stager {
    turn: u64,
    queue: Vec<Item>,
}

impl Stager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a transition; returns its turn.
    pub fn begin(&mut self) -> u64 {
        self.turn += 1;
        self.queue.clear();
        self.turn
    }

    /// Queues a step of transition `turn` for `now + delay`.
    pub fn at(&mut self, now: f64, delay: f64, turn: u64, act: Act) {
        self.queue.push(Item { due: now + delay, turn, act });
    }

    /// Steps that are due and still belong to the current transition.
    pub fn due(&mut self, now: f64) -> Vec<Act> {
        let turn = self.turn;
        let mut ready: Vec<Item> = Vec::new();
        self.queue.retain(|i| {
            if i.due <= now {
                ready.push(*i);
                false
            } else {
                true
            }
        });
        ready.sort_by(|a, b| a.due.total_cmp(&b.due));
        ready.into_iter().filter(|i| i.turn == turn).map(|i| i.act).collect()
    }

    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, px: PixelRect, scale: f64, primary: bool) -> MonitorInfo {
        MonitorInfo { id: id.into(), name: id.into(), pixels: px, work: px, scale, primary }
    }

    #[test]
    fn dip_sizing() {
        assert_eq!(swap_size(2560.0, 1440.0), (2560, 1440));
        assert_eq!(swap_size(1920.4, 1080.0), (1921, 1080));
        assert_eq!(swap_size(f64::NAN, 0.0), (1, 1));
        assert_eq!(swap_size(1e9, 20000.0), (16384, 16384));
        let s = visual_scale(PixelRect::new(0, 0, 3840, 2160), (2560, 1440));
        assert!((s.0 - 1.5).abs() < 1e-6 && (s.1 - 1.5).abs() < 1e-6);
        assert_eq!(visual_scale(PixelRect::new(0, 0, 0, 0), (0, 0)), (1.0, 1.0));
    }

    #[test]
    fn raise_gate() {
        assert!(needs_raise(true, false, false));
        assert!(needs_raise(true, true, true));
        assert!(!needs_raise(true, true, false));
        assert!(!needs_raise(false, false, true));
    }

    #[test]
    fn raise_targets_filter_and_order() {
        let s = |hwnd, topmost, visible, own_thread| Seen { hwnd, topmost, visible, own_thread };
        let seen = [
            s(1, true, true, true),
            s(2, true, true, true),
            s(3, true, true, true),
            s(4, true, false, true),
            s(5, true, true, false),
            s(6, true, true, true),
            s(7, false, true, true),
            s(8, true, true, true),
        ];
        // 2 = panel, 3 = under dock, 4 hidden, 5 foreign, 7 ends the walk.
        assert_eq!(raise_targets(&seen, &[2], &[3]), vec![6, 1]);
        assert!(raise_targets(&[], &[], &[]).is_empty());
    }

    #[test]
    fn dpi_values() {
        assert_eq!(creation_dpi(1.0), 96);
        assert_eq!(creation_dpi(1.5), 144);
        assert_eq!(creation_dpi(f64::NAN), 96);
        assert_eq!(creation_dpi(-2.0), 96);
    }

    #[test]
    fn cursor_mapping() {
        let r = PixelRect::new(1920, 0, 3840, 1080);
        assert_eq!(cursor_local(r, 1.0, 1920, 1079), Some(Vec2::new(0.0, 1.0)));
        assert_eq!(cursor_local(r, 2.0, 2020, 580), Some(Vec2::new(50.0, 250.0)));
        assert_eq!(cursor_local(r, f64::NAN, 1925, 1070), Some(Vec2::new(5.0, 10.0)));
        assert_eq!(cursor_local(r, 1.0, 1919, 10), None);
        assert_eq!(cursor_local(r, 1.0, 3840, 10), None);
        assert_eq!(cursor_local(r, 1.0, 2000, -1), None);
        assert_eq!(cursor_local(r, 1.0, 2000, 1080), None);
    }

    #[test]
    fn infos_follow_layout() {
        let infos = panel_infos(&[
            mon("b", PixelRect::new(2880, 0, 3880, 1000), 1.0, false),
            mon("a", PixelRect::new(0, 0, 2880, 1800), 2.0, true),
        ]);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].id, "b");
        assert_eq!(infos[1].index, 1);
        assert_eq!((infos[1].width, infos[1].height, infos[1].scale), (1440.0, 900.0, 2.0));
        assert_eq!(infos[1].frame, Frame::new(0.0, 0.0, 1440.0, 900.0));
        assert_eq!(infos[0].frame.x, 2880.0);
    }

    #[test]
    fn frame_matching() {
        let frames = [Frame::new(0.0, 0.0, 1920.0, 1080.0), Frame::new(1920.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(match_frame(&frames, &Frame::new(1920.0, 0.5, 1920.0, 1080.0)), Some(1));
        assert_eq!(match_frame(&frames, &Frame::new(0.2, 0.0, 1920.0, 1080.0)), Some(0));
        assert_eq!(match_frame(&frames, &Frame::new(100.0, 0.0, 1920.0, 1080.0)), None);
        assert_eq!(match_frame(&[], &frames[0]), None);
    }

    #[test]
    fn buffers() {
        assert_eq!(buffer_capacity(0), 256);
        assert_eq!(buffer_capacity(257), 512);
        assert_eq!(buffer_capacity(4096), 4096);
        assert_eq!(padded_len(5), 8);
        assert_eq!(padded_len(8), 8);
        let (u, cut) = uniform_block(&[1.0, 2.0]);
        assert_eq!((u[0], u[1], u[2], cut), (1.0, 2.0, 0.0, false));
        let (_, cut) = uniform_block(&[0.0; 20]);
        assert!(cut);
    }

    #[test]
    fn sit_plans() {
        assert_eq!(sit_plan(true, true, true), SitPlan::Staged);
        assert_eq!(sit_plan(true, false, true), SitPlan::Immediate);
        assert_eq!(sit_plan(true, true, false), SitPlan::Immediate);
        assert_eq!(sit_plan(false, true, true), SitPlan::Immediate);
    }

    #[test]
    fn staging_runs_in_order() {
        let mut s = Stager::new();
        let t = s.begin();
        s.at(0.0, HANDOVER, t, Act::AttachAbove);
        s.at(0.0, 2.0 * HANDOVER, t, Act::PlaceBehind);
        assert!(s.pending());
        assert!(s.due(0.05).is_empty());
        assert_eq!(s.due(0.1), vec![Act::AttachAbove]);
        assert_eq!(s.due(0.15), vec![]);
        assert_eq!(s.due(0.25), vec![Act::PlaceBehind]);
        assert!(!s.pending());
    }

    #[test]
    fn staging_catches_up_and_supersedes() {
        let mut s = Stager::new();
        let t = s.begin();
        s.at(0.0, 0.2, t, Act::PlaceBehind);
        s.at(0.0, 0.1, t, Act::AttachAbove);
        assert_eq!(s.due(1.0), vec![Act::AttachAbove, Act::PlaceBehind]);
        // A newer transition drops the old steps; stale steps never fire.
        let t1 = s.begin();
        s.at(0.0, 0.1, t1, Act::AttachAbove);
        let t2 = s.begin();
        assert!(!s.pending());
        s.at(0.0, 0.1, t1, Act::PlaceBehind);
        s.at(0.0, 0.1, t2, Act::AttachAbove);
        assert_eq!(s.due(1.0), vec![Act::AttachAbove]);
    }

    #[test]
    fn packer_aligns_and_reserves() {
        let mut p = Packer::default();
        assert_eq!(p.place(&[1, 2, 3, 4], 0, 256), (0, 4));
        // Next binding at the alignment; the reservation exceeds the data.
        assert_eq!(p.place(&[5; 6], 512, 256), (256, 512));
        // Odd data is padded to 4 when the reservation is smaller.
        assert_eq!(p.place(&[7; 5], 0, 0), (768, 8));
        assert_eq!(p.bytes().len(), 776);
        assert_eq!(&p.bytes()[256..263], &[5, 5, 5, 5, 5, 5, 0]);
        assert_eq!(p.bytes()[768..776], [7, 7, 7, 7, 7, 0, 0, 0]);
        p.clear();
        assert!(p.bytes().is_empty());
        assert_eq!(p.place(&[], 16, 256), (0, 16));
    }
}
