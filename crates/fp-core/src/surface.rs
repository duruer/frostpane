//! Tracked windows as landing surfaces (desktop-native 3.5.5, snow-sim 3.9,
//! snowballs 3.4). Surface frames are screen-local points, Y up.

use crate::geom::{Frame, Rect4, Vec2};

/// Identity of a ledge on one screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SurfaceId {
    /// The taskbar shelf (raw id 0).
    Dock,
    /// The bottom of the screen (raw id `i64::MAX`).
    Ground,
    /// Another application's top-level window (raw id = HWND value).
    Window(i64),
}

impl SurfaceId {
    pub fn raw(self) -> i64 {
        match self {
            SurfaceId::Dock => 0,
            SurfaceId::Ground => i64::MAX,
            SurfaceId::Window(h) => h,
        }
    }

    pub fn from_raw(v: i64) -> Self {
        match v {
            0 => SurfaceId::Dock,
            i64::MAX => SurfaceId::Ground,
            h => SurfaceId::Window(h),
        }
    }
}

/// A window top (or the dock) that weather can land on.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub id: SurfaceId,
    /// Screen-local frame of the window.
    pub frame: Frame,
    /// Higher-z windows overlapping the ledge top band (screen-local).
    pub blockers: Vec<Rect4>,
    /// Higher-z windows overlapping anything above the ledge (screen-local).
    pub covers: Vec<Rect4>,
    pub corner_radius: f64,
    /// Owning process name (empty for dock/ground); used to re-find windows
    /// across restarts (snowballs 3.10, trees 3.15).
    pub owner: String,
}

impl Surface {
    pub fn new(id: SurfaceId, frame: Frame) -> Self {
        Self { id, frame, blockers: Vec::new(), covers: Vec::new(), corner_radius: 0.0, owner: String::new() }
    }
}

/// `true` if the point is inside any rect (half-open). EffectMath `Blocked`.
pub fn blocked(rects: &[Rect4], x: f64, y: f64) -> bool {
    rects.iter().any(|r| r.contains(x, y))
}

/// Per-screen snapshot published by the window tracker every poll.
#[derive(Clone, Debug, PartialEq)]
pub struct ScreenSnap {
    /// Screen frame in global points.
    pub frame: Frame,
    /// Overlay visible (not covered by a full-screen window, not asleep).
    pub shown: bool,
    /// Ground visible (no maximised window fills the screen).
    pub ground: bool,
    pub surfaces: Vec<Surface>,
}

/// Ledge identity across screens: `(screen origin, surface id)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedgeKey {
    pub origin: Vec2,
    pub id: SurfaceId,
}

impl LedgeKey {
    pub fn new(origin: Vec2, id: SurfaceId) -> Self {
        Self { origin, id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for id in [SurfaceId::Dock, SurfaceId::Ground, SurfaceId::Window(42)] {
            assert_eq!(SurfaceId::from_raw(id.raw()), id);
        }
    }

    #[test]
    fn blocked_and_new() {
        let mut s = Surface::new(SurfaceId::Window(1), Frame::new(0.0, 0.0, 10.0, 10.0));
        s.blockers.push(Rect4::new(0.0, 0.0, 5.0, 5.0));
        assert!(blocked(&s.blockers, 1.0, 1.0));
        assert!(!blocked(&s.blockers, 6.0, 1.0));
        let k = LedgeKey::new(Vec2::ZERO, s.id);
        assert_eq!(k.id, SurfaceId::Window(1));
    }
}
