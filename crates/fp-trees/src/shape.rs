//! Tree geometry and snow patches (trees 2 TreeShape, TreeSnowPatch, 3.17
//! patch generation).

use crate::look::Topper;
use fp_core::rng::{Lcg64, Rng};

/// Geometry derived from the level (trees 2 TreeShape).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeShape {
    pub level: u8,
    pub height: f64,
    pub half: f64,
    pub tiers: u32,
}

/// One tier: hem and apex heights above the foot and half width at the hem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tier {
    pub hem: f64,
    pub apex: f64,
    pub half: f64,
}

impl Tier {
    pub fn height(&self) -> f64 {
        self.apex - self.hem
    }

    /// Half width at height `y`.
    pub fn width_at(&self, y: f64) -> f64 {
        self.half * ((self.apex - y) / (self.apex - self.hem)).clamp(0.0, 1.0)
    }
}

const TABLE: [(f64, f64, u32); 6] =
    [(96.0, 42.0, 2), (150.0, 64.0, 3), (230.0, 94.0, 4), (320.0, 124.0, 5), (430.0, 152.0, 6), (560.0, 180.0, 8)];

impl TreeShape {
    /// Shape of a level; levels above 5 clamp to 5.
    pub fn from_level(level: u8) -> TreeShape {
        let level = level.min(5);
        let (height, half, tiers) = TABLE[usize::from(level)];
        TreeShape { level, height, half, tiers }
    }

    pub fn trunk_top(&self) -> f64 {
        self.height * 0.08 + 4.0
    }

    pub fn trunk_half(&self) -> f64 {
        (self.half * 0.13).max(5.0)
    }

    /// Ornament base size.
    pub fn unit(&self) -> f64 {
        8.5 + f64::from(self.level) * 1.6
    }

    pub fn topper_size(&self) -> f64 {
        15.0 + f64::from(self.level) * 5.0
    }

    /// Tier `i` (0 = bottom).
    pub fn tier(&self, i: u32) -> Tier {
        let n = f64::from(self.tiers);
        let step = (self.height - self.trunk_top()) / (1.0 + (n - 1.0) * 0.56);
        let hem = self.trunk_top() + f64::from(i) * 0.56 * step;
        Tier { hem, apex: hem + step, half: self.half * (1.0 - 0.62 * f64::from(i) / (n - 1.0)) }
    }

    pub fn tier_list(&self) -> Vec<Tier> {
        (0..self.tiers).map(|i| self.tier(i)).collect()
    }

    /// Window padding left and right of the foot.
    pub fn pad(&self) -> f64 {
        self.half + self.height * 0.16 + 26.0
    }

    /// Window canvas size in points.
    pub fn canvas(&self) -> (f64, f64) {
        let room = self.topper_size() * 2.4 + 18.0;
        (2.0 * self.pad(), 8.0 + self.height + room)
    }

    /// Height of the topper origin: last apex minus 0.15 topper sizes.
    pub fn topper_origin(&self) -> f64 {
        self.tier(self.tiers - 1).apex - 0.15 * self.topper_size()
    }

    /// Hit test relative to the foot, untransformed (12 pt slop).
    pub fn contains(&self, x: f64, y: f64, has_topper: bool) -> bool {
        let top = self.trunk_top();
        if (-12.0..=top).contains(&y) {
            let in_trunk = x.abs() <= self.trunk_half() + 12.0;
            let in_wide = x.abs() <= self.half && y >= top - 2.0 * self.unit();
            return in_trunk || in_wide;
        }
        let in_tier =
            self.tier_list().iter().any(|t| y >= t.hem - 12.0 && y <= t.apex + 12.0 && x.abs() <= t.width_at(y) + 12.0);
        if in_tier {
            return true;
        }
        let s = self.topper_size();
        has_topper && x.abs() <= s * 1.4 + 12.0 && y > self.height - 12.0 && y <= self.height + 2.0 * s + 12.0
    }
}

/// Snow resting on a branch (trees 2 TreeSnowPatch). Position is relative to
/// the foot.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    pub widest: f64,
    pub weight: f64,
    pub fullest: f64,
    pub cling: f64,
    pub depth: f64,
}

impl Patch {
    /// Drawn radius.
    pub fn radius(&self) -> f64 {
        self.widest * (0.1 + 0.9 * self.depth.powf(0.7))
    }
}

fn add(out: &mut Vec<Patch>, rng: &mut Lcg64, (x, y, angle, widest, cling): (f64, f64, f64, f64, f64)) {
    let weight = rng.range(0.55, 1.35);
    let fullest = rng.range(0.3, 0.62);
    let noise = rng.range(-0.22, 0.22);
    out.push(Patch { x, y, angle, widest, weight, fullest, cling: (cling + noise).clamp(0.0, 1.0), depth: 0.0 });
}

