//! Non-Windows stand-in for `win`: the overlay is only implemented for Windows.

use crate::{Config, EffectFactory, PanelInfo};
use fp_core::desktop::MonitorInfo;
use fp_core::surface::ScreenSnap;
use fp_core::wind::Wind;

pub(crate) struct Host;

pub(crate) fn take_rescaled() -> bool {
    false
}

impl Host {
    pub(crate) fn new(_: &[MonitorInfo], _: Config) -> Result<Self, String> {
        Err("overlay is only implemented for Windows".into())
    }
    pub(crate) fn config(&self) -> Config {
        Config::default()
    }
    pub(crate) fn panels(&self) -> usize {
        0
    }
    pub(crate) fn infos(&self) -> Vec<PanelInfo> {
        Vec::new()
    }
    pub(crate) fn frames(&self) -> u64 {
        0
    }
    pub(crate) fn set_effect(&self, _: Option<EffectFactory>) {}
    pub(crate) fn set_share(&self, _: f64) {}
    pub(crate) fn set_wind(&self, _: Wind) {}
    pub(crate) fn set_lightning(&self, _: bool) {}
    pub(crate) fn set_keep_clear(&self, _: bool) {}
    pub(crate) fn set_behind_windows(&self, _: bool) {}
    pub(crate) fn update(&self, _: &[ScreenSnap], _: f64, _: bool) {}
    pub(crate) fn keep_on_top(&self) {}
    pub(crate) fn set_under_dock(&self, _: Vec<isize>) {}
    pub(crate) fn tick(&self) {}
}
