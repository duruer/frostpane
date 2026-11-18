//! Screen lists and small geometry helpers (characters 0.1, misc 4).

use fp_characters::StageScreen;
use fp_core::desktop::Layout;
use fp_core::geom::{Frame, Vec2};
use fp_core::surface::{ScreenSnap, SurfaceId};

/// What the characters see of each snapshot: `shown` means the monitor is
/// shown and its ground layer is visible; the dock is the taskbar ledge in
/// global points `(min_x, max_x, top)`.
pub fn stage_screens(snaps: &[ScreenSnap]) -> Vec<StageScreen> {
    snaps
        .iter()
        .map(|s| {
            let origin = s.frame.min();
            let dock = s.surfaces.iter().find(|u| u.id == SurfaceId::Dock).map(|u| {
                let g = u.frame.offset(origin.x, origin.y);
                (g.min_x(), g.max_x(), g.max_y())
            });
            StageScreen { frame: s.frame, shown: s.shown && s.ground, dock }
        })
        .collect()
}

/// The three screen lists of characters 0.1: `(visit, plough, shown)`.
/// With a main screen that is currently shown, visits use only it and the
/// plough prefers it; otherwise every screen is used.
pub fn screen_lists(
    all: &[StageScreen],
    main: Option<Frame>,
) -> (Vec<StageScreen>, Vec<StageScreen>, Vec<StageScreen>) {
    let main = main.and_then(|m| all.iter().find(|s| s.frame == m && s.shown).copied());
    match main {
        Some(m) => {
            let mut plough = vec![m];
            plough.extend(all.iter().filter(|s| s.frame != m.frame).copied());
            (vec![m], plough, all.to_vec())
        }
        None => (all.to_vec(), all.to_vec(), all.to_vec()),
    }
}

/// The snapshot frame nearest to a global point (the one containing it wins).
pub fn nearest_frame(snaps: &[ScreenSnap], at: Vec2) -> Option<Frame> {
    snaps.iter().map(|s| s.frame).min_by(|a, b| a.dist_sq(at).total_cmp(&b.dist_sq(at)))
}

/// Work area of the main screen, else of the primary monitor.
pub fn work_area(layout: &Layout, main: Option<Frame>) -> Option<Frame> {
    let chosen = main.and_then(|m| layout.screens().iter().find(|s| s.points == m));
    chosen.or(layout.primary()).map(|s| s.work_points)
}

/// Work area of the primary monitor (limit snowman, trees 3.12).
pub fn primary_work(layout: &Layout) -> Option<Frame> {
    layout.primary().map(|s| s.work_points)
}

/// Frames of every monitor in global points.
pub fn monitor_frames(layout: &Layout) -> Vec<Frame> {
    layout.screens().iter().map(|s| s.points).collect()
}

/// Window tops as global frames: obstacles for the decoration spot.
pub fn window_frames(snaps: &[ScreenSnap]) -> Vec<Frame> {
    let mut out = Vec::new();
    for s in snaps {
        let origin = s.frame.min();
        for u in s.surfaces.iter().filter(|u| matches!(u.id, SurfaceId::Window(_))) {
            out.push(u.frame.offset(origin.x, origin.y));
        }
    }
    out
}
