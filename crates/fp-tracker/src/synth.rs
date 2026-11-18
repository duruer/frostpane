//! [`HookEvent`]s synthesised from successive polls, for backends without
//! WinEvent-style hooks (porting design 3.1 "Hooks" and "Spaces").
//!
//! Bounds change -> `MoveSizeStart`, every moved window stable for
//! [`SETTLE`] seconds -> `MoveSizeEnd`, id gone -> `Destroy`. The drag events
//! carry no id, so the drag is global: it starts with the first moving window
//! and ends when none is moving any more. Time is passed in, never read.

use std::collections::BTreeMap;

use fp_core::geom::PixelRect;

use crate::{HookEvent, WindowInfo};

/// Seconds a moved window must keep its bounds before the drag ends.
pub const SETTLE: f64 = 0.3;

/// Poll differ; one per window source.
#[derive(Debug, Default)]
pub struct HookSynth {
    bounds: BTreeMap<i64, Option<PixelRect>>,
    /// Moving windows -> time of their last bounds change.
    moving: BTreeMap<i64, f64>,
    space_pending: bool,
    cloaked: Vec<i64>,
    foreground: Option<i64>,
}

impl HookSynth {
    pub fn new() -> Self {
        Self::default()
    }

    /// Diff one listing (any z-order) against the previous one. Events come
    /// in the order `MoveSizeStart`, `Destroy`s (by id), `MoveSizeEnd`.
    pub fn poll(&mut self, now: f64, windows: &[WindowInfo]) -> Vec<HookEvent> {
        let was_moving = !self.moving.is_empty();
        let mut bounds = BTreeMap::new();
        for w in windows {
            if self.bounds.get(&w.hwnd).is_some_and(|old| *old != w.bounds) {
                self.moving.insert(w.hwnd, now);
            }
            bounds.insert(w.hwnd, w.bounds);
        }
        let space = std::mem::take(&mut self.space_pending);
        let mut events = Vec::new();
        for id in self.bounds.keys().filter(|id| !bounds.contains_key(id)) {
            if space {
                self.cloaked.push(*id);
            } else {
                events.push(HookEvent::Destroy(*id));
            }
        }
        self.bounds = bounds;
        // NaN or a clock that went backwards past the settle time ends the drag.
        self.moving.retain(|id, at| self.bounds.contains_key(id) && now - *at < SETTLE);
        let moving = !self.moving.is_empty();
        if moving && !was_moving {
            events.insert(0, HookEvent::MoveSizeStart);
        }
        if was_moving && !moving {
            events.push(HookEvent::MoveSizeEnd);
        }
        events
    }

    /// The active space or virtual desktop changed: windows missing from the
    /// next poll went to another space, so they are kept for
    /// [`take_cloaked`](Self::take_cloaked) instead of reported as `Destroy`.
    pub fn space_switched(&mut self) {
        self.space_pending = true;
    }

    /// Ids that vanished in a poll after [`space_switched`](Self::space_switched),
    /// oldest first; drains the list. `HookEvent` has no cloak event, so the
    /// backend lists these as cloaked windows (`WindowInfo::portable` with
    /// `cloaked: true`) in that listing; the tracker's `cloaked_known` count
    /// then sees the switch (two cloaks = `to_other_space`).
    pub fn take_cloaked(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.cloaked)
    }

    /// The active window is now `id`; `Foreground` when it changed.
    pub fn foreground(&mut self, id: i64) -> Option<HookEvent> {
        (self.foreground.replace(id) != Some(id)).then_some(HookEvent::Foreground)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: i64, x: i32) -> WindowInfo {
        WindowInfo { hwnd: id, bounds: Some(PixelRect::new(x, 0, x + 100, 100)), ..Default::default() }
    }

    #[test]
    fn drag_start_and_end() {
        let mut s = HookSynth::new();
        assert!(s.poll(0.0, &[win(1, 0), win(2, 0)]).is_empty());
        assert!(s.poll(0.1, &[win(1, 0), win(2, 0)]).is_empty());
        assert_eq!(s.poll(0.2, &[win(1, 10), win(2, 0)]), vec![HookEvent::MoveSizeStart]);
        // keeps moving, then a second window joins: no repeat
        assert!(s.poll(0.3, &[win(1, 20), win(2, 0)]).is_empty());
        assert!(s.poll(0.4, &[win(1, 20), win(2, 5)]).is_empty());
        // window 1 settled at 0.6, window 2 not yet
        assert!(s.poll(0.65, &[win(1, 20), win(2, 5)]).is_empty());
        assert_eq!(s.poll(0.75, &[win(1, 20), win(2, 5)]), vec![HookEvent::MoveSizeEnd]);
        assert!(s.poll(1.0, &[win(1, 20), win(2, 5)]).is_empty());
        // bounds query failing counts as a change
        let mut gone = win(1, 20);
        gone.bounds = None;
        assert_eq!(s.poll(1.1, &[gone]), vec![HookEvent::MoveSizeStart, HookEvent::Destroy(2)]);
        assert_eq!(s.poll(f64::NAN, &[]), vec![HookEvent::Destroy(1), HookEvent::MoveSizeEnd]);
    }

    #[test]
    fn destroy_and_space_switch() {
        let mut s = HookSynth::new();
        s.poll(0.0, &[win(1, 0), win(2, 0), win(3, 0)]);
        assert_eq!(s.poll(0.1, &[win(1, 0), win(2, 0)]), vec![HookEvent::Destroy(3)]);
        s.space_switched();
        assert!(s.poll(0.2, &[win(4, 0)]).is_empty());
        // only the poll right after the switch
        assert_eq!(s.poll(0.3, &[]), vec![HookEvent::Destroy(4)]);
        assert_eq!(s.take_cloaked(), vec![1, 2]);
        assert!(s.take_cloaked().is_empty());
    }

    #[test]
    fn foreground_changes() {
        let mut s = HookSynth::new();
        assert_eq!(s.foreground(5), Some(HookEvent::Foreground));
        assert_eq!(s.foreground(5), None);
        assert_eq!(s.foreground(6), Some(HookEvent::Foreground));
    }
}
