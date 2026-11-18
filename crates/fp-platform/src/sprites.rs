//! Sprite (object) windows: the OS seam of `fp-layered`'s diffing host
//! (snowballs 3.7, characters 0.1, decorations 3.4-3.7, desktop-native 3.4).

use fp_core::geom::PixelRect;
use fp_core::scene::HitMode;
use tiny_skia::Pixmap;

/// Opaque OS window handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WinId(pub isize);

/// Where to put a window in the z-order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Topmost,
    NotTopmost,
    Top,
    After(WinId),
}

/// OS seam of `fp-layered`'s host. One implementation per platform.
pub trait SpriteBackend {
    /// New hidden click-through layered window.
    fn create(&mut self) -> Option<WinId>;
    fn destroy(&mut self, id: WinId);
    /// Replace the window's bitmap (premultiplied RGBA in, size = pixmap size).
    /// False when the bitmap could not be stored (the old one is gone too).
    fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool;
    /// Move to `(x, y)` pixels, apply `alpha` and show if hidden, in one call.
    fn present(&mut self, id: WinId, x: i32, y: i32, alpha: u8);
    fn hide(&mut self, id: WinId);
    /// Toggle `WS_EX_TRANSPARENT`.
    fn set_click_through(&mut self, id: WinId, on: bool);
    /// The sprite's hit mode, set on creation and whenever it changes. A
    /// backend without a global cursor (Wayland) shapes its input region from
    /// it; the default ignores it (click-through covers every other case).
    fn set_hit_mode(&mut self, _id: WinId, _mode: HitMode) {}
    /// z-order change without move/resize/activation.
    fn place(&mut self, id: WinId, p: Place);
    /// Lowest-z taskbar window among the topmost ones.
    fn lowest_taskbar(&self) -> Option<WinId>;
    /// A topmost taskbar window sits above `id`.
    fn taskbar_above(&self, id: WinId) -> bool;
    /// A foreign top-level window lies above `id` at the cursor.
    fn covered(&self, id: WinId, cursor: (i32, i32)) -> bool;
    /// Pixel rect of the foreground window when it is a real foreign app; the
    /// caller counts only monitors it covers completely as fullscreen.
    fn fullscreen(&self) -> Vec<PixelRect>;
    /// Re-apply topmost to those of `ids` found in the topmost band, keeping
    /// their relative order, so they end above everything else topmost.
    fn raise_front(&mut self, ids: &[WinId]);
    /// Order `ids` (back to front) right above the desktop host.
    fn stack_desktop(&mut self, ids: &[WinId]);
    /// Dispatch pending messages of this thread.
    fn pump(&mut self);
}

/// True when `cands` appear in `run` in the same relative order with
/// strictly increasing positions (decorations 3.7 step 4).
pub fn stack_ok<T: PartialEq>(run: &[T], cands: &[T]) -> bool {
    let mut last: Option<usize> = None;
    for c in cands {
        let Some(i) = run.iter().position(|r| r == c) else { return false };
        if last.is_some_and(|l| i <= l) {
            return false;
        }
        last = Some(i);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_order() {
        assert!(stack_ok(&[1, 2, 3, 4], &[2, 4]));
        assert!(stack_ok::<i32>(&[], &[]));
        assert!(!stack_ok(&[1, 2, 3], &[3, 2]));
        assert!(!stack_ok(&[1, 2, 3], &[2, 9]));
        assert!(!stack_ok(&[1, 2], &[1, 1]));
    }
}
