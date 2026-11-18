//! One falling flake: closed-form motion shared with the WGSL (snow-sim 3.2, 3.3).

use fp_core::geom::Vec2;
use fp_core::gpu::SnowFlake;
use fp_core::math::smoothstep;
use fp_core::rng::{Lcg64, Rng, mix_seed};
use fp_core::surface::SurfaceId;
use std::f64::consts::TAU;

const NEVER: f64 = f32::MAX as f64;

/// Shape kinds of [`Flake::shape`] (integer part; the fraction is the seed).
pub(crate) const SPECK: f64 = 0.0;
pub(crate) const CLUMP: f64 = 1.0;
pub(crate) const CRYSTAL: f64 = 2.0;
pub(crate) const BOKEH: f64 = 3.0;
/// Normal flakes smaller than this stay specks.
const SPECK_BELOW: f64 = 2.4;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Flake {
    pub x: f64,
    pub top: f64,
    pub born: f64,
    pub fall: f64,
    pub drift: f64,
    pub wind_at_birth: f64,
    pub phase: f64,
    pub sway_rate: f64,
    pub sway: f64,
    pub size: f64,
    pub alpha: f64,
    pub soft: f64,
    pub gone: f64,
    pub settles: bool,
    pub landing: Option<(SurfaceId, f64)>,
    /// Visual only (never read back by the simulation): shape kind plus a
    /// seed in the fraction, spin in rad/s (signed) and the flutter depth
    /// (how far the apparent width dips as the flake tumbles).
    pub shape: f64,
    pub spin: f64,
    pub wobble: f64,
}

impl Flake {
    /// A fresh flake above the screen; `spread` is the random extra height.
    pub fn spawn(rng: &mut Lcg64, w: f64, h: f64, spread: f64, now: f64, wind: f64, reduce: bool) -> Self {
        let big = rng.unit() < 0.02;
        let motion = if reduce { 0.6 } else { 1.0 };
        let calm = if reduce { 0.5 } else { 1.0 };
        let x = rng.range(-30.0, w + 30.0);
        let top = h + rng.range(0.0, spread);
        let phase = rng.range(0.0, TAU);
        let sway_rate = rng.range(0.5, 1.4);
        let mut f = Self {
            x,
            top,
            born: now,
            fall: 0.0,
            drift: 1.0,
            wind_at_birth: wind,
            phase,
            sway_rate,
            sway: 0.0,
            size: 0.0,
            alpha: 0.28,
            soft: 0.95,
            gone: NEVER,
            settles: !big,
            landing: None,
            shape: SPECK,
            spin: 0.0,
            wobble: 0.0,
        };
        if big {
            f.fall = rng.range(75.0, 100.0) * motion;
            f.sway = rng.range(10.0, 20.0) * calm;
            f.size = rng.range(8.0, 13.0);
        } else {
            let n = rng.unit().powf(1.6);
            f.fall = (16.0 + 50.0 * n + rng.range(0.0, 10.0)) * motion;
            f.drift = 0.35 + 0.65 * n;
            f.sway = (rng.range(4.0, 9.0) + 10.0 * n) * calm;
            f.size = 1.4 + 4.4 * n + rng.range(0.0, 1.0);
            f.alpha = 0.4 + 0.5 * n;
            f.soft = 0.75 - 0.45 * n;
        }
        f.look(big, reduce);
        f
    }

    /// Picks the visual-only shape, spin and flutter. Seeded by a hash of the
    /// spawn values rather than the field rng, so the motion, size and
    /// landing sequence stay exactly as before.
    fn look(&mut self, big: bool, reduce: bool) {
        let mut h =
            mix_seed(self.x.to_bits() ^ self.top.to_bits().rotate_left(21) ^ self.phase.to_bits().rotate_left(42));
        let mut unit = || {
            h = mix_seed(h);
            (h >> 11) as f64 / (1u64 << 53) as f64
        };
        let kind = if big {
            BOKEH
        } else if self.size < SPECK_BELOW {
            SPECK
        } else if unit() < 0.5 * smoothstep(3.6, 6.2, self.size) {
            CRYSTAL
        } else {
            CLUMP
        };
        let (spin, wobble) = match kind as u8 {
            3 => ((0.1, 0.3), (0.04, 0.1)),
            2 => ((0.25, 1.0), (0.2, 0.4)),
            1 => ((0.4, 1.5), (0.15, 0.35)),
            _ => ((0.4, 1.6), (0.1, 0.25)),
        };
        let sign = if unit() < 0.5 { -1.0 } else { 1.0 };
        let calm = if reduce { (0.3, 0.4) } else { (1.0, 1.0) };
        // The seed stays below 1 in f32, so the kind survives the cast.
        self.shape = kind + unit().min(0.999);
        self.spin = sign * (spin.0 + (spin.1 - spin.0) * unit()) * calm.0;
        self.wobble = (wobble.0 + (wobble.1 - wobble.0) * unit()) * calm.1;
    }

    /// Position at flake clock `now` with total wind displacement `wind`.
    pub fn position(&self, now: f64, wind: f64, span: f64) -> Vec2 {
        let a = now - self.born;
        let p = self.phase + self.sway_rate * a;
        let y =
            self.top - self.fall * (a + 0.12 / (1.3 * self.sway_rate) * ((1.3 * self.phase).cos() - (1.3 * p).cos()));
        let sway_x = self.sway / self.sway_rate
            * (p.sin() - self.phase.sin() + (0.3 / 2.3) * ((2.3 * p).sin() - (2.3 * self.phase).sin()));
        let x = self.x + self.drift * (wind - self.wind_at_birth) + sway_x;
        Vec2::new(x - span * ((x + 30.0) / span).floor(), y)
    }

    /// Shift the flake clock back after the field epoch moved forward.
    pub fn rebase(&mut self, by: f64, carried: f64) {
        self.born -= by;
        self.wind_at_birth -= carried;
        if self.landing.is_some() {
            self.gone -= by;
        }
    }

    pub fn to_gpu(self) -> SnowFlake {
        SnowFlake {
            x: self.x as f32,
            top: self.top as f32,
            born: self.born as f32,
            fall: self.fall as f32,
            drift: self.drift as f32,
            wind_at_birth: self.wind_at_birth as f32,
            phase: self.phase as f32,
            sway_rate: self.sway_rate as f32,
            sway: self.sway as f32,
            size: self.size as f32,
            alpha: self.alpha as f32,
            soft: self.soft as f32,
            gone: self.gone as f32,
            shape: self.shape as f32,
            spin: self.spin as f32,
            wobble: self.wobble as f32,
        }
    }
}
