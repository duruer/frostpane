//! Wind setting, natural breeze and the shared `Blow` gust helper
//! (snow-sim 3.4, other-weather 1.1). Units: points per second, negative = left.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Wind {
    #[default]
    None,
    GentleLeft,
    StrongLeft,
    GentleRight,
    StrongRight,
    Changing,
}

impl Wind {
    pub const ALL: [Wind; 6] =
        [Wind::None, Wind::GentleLeft, Wind::StrongLeft, Wind::GentleRight, Wind::StrongRight, Wind::Changing];

    pub fn raw(self) -> &'static str {
        match self {
            Wind::None => "none",
            Wind::GentleLeft => "gentleLeft",
            Wind::StrongLeft => "strongLeft",
            Wind::GentleRight => "gentleRight",
            Wind::StrongRight => "strongRight",
            Wind::Changing => "changing",
        }
    }

    /// Exact parse; unknown -> `None` wind.
    pub fn parse(s: &str) -> Wind {
        Self::ALL.into_iter().find(|w| w.raw() == s).unwrap_or(Wind::None)
    }

    /// Target speed of the user setting at monotonic time `t` seconds.
    pub fn speed(self, t: f64) -> f64 {
        match self {
            Wind::None => 0.0,
            Wind::GentleLeft => -45.0,
            Wind::StrongLeft => -120.0,
            Wind::GentleRight => 45.0,
            Wind::StrongRight => 120.0,
            Wind::Changing => {
                let v = 0.55 * (0.019 * t).sin() + 0.35 * (0.0071 * t + 2.1).sin() + 0.25 * (0.043 * t + 0.7).sin();
                v.clamp(-1.0, 1.0) * 130.0
            }
        }
    }
}

/// Natural breeze speed (always present), snow-sim 3.4 (1).
pub fn breeze_speed(t: f64) -> f64 {
    10.0 * (0.07 * t).sin() + 6.0 * (0.19 * t + 1.7).sin() + 3.0 * (0.43 * t + 4.1).sin()
}

/// Exact integral of [`breeze_speed`] (displacement).
pub fn breeze_displacement(t: f64) -> f64 {
    -(10.0 / 0.07) * (0.07 * t).cos() - (6.0 / 0.19) * (0.19 * t + 1.7).cos() - (3.0 / 0.43) * (0.43 * t + 4.1).cos()
}

/// Low-passed user wind with gusts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Blow {
    pub strength: f64,
    pub steady: f64,
    pub speed: f64,
    pub carried: f64,
}

impl Blow {
    pub fn new(strength: f64) -> Self {
        Self { strength, ..Default::default() }
    }

    /// Gust factor `n` at clock `c`.
    pub fn gust(c: f64) -> f64 {
        0.3 * (0.31 * c).sin() + 0.2 * (0.83 * c + 1.4).sin() + 0.5 * (0.12 * c + 0.5).sin().max(0.0).powi(4)
    }

    /// Advance by `dt` at field clock `clock`; `wind_t` is the time base for
    /// [`Wind::Changing`].
    pub fn step(&mut self, dt: f64, clock: f64, wind: Wind, wind_t: f64) {
        self.steady += (wind.speed(wind_t) * self.strength - self.steady) * (dt * 0.6).min(1.0);
        self.speed = self.steady * (0.85 + Self::gust(clock));
        self.carried += self.speed * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_and_speeds() {
        for w in Wind::ALL {
            assert_eq!(Wind::parse(w.raw()), w);
        }
        assert_eq!(Wind::parse("GentleLeft"), Wind::None);
        assert_eq!(Wind::GentleLeft.speed(0.0), -45.0);
        assert_eq!(Wind::StrongLeft.speed(0.0), -120.0);
        assert_eq!(Wind::GentleRight.speed(0.0), 45.0);
        assert_eq!(Wind::StrongRight.speed(0.0), 120.0);
        assert_eq!(Wind::None.speed(0.0), 0.0);
        for t in [0.0, 50.0, 300.0, 1000.0] {
            assert!(Wind::Changing.speed(t).abs() <= 130.0);
        }
        assert_eq!(serde_json::to_string(&Wind::StrongRight).unwrap(), "\"strongRight\"");
    }

    #[test]
    fn breeze_integral_matches() {
        let (t, h) = (12.3, 1e-4);
        let num = (breeze_displacement(t + h) - breeze_displacement(t - h)) / (2.0 * h);
        assert!((num - breeze_speed(t)).abs() < 1e-5);
    }

    #[test]
    fn blow_low_pass() {
        let mut b = Blow::new(1.0);
        for i in 0..600 {
            b.step(1.0 / 60.0, i as f64 / 60.0, Wind::StrongRight, 0.0);
        }
        assert!(b.steady > 100.0 && b.steady <= 120.0);
        assert!(b.carried > 0.0);
        assert!(Blow::gust(0.0).abs() < 1.0);
    }
}
