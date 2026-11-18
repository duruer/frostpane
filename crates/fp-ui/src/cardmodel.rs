//! Pure geometry, timing and state of the cards and the city search
//! (ui 3.9 ShownCard/QuoteCard, snowman F4/F9, trees 3.9).

use fp_core::desktop::Layout;
use fp_core::geom::{Frame, Vec2};
use fp_core::scene::{Owner, Sprite};
use fp_core::settings::City;

/// Shadow gutter of the cards, per side (ShownCard).
pub const GUTTER: f64 = 24.0;
/// Quote card width (ui 3.9).
pub const QUOTE_W: f64 = 340.0;
/// Gap between the gift point and the quote card top.
pub const QUOTE_DROP: f64 = 44.0;
/// Quote card margin to the screen edge.
pub const QUOTE_EDGE: f64 = 12.0;

/// Quote card left edge: centred on `x`, kept `QUOTE_EDGE` inside the screen.
pub fn quote_left(x: f64, screen_left: f64, screen_right: f64) -> f64 {
    let lo = screen_left + QUOTE_EDGE;
    let hi = (screen_right - QUOTE_W - QUOTE_EDGE).max(lo);
    (x - QUOTE_W / 2.0).clamp(lo, hi)
}

/// Quote card top edge in Y-up points: 44 below the point.
pub fn quote_top(point_y: f64) -> f64 {
    point_y - QUOTE_DROP
}

/// Seconds until the quote card fades by itself: `10 + length / 12`.
pub fn quote_auto_fade(text_len: usize) -> f64 {
    10.0 + text_len as f64 / 12.0
}

/// Linear fade in, then (after `fade()`) linear fade out; re-entrant fades
/// are ignored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub fade_in: f64,
    pub fade_out: f64,
    born: f64,
    dying: Option<f64>,
}

impl Fade {
    /// Generic shown card: 0.3 s in, 0.25 s out.
    pub fn card(now: f64) -> Self {
        Self { fade_in: 0.3, fade_out: 0.25, born: now, dying: None }
    }

    /// Quote card: 0.25 s in, 0.3 s out.
    pub fn quote(now: f64) -> Self {
        Self { fade_in: 0.25, fade_out: 0.3, born: now, dying: None }
    }

    pub fn fade(&mut self, now: f64) {
        if self.dying.is_none() {
            self.dying = Some(now);
        }
    }

    pub fn is_fading(&self) -> bool {
        self.dying.is_some()
    }

    /// Opacity 0..=1 at `now`.
    pub fn opacity(&self, now: f64) -> f32 {
        let up = if self.fade_in <= 0.0 { 1.0 } else { ((now - self.born) / self.fade_in).clamp(0.0, 1.0) };
        let down = match self.dying {
            Some(at) if self.fade_out > 0.0 => 1.0 - ((now - at) / self.fade_out).clamp(0.0, 1.0),
            Some(_) => 0.0,
            None => 1.0,
        };
        (up.min(down)) as f32
    }

    /// True once the fade-out has finished.
    pub fn finished(&self, now: f64) -> bool {
        self.dying.is_some_and(|at| now - at >= self.fade_out)
    }
}

/// Top-left of a window, from a global Y-up top-left point, as egui logical
/// screen coordinates (Y down): pixels of the monitor holding the point,
/// divided by that monitor's scale.
pub fn window_pos(layout: &Layout, top_left: Vec2) -> (f32, f32) {
    if layout.is_empty() {
        return (top_left.x as f32, -top_left.y as f32);
    }
    let (px, py) = layout.to_pixels(top_left);
    let scale = layout.at_point(top_left).map_or(1.0, |s| s.scale()).max(0.1);
    ((px / scale) as f32, (py / scale) as f32)
}

/// Work-area frame of the monitor holding `p` (else the primary's, else none).
pub fn work_at(layout: &Layout, p: Vec2) -> Option<Frame> {
    layout.at_point(p).or_else(|| layout.primary()).map(|s| s.work_points)
}

