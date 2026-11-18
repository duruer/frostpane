//! Renderer-neutral draw lists (misc 5, snow-sim 4, other-weather 2.x).
//!
//! Simulations never touch wgpu. Each frame they fill a [`DrawList`] with
//! [`Pass`]es: raw uniform floats plus storage-buffer bytes. `fp-overlay`
//! owns one WGSL pipeline per [`PassKind`] whose bind layout matches the
//! contract documented on each variant.

use bytemuck::{Pod, Zeroable};

/// Rain streak, 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RainStroke {
    pub head: [f32; 2],
    pub tail: [f32; 2],
    pub width: f32,
    pub alpha: f32,
    pub glow: f32,
    pub pad: f32,
}

/// Rain ripple, 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RainRipple {
    pub center: [f32; 2],
    pub radius: f32,
    pub alpha: f32,
}

/// Sakura petal or autumn leaf, 40 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct PetalSprite {
    pub position: [f32; 2],
    pub size: f32,
    pub angle: f32,
    pub flip: f32,
    pub alpha: f32,
    pub soft: f32,
    pub tint: f32,
    pub shape: f32,
    pub pad: f32,
}

/// Falling star, 24 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StarSprite {
    pub position: [f32; 2],
    pub size: f32,
    pub angle: f32,
    pub alpha: f32,
    pub hue: f32,
}

/// Meteor trail, 28 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StarTrail {
    pub head: [f32; 2],
    pub tail: [f32; 2],
    pub width: f32,
    pub alpha: f32,
    pub hue: f32,
}

/// Firefly, 24 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FireflySprite {
    pub position: [f32; 2],
    pub glow: f32,
    pub hue: f32,
    pub size: f32,
    pub pad: f32,
}

/// Loose snow sprite (grain, lump, puff), 32 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SnowSprite {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    pub size: f32,
    pub alpha: f32,
    pub soft: f32,
    pub lump: f32,
}

/// Falling flake evaluated in closed form on the GPU (snow-sim 3.3), 52 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct SnowFlake {
    pub x: f32,
    pub top: f32,
    pub born: f32,
    pub fall: f32,
    pub drift: f32,
    pub wind_at_birth: f32,
    pub phase: f32,
    pub sway_rate: f32,
    pub sway: f32,
    pub size: f32,
    pub alpha: f32,
    pub soft: f32,
    pub gone: f32,
}

const _: () = assert!(size_of::<RainStroke>() == 32);
const _: () = assert!(size_of::<RainRipple>() == 16);
const _: () = assert!(size_of::<PetalSprite>() == 40);
const _: () = assert!(size_of::<StarSprite>() == 24);
const _: () = assert!(size_of::<StarTrail>() == 28);
const _: () = assert!(size_of::<FireflySprite>() == 24);
const _: () = assert!(size_of::<SnowSprite>() == 32);
const _: () = assert!(size_of::<SnowFlake>() == 52);

/// Colour blending of a pass. All shaders output premultiplied colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// colour and alpha: One, OneMinusSrcAlpha.
    Premultiplied,
    /// colour: One, One; alpha: One, OneMinusSrcAlpha.
    Additive,
}

/// Which overlay window a draw targets (snow behind-windows split, desktop-native 3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The main panel (behind windows when `behindWindows` is on).
    Main,
    /// The topmost "above" panel (snow on window tops).
    Above,
}

/// Pipeline selector. Bindings: group 0 = scene uniform (`DrawList::scene`),
/// binding 1 = pass uniforms, bindings 2.. = `Pass::buffers` in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PassKind {
    /// uniforms: Drift {origin.xy, columnWidth, count, bottom, top, occluderCount, pad};
    /// buffers: [columns f32 (shape ++ bed)], [occluders vec4]. 4 vertices.
    SnowDrift,
    /// uniforms: Weather {now, wind, breeze, span, margin, pad..}; buffers: [SnowFlake]. Instanced.
    SnowFlakes,
    /// buffers: [SnowSprite]. Instanced.
    SnowLoose,
    /// uniforms: {flash, height}. 4 vertices.
    RainFlash,
    /// buffers: [RainRipple]. Instanced.
    RainRipples,
    /// buffers: [RainStroke]. Instanced.
    RainStrokes,
    /// buffers: [PetalSprite]. Instanced.
    Petals,
    /// buffers: [StarTrail]. Instanced, additive.
    StarTrails,
    /// buffers: [StarSprite]. Instanced, additive.
    Stars,
    /// buffers: [FireflySprite]. Instanced, additive.
    Fireflies,
}

impl PassKind {
    pub fn blend(self) -> Blend {
        match self {
            PassKind::StarTrails | PassKind::Stars | PassKind::Fireflies => Blend::Additive,
            _ => Blend::Premultiplied,
        }
    }
}

/// One draw call.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub kind: PassKind,
    /// Padded to a multiple of 4 floats by [`DrawList::push`].
    pub uniforms: Vec<f32>,
    pub buffers: Vec<Vec<u8>>,
    pub vertices: u32,
    pub instances: u32,
}

/// Everything one overlay panel draws in one frame, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    /// Scene uniform: `[width, height, time, scale]` (points).
    pub scene: [f32; 4],
    pub passes: Vec<Pass>,
}

impl DrawList {
    pub fn new(width: f32, height: f32, time: f32) -> Self {
        Self { scene: [width, height, time, 1.0], passes: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.passes.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    pub fn push(&mut self, mut pass: Pass) {
        while !pass.uniforms.len().is_multiple_of(4) {
            pass.uniforms.push(0.0);
        }
        self.passes.push(pass);
    }

    /// Instanced 4-vertex quads from one slice; skipped when empty.
    pub fn instanced<T: Pod>(&mut self, kind: PassKind, uniforms: Vec<f32>, items: &[T]) {
        if items.is_empty() {
            return;
        }
        self.push(Pass {
            kind,
            uniforms,
            buffers: vec![bytemuck::cast_slice(items).to_vec()],
            vertices: 4,
            instances: items.len() as u32,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blends() {
        assert_eq!(PassKind::Stars.blend(), Blend::Additive);
        assert_eq!(PassKind::StarTrails.blend(), Blend::Additive);
        assert_eq!(PassKind::Fireflies.blend(), Blend::Additive);
        assert_eq!(PassKind::Petals.blend(), Blend::Premultiplied);
    }

    #[test]
    fn draw_list() {
        let mut d = DrawList::new(10.0, 20.0, 1.0);
        assert!(d.is_empty());
        d.instanced::<RainRipple>(PassKind::RainRipples, vec![], &[]);
        assert!(d.is_empty());
        d.instanced(PassKind::RainRipples, vec![1.0], &[RainRipple::default(); 3]);
        let p = &d.passes[0];
        assert_eq!((p.instances, p.vertices, p.buffers[0].len()), (3, 4, 48));
        assert_eq!(p.uniforms, vec![1.0, 0.0, 0.0, 0.0]);
        d.clear();
        assert!(d.is_empty());
        assert_eq!(d.scene, [10.0, 20.0, 1.0, 1.0]);
    }
}
