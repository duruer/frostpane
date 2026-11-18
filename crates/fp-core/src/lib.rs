//! Frostpane shared core types.
//!
//! Everything in this crate is platform-free and deterministic: no OS calls,
//! no global state, randomness only through [`rng::Rng`], time only through
//! [`clock::Clock`] or explicit arguments. All other crates build on these
//! types; see `docs/ARCHITECTURE.md` for the coordinate convention.

pub mod clock;
pub mod desktop;
pub mod effect;
pub mod falling;
pub mod feed;
pub mod frame;
pub mod geom;
pub mod gpu;
pub mod marker;
pub mod math;
pub mod prefs;
pub mod rng;
pub mod scene;
pub mod settings;
pub mod surface;
pub mod version;
pub mod wind;

pub use geom::{Frame, PixelRect, Rect4, Vec2};
