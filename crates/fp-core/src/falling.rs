//! The per-monitor weather simulation contract (`IFalling`, snow-sim 3.16,
//! other-weather 1). Coordinates: screen-local points, Y up.

use crate::geom::Vec2;
use crate::gpu::{DrawList, Layer};
use crate::surface::Surface;
use crate::wind::Wind;

/// Implemented by snow, rain, petals/leaves, starfall and fireflies.
/// One instance per monitor, owned by that monitor's render thread.
pub trait Falling: Send {
    /// Density multiplier (power saving x storm strength).
    fn set_share(&mut self, share: f64);
    fn share(&self) -> f64;
    /// User wind setting.
    fn set_wind(&mut self, wind: Wind);
    /// Wall-clock seconds for [`Wind::Changing`] (other-weather 1.1), so all
    /// monitors share one changing-wind phase. Until the first call the
    /// field's own clock is used.
    fn set_wind_time(&mut self, _t: f64) {}
    /// Advance by `dt` (already clamped). `cursor` is screen-local or `None`.
    fn step(&mut self, dt: f64, cursor: Option<Vec2>);
    /// New tracked-window surfaces. `interval` = seconds since last update
    /// (<= 0.25); `to_other_space` suppresses "window moved away" reactions.
    fn update(&mut self, surfaces: &[Surface], interval: f64, to_other_space: bool);
    /// Fill `out` for one panel.
    fn draw(&self, layer: Layer, out: &mut DrawList);

    fn show_ground(&mut self, _on: bool) {}
    fn keep_clear(&mut self, _on: bool) {}
    /// Rain: thunderstorm on/off.
    fn set_lightning(&mut self, _on: bool) {}
    /// Does this effect use the above panel at all (snow only)?
    fn draws_above(&self) -> bool {
        false
    }
    /// Is an above panel attached (snow splits its layers)?
    fn set_split(&mut self, _on: bool) {}
    /// Is there anything to draw on the above panel this frame?
    fn has_above(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy(f64);
    impl Falling for Dummy {
        fn set_share(&mut self, s: f64) {
            self.0 = s;
        }
        fn share(&self) -> f64 {
            self.0
        }
        fn set_wind(&mut self, _: Wind) {}
        fn step(&mut self, _: f64, _: Option<Vec2>) {}
        fn update(&mut self, _: &[Surface], _: f64, _: bool) {}
        fn draw(&self, _: Layer, _: &mut DrawList) {}
    }

    #[test]
    fn defaults_are_noops() {
        let mut d = Dummy(1.0);
        d.set_share(2.0);
        d.set_wind(Wind::None);
        d.set_wind_time(10.0);
        d.step(0.1, None);
        d.update(&[], 0.1, false);
        d.draw(Layer::Main, &mut DrawList::default());
        d.show_ground(true);
        d.keep_clear(true);
        d.set_lightning(true);
        d.set_split(true);
        assert_eq!(d.share(), 2.0);
        assert!(!d.draws_above());
        assert!(!d.has_above());
    }
}