/// Bounding box `[min_x, min_y, max_x, max_y]` of the snowman stack whose
/// bottom ball is `bottom`: the bottom ball's window plus every ball window
/// above it that overlaps it horizontally.
pub fn stack_box(sprites: &[Sprite], bottom: u64) -> Option<[f64; 4]> {
    let base = sprites.iter().find(|s| s.key.owner_byte() == Owner::Ball as u8 && s.key.id() == bottom)?;
    let b = base.frame;
    let mut r = [b.min_x(), b.min_y(), b.max_x(), b.max_y()];
    for s in sprites {
        let f = s.frame;
        if s.key.owner_byte() == Owner::Ball as u8
            && s.key != base.key
            && f.min_y() >= b.min_y()
            && f.max_x() > b.min_x()
            && f.min_x() < b.max_x()
        {
            r = [r[0].min(f.min_x()), r[1].min(f.min_y()), r[2].max(f.max_x()), r[3].max(f.max_y())];
        }
    }
    Some(r)
}

/// Limit snowman window (F9): flush with the bottom-right corner of the work
/// area (x = maxX - w, bottom = minY, Y up). Returns the top-left (Y up).
pub fn limit_top_left(work: Frame, size: (f64, f64)) -> Vec2 {
    Vec2::new(work.max_x() - size.0, work.min_y() + size.1)
}

/// Limit snowman window size in points (snowman F9).
pub const LIMIT_SIZE: (f64, f64) = (322.0, 176.0);
/// Puppet rest spot, measured from the window's right edge.
pub const LIMIT_REST: f64 = 34.0;
/// Puppet start and exit spot from the right edge (just outside the window).
pub const LIMIT_EXIT: f64 = -50.0;
/// The bubble pops this long after the window opens ...
pub const LIMIT_BUBBLE_AT: f64 = 0.8;
/// ... stays this long ...
pub const LIMIT_BUBBLE_FOR: f64 = 4.5;
/// ... and the window closes this long after the bubble hides.
pub const LIMIT_CLOSE_AFTER: f64 = 1.3;
/// Bubble fade in/out time.
pub const LIMIT_BUBBLE_FADE: f64 = 0.2;

/// Seconds from opening until the limit snowman leaves (bubble hides).
pub fn limit_leave_at() -> f64 {
    LIMIT_BUBBLE_AT + LIMIT_BUBBLE_FOR
}

/// Seconds from opening until the limit window closes.
pub fn limit_total() -> f64 {
    limit_leave_at() + LIMIT_CLOSE_AFTER
}

/// Puppet goal (distance from the window's right edge) at `age` seconds:
/// the rest spot until it leaves, then the exit spot.
pub fn limit_goal_from_right(age: f64) -> f64 {
    if age < limit_leave_at() { LIMIT_REST } else { LIMIT_EXIT }
}

/// Bubble opacity at `age`: 0 before the pop, a 0.2 s fade in, held, then a
/// 0.2 s fade out when the snowman leaves.
pub fn limit_bubble_alpha(age: f64) -> f64 {
    if !age.is_finite() {
        return 0.0;
    }
    let up = ((age - LIMIT_BUBBLE_AT) / LIMIT_BUBBLE_FADE).clamp(0.0, 1.0);
    let down = 1.0 - ((age - limit_leave_at()) / LIMIT_BUBBLE_FADE).clamp(0.0, 1.0);
    up.min(down)
}

/// Station bubble: centred horizontally on `at`, bottom 24 pt above it,
/// clamped into the work area. Returns the top-left (Y up).
pub fn station_top_left(at: Vec2, size: (f64, f64), work: Frame) -> Vec2 {
    let x = (at.x - size.0 / 2.0).clamp(work.min_x() + 8.0, (work.max_x() - size.0 - 8.0).max(work.min_x() + 8.0));
    let top = (at.y + 24.0 + size.1).min(work.max_y() - 8.0).max(work.min_y() + size.1 + 8.0);
    Vec2::new(x, top)
}

/// Decoration picker (decorations 3.1): near the pointer. `size` is the whole
/// window including the 18 pt shadow margin. X follows the cursor (58 pt left
/// of it, clamped into the work area with the right bound winning); with a
/// taskbar on top the window hangs 12 pt below the work-area top, otherwise it
/// sits 12 pt above the work-area bottom. Returns the top-left (Y up).
pub fn picker_top_left(cursor: Vec2, screen: Frame, work: Frame, size: (f64, f64)) -> Vec2 {
    let (w, h) = size;
    let x = (cursor.x - 58.0).max(work.min_x() - 10.0).min(work.max_x() - w + 10.0);
    let top = if work.max_y() < screen.max_y() { work.max_y() + 12.0 } else { work.min_y() - 12.0 + h };
    Vec2::new(x.round(), top.round())
}

