//! The effect factory: `make_fall` builds one simulation per monitor.

use fp_core::effect::EffectKind;
use fp_core::falling::Falling;
use fp_core::feed::SnowFeed;
use fp_core::geom::Vec2;
use fp_core::rng::mix_seed;
use fp_overlay::{EffectFactory, PanelInfo};
use std::sync::Arc;

/// One simulation for the panel described by `info`.
pub fn make_fall(
    kind: EffectKind,
    info: &PanelInfo,
    feed: &Arc<SnowFeed>,
    reduce_motion: bool,
    seed: u64,
) -> Box<dyn Falling> {
    let (w, h) = (info.width, info.height);
    match kind {
        EffectKind::Snow => Box::new(fp_snow::SnowField::new(
            fp_snow::SnowConfig {
                width: w,
                height: h,
                reduce_motion,
                origin: Vec2::new(info.frame.x, info.frame.y),
                seed,
            },
            Some(feed.clone()),
        )),
        EffectKind::Rain => Box::new(fp_rain::RainField::new(w, h, reduce_motion, seed)),
        EffectKind::Sakura => {
            Box::new(fp_petals::PetalField::new(fp_petals::PetalKind::Sakura, w, h, reduce_motion, seed))
        }
        EffectKind::Leaves => {
            Box::new(fp_petals::PetalField::new(fp_petals::PetalKind::Leaves, w, h, reduce_motion, seed))
        }
        EffectKind::Fireflies => Box::new(fp_sky::FireflyField::new(w, h, reduce_motion, seed)),
        EffectKind::Starfall => Box::new(fp_sky::StarfallField::new(w, h, reduce_motion, seed)),
    }
}

/// The factory handed to the overlay: every panel gets its own seed.
pub fn factory(kind: EffectKind, feed: Arc<SnowFeed>, reduce_motion: bool, base_seed: u64) -> EffectFactory {
    Arc::new(move |info: &PanelInfo| {
        make_fall(kind, info, &feed, reduce_motion, mix_seed(base_seed ^ info.index as u64))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::geom::Frame;
    use fp_core::gpu::{DrawList, Layer};

    fn info(index: usize) -> PanelInfo {
        PanelInfo {
            index,
            id: format!("m{index}"),
            width: 640.0,
            height: 360.0,
            scale: 1.0,
            frame: Frame::new(0.0, 0.0, 640.0, 360.0),
        }
    }

    #[test]
    fn every_effect_builds_and_runs() {
        let feed = Arc::new(SnowFeed::default());
        for kind in EffectKind::ALL {
            let f = factory(kind, feed.clone(), false, 7);
            let mut fall = f(&info(0));
            fall.set_share(2.0);
            assert_eq!(fall.share(), 2.0, "{kind:?}");
            fall.step(0.016, None);
            fall.draw(Layer::Main, &mut DrawList::default());
        }
    }

    #[test]
    fn panels_get_different_seeds() {
        let feed = Arc::new(SnowFeed::default());
        let f = factory(EffectKind::Rain, feed, true, 1);
        let (mut a, mut b) = (f(&info(0)), f(&info(1)));
        for _ in 0..30 {
            a.step(0.05, None);
            b.step(0.05, None);
        }
        let (mut da, mut db) = (DrawList::default(), DrawList::default());
        a.draw(Layer::Main, &mut da);
        b.draw(Layer::Main, &mut db);
        let bytes =
            |d: &DrawList| d.passes.iter().flat_map(|p| p.buffers.iter().flatten().copied()).collect::<Vec<u8>>();
        assert_ne!(bytes(&da), bytes(&db));
    }
}
