//! Reactions to settings changes (core-app 2.4): which subsystems must hear
//! about a set of [`Changes`].

use fp_core::settings::Changes;

/// What to do after a mutator returned some [`Changes`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reaction {
    /// Rewrite `settings.json`.
    pub save: bool,
    /// Rebuild the tray menu and icon.
    pub tray: bool,
    /// The shown weather changed: new effect on every panel.
    pub effect: bool,
    /// Push the snow share (`power saving x storm`).
    pub share: bool,
    /// Push the lightning flag.
    pub lightning: bool,
    /// A source mode flipped: sync the 3 h timers.
    pub sources: bool,
    /// Push wind, keep-clear, behind-windows to the overlay (diffed by the host).
    pub overlay: bool,
}

/// Map changes to reactions.
pub fn react(c: Changes) -> Reaction {
    Reaction {
        save: c.needs_save(),
        tray: !c.is_empty(),
        effect: c.any(Changes::SHOWN_WEATHER),
        share: c.any(Changes::SNOW_STRENGTH | Changes::SHOWN_WEATHER | Changes::HEAVY_SNOW),
        lightning: c.any(Changes::LIGHTNING_SHOWN | Changes::MATCH_WEATHER | Changes::LIGHTNING),
        sources: c.any(Changes::MATCH_WEATHER | Changes::RANDOM_WEATHER | Changes::MATCH_SEASON),
        overlay: !c.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_changed_nothing_happens() {
        assert_eq!(react(Changes::NONE), Reaction::default());
    }

    #[test]
    fn shown_weather() {
        let r = react(Changes::SHOWN_WEATHER | Changes::WEATHER);
        assert!(r.save && r.tray && r.effect && r.share && r.overlay);
        assert!(!r.lightning && !r.sources);
    }

    #[test]
    fn modes_and_lightning() {
        let r = react(Changes::MATCH_WEATHER);
        assert!(r.sources && r.lightning && r.save);
        assert!(!r.effect);
        assert!(react(Changes::LIGHTNING_SHOWN).lightning);
        assert!(react(Changes::RANDOM_WEATHER).sources);
        assert!(react(Changes::MATCH_SEASON).sources);
        assert!(react(Changes::HEAVY_SNOW).share);
        assert!(react(Changes::SNOW_STRENGTH).share);
    }

    #[test]
    fn runtime_only_changes_do_not_save() {
        let r = react(Changes::SHOWN_WEATHER | Changes::PAUSED_FOR_BATTERY);
        assert!(!r.save);
        assert!(r.tray);
        assert!(react(Changes::OTHER).save);
    }
}