/// One-line label of a city: name, region, country.
pub fn city_label(c: &City) -> String {
    [c.name.as_str(), c.admin.as_str(), c.country.as_str()]
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ")
}

/// Search box state of the city picker.
#[derive(Clone, Debug, PartialEq)]
pub enum CityState {
    Idle,
    Searching(String),
    Results(Vec<City>),
    NoMatch,
    Offline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CitySearch {
    pub query: String,
    pub state: CityState,
}

/// Shortest query worth sending.
pub const MIN_QUERY: usize = 2;

impl Default for CitySearch {
    fn default() -> Self {
        Self { query: String::new(), state: CityState::Idle }
    }
}

impl CitySearch {
    pub fn can_search(&self) -> bool {
        self.query.trim().chars().count() >= MIN_QUERY && !matches!(self.state, CityState::Searching(_))
    }

    /// Start a search; returns the query to send, if any.
    pub fn begin(&mut self) -> Option<String> {
        if !self.can_search() {
            return None;
        }
        let q = self.query.trim().to_string();
        self.state = CityState::Searching(q.clone());
        Some(q)
    }

    /// Store a reply. A reply for a query other than the running one is stale
    /// and ignored.
    pub fn finish(&mut self, query: &str, result: Result<Vec<City>, ()>) {
        if self.state != CityState::Searching(query.to_string()) {
            return;
        }
        self.state = match result {
            Ok(v) if v.is_empty() => CityState::NoMatch,
            Ok(v) => CityState::Results(v),
            Err(()) => CityState::Offline,
        };
    }

    /// The user picked result `i`; clears the list.
    pub fn pick(&mut self, i: usize) -> Option<City> {
        let CityState::Results(list) = &self.state else { return None };
        let c = list.get(i).cloned()?;
        self.state = CityState::Idle;
        self.query.clear();
        Some(c)
    }
}

// ---- tray pointer (core-app 2.7) ----

/// Tray pointer window size in points.
pub const POINTER_SIZE: (f64, f64) = (240.0, 170.0);
/// Seconds the pointer stays before it fades out.
pub const POINTER_SHOW: f64 = 5.0;
const POINTER_FADE_IN: f64 = 0.2;
const POINTER_FADE_OUT: f64 = 0.4;

/// Placement of the tray pointer, all in global points (Y up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerPlan {
    /// Window top-left.
    pub top_left: Vec2,
    /// Taskbar on top: the arrow points up and the label sits below the icon.
    pub below: bool,
    /// Ring centre, relative to the window top-left (x right, y down).
    pub ring: (f64, f64),
    /// Ring side: `max(icon w, icon h) + 14`.
    pub ring_side: f64,
}

/// Plan the pointer for the icon rect `icon` (global points, Y up). `work` is
/// the icon's monitor work area, `all` the bounds of every monitor. The
/// caller has already checked the icon is inside the taskbar window.
pub fn pointer_plan(icon: Frame, work: Frame, all: Frame) -> PointerPlan {
    let below = icon.max_y() > work.y + work.h / 2.0;
    let (w, h) = POINTER_SIZE;
    let hi = (all.max_x() - w).max(all.min_x());
    let left = (icon.mid_x() - w / 2.0).clamp(all.min_x(), hi);
    let top = if below { icon.max_y() } else { icon.min_y() + h };
    let c = icon.center();
    PointerPlan {
        top_left: Vec2::new(left, top),
        below,
        ring: (c.x - left, top - c.y),
        ring_side: icon.w.max(icon.h) + 14.0,
    }
}

/// Window opacity `t` seconds after showing: 0 to 1 over 0.2 s, held until
/// 5 s, then 1 to 0 over 0.4 s. `None` once the window should close.
pub fn pointer_opacity(t: f64) -> Option<f64> {
    if t.is_nan() || t < 0.0 {
        return Some(0.0);
    }
    if t >= POINTER_SHOW + POINTER_FADE_OUT {
        return None;
    }
    Some(if t < POINTER_FADE_IN {
        t / POINTER_FADE_IN
    } else if t < POINTER_SHOW {
        1.0
    } else {
        1.0 - (t - POINTER_SHOW) / POINTER_FADE_OUT
    })
}