/// Deterministic patch list for `(shape, seed)` (3.17).
pub fn generate_patches(shape: &TreeShape, seed: u64) -> Vec<Patch> {
    let mut rng = Lcg64::new(seed.wrapping_add(311));
    let mut out = Vec::new();
    let u = shape.unit() * 0.42;
    let tiers = shape.tier_list();
    for (i, t) in tiers.iter().enumerate() {
        let h = t.height();
        let limit = tiers.get(i + 1).map_or(t.apex, |n| n.hem + 0.05 * h);
        let slope = h.atan2(t.half);
        let mut y = t.hem + 2.0;
        while y < limit {
            let w = t.width_at(y);
            let tt = (y - t.hem) / (limit - t.hem).max(1.0);
            let base = 0.28 + 0.4 * tt;
            for s in [-1.0, 1.0] {
                let wide = u * rng.range(0.8, 1.15);
                add(&mut out, &mut rng, (s * w * 0.94, y, -s * slope, wide, base));
            }
            let k = (w / (u * 2.6)) as i64;
            for j in 1..=k {
                for s in [-1.0, 1.0] {
                    if rng.unit() < 0.35 {
                        let x = s * w * (0.94 - j as f64 / (k + 1) as f64 * 0.9);
                        let yy = y + rng.range(-u, u) * 0.4;
                        let angle = rng.range(-0.25, 0.25);
                        let wide = u * rng.range(0.6, 1.0);
                        add(&mut out, &mut rng, (x, yy, angle, wide, base + 0.34 * (1.0 - x.abs() / w.max(1.0))));
                    }
                }
            }
            y += u * 1.7;
        }
        let n = ((t.half / (u * 1.5)) as i64).max(4);
        for l in 0..=n {
            let x = -0.9 * t.half + 1.8 * t.half * l as f64 / n as f64;
            let angle = rng.range(-0.12, 0.12);
            let wide = u * rng.range(0.8, 1.2);
            add(&mut out, &mut rng, (x, t.hem + 0.06 * h, angle, wide, 0.14));
        }
    }
    out
}

/// Perch of the snow cap on the topper: `(x, y, wide)` for topper size `s`.
pub fn perch(topper: Option<Topper>, s: f64) -> (f64, f64, f64) {
    let (x, y, w) = match topper {
        Some(Topper::Star) => (0.0, 1.55, 0.14),
        Some(Topper::Snowflake) => (0.0, 1.68, 0.12),
        Some(Topper::Bow) => (0.0, 1.3, 0.2),
        Some(Topper::Bell) => (0.0, 1.48, 0.12),
        Some(Topper::Heart) => (0.42, 1.32, 0.16),
        Some(Topper::Spire) => (0.0, 1.92, 0.08),
        None => (0.0, 0.1, 0.16),
    };
    (x * s, y * s, w * s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_and_derived() {
        let s = TreeShape::from_level(2);
        assert_eq!((s.height, s.half, s.tiers), (230.0, 94.0, 4));
        assert_eq!(TreeShape::from_level(200).level, 5);
        assert!((s.trunk_top() - 22.4).abs() < 1e-9);
        assert!((s.trunk_half() - 12.22).abs() < 1e-9);
        assert_eq!(TreeShape::from_level(0).trunk_half(), 5.46);
        assert!((s.unit() - 11.7).abs() < 1e-9);
        assert!((s.topper_size() - 25.0).abs() < 1e-9);
        let t0 = s.tier(0);
        assert!((t0.hem - 22.4).abs() < 1e-9);
        assert_eq!(t0.half, 94.0);
        let last = s.tier(3);
        assert!((last.apex - 230.0).abs() < 1e-6);
        assert!((last.half - 94.0 * 0.38).abs() < 1e-9);
        assert_eq!(s.tier_list().len(), 4);
        assert_eq!(t0.width_at(t0.apex + 5.0), 0.0);
        assert_eq!(t0.width_at(t0.hem - 5.0), 94.0);
        let (w, h) = s.canvas();
        assert!((w - 2.0 * (94.0 + 36.8 + 26.0)).abs() < 1e-9);
        assert!((h - (8.0 + 230.0 + 25.0 * 2.4 + 18.0)).abs() < 1e-9);
        assert!((s.topper_origin() - (230.0 - 3.75)).abs() < 1e-6);
    }

    #[test]
    fn hit_test() {
        let s = TreeShape::from_level(2);
        assert!(s.contains(0.0, 5.0, false));
        assert!(s.contains(90.0, 20.0, false));
        assert!(!s.contains(100.0, 0.0, false));
        assert!(!s.contains(200.0, 5.0, false));
        assert!(s.contains(0.0, 100.0, false));
        assert!(!s.contains(200.0, 100.0, false));
        assert!(s.contains(0.0, 240.0, false), "tier slop");
        assert!(!s.contains(0.0, 260.0, false));
        assert!(s.contains(0.0, 260.0, true));
        assert!(!s.contains(100.0, 260.0, true));
        assert!(!s.contains(0.0, 400.0, true));
    }

    #[test]
    fn patches_are_deterministic() {
        for level in 0..=5 {
            let shape = TreeShape::from_level(level);
            let a = generate_patches(&shape, 1234);
            assert_eq!(a, generate_patches(&shape, 1234));
            assert_ne!(a, generate_patches(&shape, 1235));
            assert!(a.len() > 20);
            assert!(a.iter().all(|p| p.depth == 0.0 && (0.0..=1.0).contains(&p.cling)));
            assert!(a.iter().all(|p| p.weight >= 0.55 && p.fullest >= 0.3 && p.fullest <= 0.62));
        }
        let p = Patch { x: 0.0, y: 0.0, angle: 0.0, widest: 10.0, weight: 1.0, fullest: 0.5, cling: 0.0, depth: 1.0 };
        assert!((p.radius() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn perches() {
        let all = [
            None,
            Some(Topper::Star),
            Some(Topper::Snowflake),
            Some(Topper::Bow),
            Some(Topper::Bell),
            Some(Topper::Heart),
            Some(Topper::Spire),
        ];
        for t in all {
            let (_, y, w) = perch(t, 10.0);
            assert!(y > 0.0 && w > 0.0);
        }
        assert_eq!(perch(Some(Topper::Heart), 10.0).0, 4.2);
    }
}
