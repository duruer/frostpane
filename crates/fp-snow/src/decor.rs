//! Decorative in-app snow (snow-sim 3.17) for the settings window.

/// One dot of the falling-snow backdrop at time `t` in a `w`x`h` box (Y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub alpha: f64,
}

/// `frac(sin((13 i + k) 12.9898) 43758.5453)`.
fn noise(i: usize, k: u32) -> f64 {
    let v = (((13 * i) as f64) + f64::from(k)) * 12.9898;
    let s = v.sin() * 43758.5453;
    s - s.floor()
}

/// Positions of `count` dots.
pub fn dots(count: usize, w: f64, h: f64, t: f64) -> Vec<Dot> {
    (0..count)
        .map(|i| {
            let speed = 12.0 + 22.0 * noise(i, 1);
            let radius = 1.0 + 2.2 * noise(i, 2);
            let y = (noise(i, 3) * h + t * speed).rem_euclid(h + 10.0) - 5.0;
            let x = noise(i, 4) * w + (0.5 * t + noise(i, 5) * 6.3).sin() * 10.0;
            Dot { x, y, radius, alpha: 0.55 + 0.45 * noise(i, 6) }
        })
        .collect()
}