/// Pulse ring `index` (0 or 1) at `t`: `(scale, opacity)`. Scale 1 to 1.9
/// (quadratic ease-out), opacity 0.9 to 0 over 1.3 s, repeating; ring 1
/// starts 0.65 s later and is invisible before its first cycle.
pub fn pointer_ring(index: usize, t: f64) -> (f64, f64) {
    const CYCLE: f64 = 1.3;
    let t = t - if index == 0 { 0.0 } else { 0.65 };
    if t.is_nan() || t < 0.0 {
        return (1.0, 0.0);
    }
    let p = (t % CYCLE) / CYCLE;
    let eased = 1.0 - (1.0 - p) * (1.0 - p);
    (1.0 + 0.9 * eased, 0.9 * (1.0 - p))
}

/// Vertical bob of the arrow and label (points, y down): from 8 pt away
/// from the icon to 0 over 0.55 s, sine ease-in-out, back and forth.
pub fn pointer_bob(below: bool, t: f64) -> f64 {
    const HALF: f64 = 0.55;
    if !t.is_finite() || t < 0.0 {
        return if below { 8.0 } else { -8.0 };
    }
    let phase = (t % (2.0 * HALF)) / HALF;
    let p = if phase <= 1.0 { phase } else { 2.0 - phase };
    let eased = 0.5 - 0.5 * (std::f64::consts::PI * p).cos();
    let start = if below { 8.0 } else { -8.0 };
    start * (1.0 - eased)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::desktop::MonitorInfo;
    use fp_core::geom::PixelRect;
    use fp_core::scene::{HitMode, SpriteKey, ZBand};

    fn city(name: &str, admin: &str, country: &str) -> City {
        City { name: name.into(), admin: admin.into(), country: country.into(), latitude: 1.0, longitude: 2.0 }
    }

    fn layout() -> Layout {
        Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "A".into(),
            pixels: PixelRect { left: 0, top: 0, right: 2000, bottom: 1000 },
            work: PixelRect { left: 0, top: 0, right: 2000, bottom: 960 },
            scale: 2.0,
            primary: true,
        }])
    }

    fn ball(id: u64, x: f64, y: f64, w: f64, h: f64) -> Sprite {
        Sprite {
            key: SpriteKey::new(Owner::Ball, id),
            frame: Frame::new(x, y, w, h),
            z: ZBand::Front,
            hit: HitMode::Alpha,
            rev: 0,
            opacity: 1.0,
        }
    }

    #[test]
    fn quote_geometry() {
        assert_eq!(quote_left(500.0, 0.0, 1920.0), 330.0);
        assert_eq!(quote_left(10.0, 0.0, 1920.0), 12.0);
        assert_eq!(quote_left(1900.0, 0.0, 1920.0), 1920.0 - 340.0 - 12.0);
        // A screen narrower than the card: left bound wins, no panic.
        assert_eq!(quote_left(100.0, 0.0, 200.0), 12.0);
        assert_eq!(quote_top(300.0), 256.0);
        assert!((quote_auto_fade(120) - 20.0).abs() < 1e-12);
    }

    #[test]
    fn fade_runs_in_then_out_once() {
        let mut f = Fade::card(10.0);
        assert_eq!(f.opacity(10.0), 0.0);
        assert!((f.opacity(10.15) - 0.5).abs() < 1e-6);
        assert_eq!(f.opacity(11.0), 1.0);
        assert!(!f.is_fading() && !f.finished(11.0));
        f.fade(12.0);
        f.fade(12.1); // re-entrant: ignored
        assert!(f.is_fading());
        assert!((f.opacity(12.125) - 0.5).abs() < 1e-6);
        assert!(!f.finished(12.2) && f.finished(12.25));
        assert_eq!(f.opacity(13.0), 0.0);
        let q = Fade::quote(0.0);
        assert_eq!((q.fade_in, q.fade_out), (0.25, 0.3));
        let z = Fade { fade_in: 0.0, fade_out: 0.0, born: 0.0, dying: None };
        assert_eq!(z.opacity(0.0), 1.0);
        let mut z = z;
        z.fade(1.0);
        assert_eq!(z.opacity(1.0), 0.0);
    }

    #[test]
    fn window_position_uses_pixels_and_scale() {
        let l = layout();
        // Primary top = 1000 / 2 = 500 points; global (100, 400) -> pixel (200, 200) -> logical (100, 100).
        assert_eq!(window_pos(&l, Vec2::new(100.0, 400.0)), (100.0, 100.0));
        let empty = Layout::new(vec![]);
        assert_eq!(window_pos(&empty, Vec2::new(5.0, 7.0)), (5.0, -7.0));
        assert!(work_at(&l, Vec2::new(10.0, 10.0)).is_some());
        assert!(work_at(&empty, Vec2::new(10.0, 10.0)).is_none());
        // A point outside every monitor falls back to the primary.
        assert!(work_at(&l, Vec2::new(-5000.0, 10.0)).is_some());
    }

    #[test]
    fn stack_box_unions_overlapping_balls() {
        let s = vec![
            ball(1, 100.0, 0.0, 60.0, 60.0),
            ball(2, 105.0, 55.0, 50.0, 50.0),
            ball(3, 400.0, 0.0, 60.0, 60.0),
            Sprite { key: SpriteKey::new(Owner::Tree, 9), ..ball(9, 100.0, 0.0, 900.0, 900.0) },
        ];
        assert_eq!(stack_box(&s, 1), Some([100.0, 0.0, 160.0, 105.0]));
        assert_eq!(stack_box(&s, 3), Some([400.0, 0.0, 460.0, 60.0]));
        assert_eq!(stack_box(&s, 7), None);
    }

    #[test]
    fn limit_timeline() {
        assert!((limit_leave_at() - 5.3).abs() < 1e-12);
        assert!((limit_total() - 6.6).abs() < 1e-12);
        assert_eq!(limit_goal_from_right(0.0), LIMIT_REST);
        assert_eq!(limit_goal_from_right(5.29), LIMIT_REST);
        assert_eq!(limit_goal_from_right(5.3), LIMIT_EXIT);
        assert_eq!(limit_bubble_alpha(0.0), 0.0);
        assert_eq!(limit_bubble_alpha(0.8), 0.0);
        assert!((limit_bubble_alpha(0.9) - 0.5).abs() < 1e-9);
        assert_eq!(limit_bubble_alpha(3.0), 1.0);
        assert!((limit_bubble_alpha(5.4) - 0.5).abs() < 1e-9);
        assert_eq!(limit_bubble_alpha(5.5), 0.0);
        assert_eq!(limit_bubble_alpha(f64::NAN), 0.0);
    }

    #[test]
    fn corner_windows() {
        let work = Frame::new(0.0, 0.0, 1000.0, 600.0);
        assert_eq!(limit_top_left(work, (300.0, 150.0)), Vec2::new(700.0, 150.0));
        let screen = Frame::new(0.0, 0.0, 1000.0, 640.0);
        // Taskbar on top (work.max_y < screen.max_y): hangs below it.
        assert_eq!(picker_top_left(Vec2::new(500.0, 300.0), screen, work, (400.0, 500.0)), Vec2::new(442.0, 612.0));
        // Taskbar on the bottom: sits above it.
        let wb = Frame::new(0.0, 40.0, 1000.0, 600.0);
        assert_eq!(picker_top_left(Vec2::new(500.0, 300.0), screen, wb, (400.0, 500.0)), Vec2::new(442.0, 528.0));
        // Cursor clamps left and right.
        assert_eq!(picker_top_left(Vec2::new(0.0, 0.0), screen, wb, (400.0, 500.0)).x, -10.0);
        assert_eq!(picker_top_left(Vec2::new(990.0, 0.0), screen, wb, (400.0, 500.0)).x, 610.0);
        // Window wider than the work area: the right bound wins.
        assert_eq!(picker_top_left(Vec2::new(0.0, 0.0), screen, wb, (1100.0, 500.0)).x, -90.0);
        let st = station_top_left(Vec2::new(500.0, 100.0), (200.0, 100.0), work);
        assert_eq!(st, Vec2::new(400.0, 224.0));
        // Clamped at the edges.
        let st = station_top_left(Vec2::new(5.0, 590.0), (200.0, 100.0), work);
        assert_eq!(st.x, 8.0);
        assert_eq!(st.y, 592.0);
        let st = station_top_left(Vec2::new(990.0, 0.0), (200.0, 100.0), work);
        assert_eq!(st.x, 792.0);
        let tiny = station_top_left(Vec2::new(0.0, 0.0), (900.0, 100.0), Frame::new(0.0, 0.0, 500.0, 600.0));
        assert_eq!(tiny.x, 8.0);
    }

    #[test]
    fn city_labels() {
        assert_eq!(city_label(&city("Oslo", "Oslo", "Norway")), "Oslo, Oslo, Norway");
        assert_eq!(city_label(&city("Nome", "", "USA")), "Nome, USA");
    }

    #[test]
    fn city_search_flow() {
        let mut s = CitySearch::default();
        assert!(!s.can_search());
        assert_eq!(s.begin(), None);
        s.query = " O ".into();
        assert!(!s.can_search());
        s.query = "  Oslo ".into();
        assert!(s.can_search());
        assert_eq!(s.begin().as_deref(), Some("Oslo"));
        assert_eq!(s.state, CityState::Searching("Oslo".into()));
        assert!(!s.can_search());
        // Stale reply is ignored.
        s.finish("Osl", Ok(vec![city("X", "", "")]));
        assert_eq!(s.state, CityState::Searching("Oslo".into()));
        s.finish("Oslo", Ok(vec![city("Oslo", "Oslo", "Norway"), city("Oslo", "MN", "USA")]));
        assert!(matches!(&s.state, CityState::Results(v) if v.len() == 2));
        assert_eq!(s.pick(5), None);
        let c = s.pick(1).unwrap();
        assert_eq!(c.admin, "MN");
        assert_eq!(s.state, CityState::Idle);
        assert!(s.query.is_empty());
        assert_eq!(s.pick(0), None);
        s.query = "Zzz".into();
        s.begin();
        s.finish("Zzz", Ok(vec![]));
        assert_eq!(s.state, CityState::NoMatch);
        s.begin();
        s.finish("Zzz", Err(()));
        assert_eq!(s.state, CityState::Offline);
    }

    #[test]
    fn tray_pointer_plan_and_timeline() {
        let work = Frame::new(0.0, 48.0, 1920.0, 1032.0);
        let all = Frame::new(0.0, 0.0, 1920.0, 1080.0);
        // Taskbar at the bottom: label above, window bottom on the icon bottom.
        let icon = Frame::new(1800.0, 8.0, 24.0, 32.0);
        let p = pointer_plan(icon, work, all);
        assert!(!p.below);
        assert_eq!(p.top_left, Vec2::new(1680.0, 178.0));
        assert_eq!(p.ring, (132.0, 154.0));
        assert_eq!(p.ring_side, 46.0);
        // Near the right edge the window is clamped inside the screens.
        let edge = pointer_plan(Frame::new(1900.0, 8.0, 16.0, 16.0), work, all);
        assert_eq!(edge.top_left.x, 1680.0);
        // Taskbar on top.
        let top_work = Frame::new(0.0, 0.0, 1920.0, 1032.0);
        let up = pointer_plan(Frame::new(100.0, 1040.0, 24.0, 32.0), top_work, all);
        assert!(up.below);
        assert_eq!(up.top_left.y, 1072.0);
        // Auto-hide taskbar: work area is the whole monitor, icon still gets a pointer.
        assert!(pointer_plan(Frame::new(900.0, 1060.0, 24.0, 20.0), all, all).below);

        assert_eq!(pointer_opacity(-1.0), Some(0.0));
        assert_eq!(pointer_opacity(f64::NAN), Some(0.0));
        assert_eq!(pointer_opacity(0.1), Some(0.5));
        assert_eq!(pointer_opacity(2.0), Some(1.0));
        assert!((pointer_opacity(5.2).unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(pointer_opacity(5.4), None);

        assert_eq!(pointer_ring(1, 0.3), (1.0, 0.0));
        assert_eq!(pointer_ring(0, f64::NAN), (1.0, 0.0));
        assert_eq!(pointer_ring(0, 0.0), (1.0, 0.9));
        let (s, o) = pointer_ring(1, 0.65 + 0.65);
        assert!((s - 1.675).abs() < 1e-9 && (o - 0.45).abs() < 1e-9);

        assert_eq!(pointer_bob(true, 0.0), 8.0);
        assert_eq!(pointer_bob(false, -1.0), -8.0);
        assert!(pointer_bob(true, 0.55).abs() < 1e-9);
        assert!((pointer_bob(false, 1.1) + 8.0).abs() < 1e-9);
    }
}
