//! Wallpapers tab model (ui 3.2): which packs exist, the picked state that
//! drives the ring and the day-and-night pill, and the time-card rules.

use fp_core::effect::EffectKind;
use fp_sources::daynight::{self, Slot};

/// Display order of the packs (ui 3.2): snow, rain, starfall, fireflies,
/// leaves, sakura.
pub const ORDER: [EffectKind; 6] = [
    EffectKind::Snow,
    EffectKind::Rain,
    EffectKind::Starfall,
    EffectKind::Fireflies,
    EffectKind::Leaves,
    EffectKind::Sakura,
];

/// One available wallpaper pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paper {
    pub id: &'static str,
    pub effect: EffectKind,
    /// File name of the main picture inside the wallpapers folder.
    pub main: String,
    /// True when the dawn still exists: the pack can change with the day.
    pub daylong: bool,
}

/// Main picture of a pack: `<id>.jpg`, else its midday still.
fn main_file(id: &str, exists: &impl Fn(&str) -> bool) -> Option<String> {
    let plain = format!("{id}.jpg");
    if exists(&plain) {
        return Some(plain);
    }
    let midday = daynight::file_name(id, Slot::Midday);
    exists(&midday).then_some(midday)
}

/// Packs whose main picture exists, in display order. `exists` answers for a
/// file name inside the wallpapers folder.
pub fn scan(exists: impl Fn(&str) -> bool) -> Vec<Paper> {
    ORDER
        .iter()
        .filter_map(|&effect| {
            let id = effect.wallpaper_id();
            let main = main_file(id, &exists)?;
            Some(Paper { id, effect, main, daylong: exists(&daynight::file_name(id, Slot::Dawn)) })
        })
        .collect()
}

/// Picked id of a pack's whole day cycle.
pub fn cycle_id(pack: &str) -> String {
    format!("{pack}-day")
}

/// Picked id of one still.
pub fn still_id(pack: &str, slot: Slot) -> String {
    format!("{pack}-{}", slot.name())
}

/// Which picture is chosen now (not persisted across restarts, except an
/// active day cycle which starts as `<pack>-day`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picked(pub Option<String>);

impl Picked {
    /// Initial state: the running cycle's pack, if any.
    pub fn initial(day_pack: Option<&str>) -> Self {
        Picked(day_pack.map(cycle_id))
    }

    /// Ring on the tile: the id equals the pack id or starts with `<id>-`.
    pub fn ring(&self, pack: &str) -> bool {
        self.0.as_deref().is_some_and(|p| p == pack || p.starts_with(&format!("{pack}-")))
    }

    /// The pill is accented only for a cycle or a time still of this pack.
    pub fn pill_accent(&self, pack: &str) -> bool {
        self.0.as_deref().is_some_and(|p| p.starts_with(&format!("{pack}-")))
    }

    /// True while this pack's day cycle runs.
    pub fn cycle_on(&self, pack: &str) -> bool {
        self.0.as_deref() == Some(cycle_id(pack).as_str())
    }

    pub fn set(&mut self, id: String) {
        self.0 = Some(id);
    }
}

/// The 8 stills in the time card: `(slot, start minute)`.
pub fn times() -> [(Slot, u32); 8] {
    Slot::ALL.map(|s| (s, s.start_minute()))
}

/// Slot of the current local time.
pub fn current_slot(minutes_of_day: u32) -> Slot {
    daynight::slot_at(minutes_of_day)
}

/// What a click on the wallpapers tab asks the host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WallAction {
    /// Apply the main picture (stops any cycle).
    Main { pack: String },
    /// Apply one still (stops any cycle).
    Still { pack: String, slot: Slot },
    /// Start the day cycle for the pack.
    TurnOn { pack: String },
    /// Stop the cycle and freeze the still of the current time.
    TurnOff { pack: String, slot: Slot },
}

impl WallAction {
    /// The picked id this action leads to once it succeeds.
    pub fn picked_id(&self) -> String {
        match self {
            WallAction::Main { pack } => pack.clone(),
            WallAction::Still { pack, slot } | WallAction::TurnOff { pack, slot } => still_id(pack, *slot),
            WallAction::TurnOn { pack } => cycle_id(pack),
        }
    }

    /// File to apply (relative to the wallpapers folder) for a pack list.
    pub fn file(&self, papers: &[Paper]) -> Option<String> {
        match self {
            WallAction::Main { pack } => papers.iter().find(|p| p.id == pack).map(|p| p.main.clone()),
            WallAction::Still { pack, slot } | WallAction::TurnOff { pack, slot } => {
                Some(daynight::file_name(pack, *slot))
            }
            WallAction::TurnOn { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_orders_and_detects_daylong() {
        let files = ["winter.jpg", "winter-dawn.jpg", "night-midday.jpg", "spring.jpg", "rain-dusk.jpg"];
        let papers = scan(|f| files.contains(&f));
        let ids: Vec<_> = papers.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["winter", "night", "spring"]);
        assert!(papers[0].daylong);
        assert!(!papers[1].daylong);
        assert_eq!(papers[1].main, "night-midday.jpg");
        assert_eq!(papers[2].effect, EffectKind::Sakura);
        assert!(scan(|_| false).is_empty());
    }

    #[test]
    fn picked_rules() {
        let mut p = Picked::initial(None);
        assert!(!p.ring("winter") && !p.pill_accent("winter") && !p.cycle_on("winter"));
        p.set("winter".into());
        assert!(p.ring("winter") && !p.pill_accent("winter"));
        assert!(!p.ring("win"));
        p.set("winter-golden".into());
        assert!(p.ring("winter") && p.pill_accent("winter") && !p.cycle_on("winter"));
        assert!(!p.ring("rain"));
        let p = Picked::initial(Some("rain"));
        assert!(p.cycle_on("rain") && p.pill_accent("rain") && p.ring("rain"));
    }

    #[test]
    fn times_and_slots() {
        let t = times();
        assert_eq!(t[0], (Slot::Dawn, 360));
        assert_eq!(t[7], (Slot::Night, 1170));
        assert_eq!(current_slot(0), Slot::Night);
        assert_eq!(current_slot(360), Slot::Dawn);
        assert_eq!(current_slot(15 * 60 + 29), Slot::Midday);
        assert_eq!(current_slot(15 * 60 + 30), Slot::Golden);
        assert_eq!(current_slot(23 * 60), Slot::Night);
    }

    #[test]
    fn actions_name_their_result() {
        let papers = scan(|f| f == "winter.jpg");
        let main = WallAction::Main { pack: "winter".into() };
        assert_eq!(main.picked_id(), "winter");
        assert_eq!(main.file(&papers).as_deref(), Some("winter.jpg"));
        assert_eq!(WallAction::Main { pack: "x".into() }.file(&papers), None);
        let still = WallAction::Still { pack: "winter".into(), slot: Slot::Dusk };
        assert_eq!(still.picked_id(), "winter-dusk");
        assert_eq!(still.file(&papers).as_deref(), Some("winter-dusk.jpg"));
        let on = WallAction::TurnOn { pack: "winter".into() };
        assert_eq!(on.picked_id(), "winter-day");
        assert_eq!(on.file(&papers), None);
        let off = WallAction::TurnOff { pack: "winter".into(), slot: Slot::Night };
        assert_eq!(off.picked_id(), "winter-night");
        assert_eq!(off.file(&papers).as_deref(), Some("winter-night.jpg"));
    }
}
