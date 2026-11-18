//! Desktop objects drawn on the CPU into their own layered windows
//! (snowballs, snowmen, trees, characters, gifts, decorations).
//!
//! Pure crates describe *what* to show as [`Sprite`]s; `fp-layered` owns the
//! OS windows and diffs sprite lists by key every frame.

use crate::geom::{Frame, Vec2};

/// Stable identity of one desktop object window. The high byte is the
/// owning subsystem ([`SpriteKey::new`]), so keys never collide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpriteKey(pub u64);

/// Owner namespaces for [`SpriteKey`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Owner {
    Ball = 1,
    Tree = 2,
    Character = 3,
    Gift = 4,
    Decoration = 5,
    Fight = 6,
    Puppet = 7,
    Pointer = 8,
}

impl SpriteKey {
    pub fn new(owner: Owner, id: u64) -> Self {
        SpriteKey(((owner as u64) << 56) | (id & 0x00FF_FFFF_FFFF_FFFF))
    }
    pub fn owner_byte(self) -> u8 {
        (self.0 >> 56) as u8
    }
    pub fn id(self) -> u64 {
        self.0 & 0x00FF_FFFF_FFFF_FFFF
    }
}

/// Z band of an object window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZBand {
    /// Topmost, above the weather overlay.
    Front,
    /// Just below the taskbar (objects resting on the dock).
    UnderDock,
    /// On the desktop layer, behind all normal windows (decorations).
    Desktop,
}

/// How the window takes mouse input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitMode {
    /// Always click-through.
    Never,
    /// Clickable only over pixels with alpha > 0 (cursor poll toggles WS_EX_TRANSPARENT).
    Alpha,
    /// Always clickable.
    Always,
}

/// One object window for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub key: SpriteKey,
    /// Window box in global points (Y up).
    pub frame: Frame,
    pub z: ZBand,
    pub hit: HitMode,
    /// Bumped whenever the art must be repainted.
    pub rev: u64,
    /// 0 hides the window without destroying it.
    pub opacity: f32,
}

/// Mouse state fed to the desktop world each step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerInput {
    /// Cursor in global points.
    pub pos: Option<Vec2>,
    pub left_down: bool,
    pub right_down: bool,
    /// Sprite under the cursor (hit-tested by the host using the alpha mask).
    pub hot: Option<SpriteKey>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let k = SpriteKey::new(Owner::Tree, 0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(k.owner_byte(), Owner::Tree as u8);
        assert_eq!(k.id(), 0x00FF_FFFF_FFFF_FFFF);
        assert_ne!(SpriteKey::new(Owner::Ball, 1), SpriteKey::new(Owner::Gift, 1));
    }
}
