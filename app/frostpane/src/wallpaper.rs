//! Wallpaper actions and the day-and-night cycle (other-weather 2.6,
//! desktop-native 3.12). `fp-sources` decides; this module copies the file
//! (the OS never holds the original), calls the OS and keeps two prefs:
//! `dayWallpaper` (pack of the running cycle) and `dayWallpaperShown` (the
//! still last applied, by its original file name).

use fp_core::settings::Weathers;
use fp_sources::daynight::{self, Action};
use fp_ui::wallpapers::{self, WallAction};
use fp_win::Shell;
use std::path::Path;

pub const KEY_PACK: &str = "dayWallpaper";
pub const KEY_SHOWN: &str = "dayWallpaperShown";
/// The staged copy alternates between two names so the OS sees a new file.
const TAGS: [&str; 2] = ["frostpane-day-a", "frostpane-day-b"];
const STAGE_PREFIX: &str = "frostpane-day-";
/// Day-and-night check period (other-weather 2.6).
pub const EVERY_S: f64 = 60.0;

/// Folders the wallpaper code works in.
pub struct Dirs<'a> {
    /// `%LOCALAPPDATA%\Frostpane`; staged copies go to `<data>/wallpapers`.
    pub data: &'a Path,
    /// `%LOCALAPPDATA%\Frostpane\Wallpapers`: the packs.
    pub packs: &'a Path,
}

/// A plain file name inside the packs folder.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':']) && name != "." && name != ".."
}

/// Packs that can follow the day: the dawn still exists.
pub fn day_packs(dir: &Path) -> Vec<String> {
    wallpapers::scan(|f| dir.join(f).is_file()).into_iter().filter(|p| p.daylong).map(|p| p.id.to_string()).collect()
}

#[derive(Default)]
pub struct Wallpaper {
    flip: usize,
}

fn base(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
}

impl Wallpaper {
    /// Copy `<packs>/<file>` to a fresh staged name and set it on every monitor.
    pub fn apply(&mut self, os: &dyn Shell, dirs: &Dirs<'_>, file: &str) -> bool {
        if !plain_name(file) {
            return false;
        }
        let src = dirs.packs.join(file);
        if !src.is_file() {
            return false;
        }
        self.flip = (self.flip + 1) % TAGS.len();
        match fp_win::stage_wallpaper(dirs.data, &src, TAGS[self.flip]) {
            Ok(staged) => os.wallpaper_set(&staged),
            Err(e) => {
                log::warn!("staging wallpaper failed: {e}");
                false
            }
        }
    }

    /// The OS wallpaper's file name for `daynight::step`: our own staged
    /// copies count as the still we recorded as shown.
    fn os_name(os: &dyn Shell, shown: Option<&str>) -> Option<String> {
        let current = base(&os.wallpaper_current()?);
        match shown {
            Some(s) if current.starts_with(STAGE_PREFIX) => Some(s.to_string()),
            _ => Some(current),
        }
    }

    /// One minute step of the cycle (`minutes` since local midnight).
    pub fn step(&mut self, os: &dyn Shell, dirs: &Dirs<'_>, prefs: &mut fp_core::prefs::Prefs, minutes: u32) -> Action {
        let pack = prefs.get_str(KEY_PACK).map(str::to_string);
        let shown = prefs.get_str(KEY_SHOWN).map(str::to_string);
        let os_cur = Self::os_name(os, shown.as_deref());
        let action =
            daynight::step(pack.as_deref(), shown.as_deref(), os_cur.as_deref(), minutes, &day_packs(dirs.packs));
        match &action {
            Action::Nothing => {}
            Action::Show { file } => {
                if self.apply(os, dirs, file) {
                    let _ = prefs.put(KEY_SHOWN, file);
                }
            }
            Action::Stop => stop_cycle(prefs),
        }
        action
    }

    /// Run a wallpaper action from the Wallpapers tab; true when the picture is set.
    pub fn run(
        &mut self,
        a: &WallAction,
        os: &dyn Shell,
        dirs: &Dirs<'_>,
        prefs: &mut fp_core::prefs::Prefs,
        minutes: u32,
    ) -> bool {
        match a {
            WallAction::Main { .. } | WallAction::Still { .. } => {
                stop_cycle(prefs);
                let papers = wallpapers::scan(|f| dirs.packs.join(f).is_file());
                a.file(&papers).is_some_and(|f| self.apply(os, dirs, &f))
            }
            WallAction::TurnOff { pack, slot } => {
                stop_cycle(prefs);
                self.apply(os, dirs, &daynight::file_name(pack, *slot))
            }
            WallAction::TurnOn { pack } => {
                let file = daynight::file_name(pack, daynight::slot_at(minutes));
                let ok = self.apply(os, dirs, &file);
                if ok {
                    let _ = prefs.edit(|m| {
                        m.insert(KEY_PACK.into(), pack.clone().into());
                        m.insert(KEY_SHOWN.into(), file.into());
                    });
                } else {
                    stop_cycle(prefs);
                }
                ok
            }
        }
    }
}

/// Stop the cycle: forget both prefs.
pub fn stop_cycle(prefs: &mut fp_core::prefs::Prefs) {
    if prefs.get_str(KEY_PACK).is_some() || prefs.get_str(KEY_SHOWN).is_some() {
        let _ = prefs.edit(|m| {
            m.remove(KEY_PACK);
            m.remove(KEY_SHOWN);
        });
    }
}

/// The weather a "with <weather>" link sets after the picture (manual pick).
pub fn with_weather(w: &mut Weathers, effect: fp_core::effect::EffectKind) -> fp_core::settings::Changes {
    w.set_weather(Some(effect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::prefs::Prefs;
    use fp_core::settings::Changes;
    use fp_win::WallpaperScreen;
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct Os {
        set: RefCell<Vec<PathBuf>>,
        current: RefCell<Option<PathBuf>>,
        refuse: bool,
    }

    impl Shell for Os {
        fn open_url(&self, _: &str) {}
        fn login_is_on(&self) -> bool {
            false
        }
        fn login_blocked(&self) -> bool {
            false
        }
        fn login_set(&self, _: bool) -> bool {
            true
        }
        fn show_startup_apps(&self) {}
        fn taskbar_light(&self) -> bool {
            false
        }
        fn tray_icon_promoted(&self) -> bool {
            true
        }
        fn tray_icon_spot(&self, r: fp_core::geom::PixelRect) -> Option<fp_core::geom::PixelRect> {
            Some(r)
        }
        fn region_latitude(&self) -> Option<f64> {
            None
        }
        fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
            Vec::new()
        }
        fn wallpaper_set(&self, path: &Path) -> bool {
            if self.refuse {
                return false;
            }
            self.set.borrow_mut().push(path.to_path_buf());
            *self.current.borrow_mut() = Some(path.to_path_buf());
            true
        }
        fn wallpaper_set_each(&self, _: &[(String, PathBuf)]) -> bool {
            true
        }
        fn wallpaper_current(&self) -> Option<PathBuf> {
            self.current.borrow().clone()
        }
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("fp-wall-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Wallpapers")).unwrap();
            for f in ["winter.jpg", "winter-dawn.jpg", "winter-midday.jpg", "winter-night.jpg", "rain.jpg"] {
                std::fs::write(root.join("Wallpapers").join(f), b"jpg").unwrap();
            }
            Self { root }
        }
        fn dirs(&self) -> (PathBuf, PathBuf) {
            (self.root.clone(), self.root.join("Wallpapers"))
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn plain_names_only() {
        assert!(plain_name("a.jpg"));
        for bad in ["", "..", ".", "a/b.jpg", "a\\b.jpg", "c:x.jpg"] {
            assert!(!plain_name(bad), "{bad}");
        }
    }

    #[test]
    fn only_packs_with_a_dawn_follow_the_day() {
        let f = Fixture::new("packs");
        assert_eq!(day_packs(&f.dirs().1), vec!["winter".to_string()]);
    }

    #[test]
    fn main_and_still_apply_and_stop_cycle() {
        let f = Fixture::new("main");
        let (data, packs) = f.dirs();
        let dirs = Dirs { data: &data, packs: &packs };
        let os = Os::default();
        let mut prefs = Prefs::memory(Default::default());
        prefs.put(KEY_PACK, &"winter").unwrap();
        let mut w = Wallpaper::default();
        assert!(w.run(&WallAction::Main { pack: "winter".into() }, &os, &dirs, &mut prefs, 600));
        assert_eq!(prefs.get_str(KEY_PACK), None);
        let staged = os.set.borrow()[0].clone();
        assert!(staged.starts_with(data.join("wallpapers")));
        assert_eq!(std::fs::read(&staged).unwrap(), b"jpg");
        let still = WallAction::Still { pack: "winter".into(), slot: daynight::Slot::Midday };
        assert!(w.run(&still, &os, &dirs, &mut prefs, 600));
        assert_ne!(os.set.borrow()[0], os.set.borrow()[1], "staged names alternate");
        // A pack that has no picture fails.
        assert!(!w.run(&WallAction::Main { pack: "spring".into() }, &os, &dirs, &mut prefs, 600));
        let missing = WallAction::Still { pack: "winter".into(), slot: daynight::Slot::Dusk };
        assert!(!w.run(&missing, &os, &dirs, &mut prefs, 600));
    }

    #[test]
    fn os_refusal_is_a_failure() {
        let f = Fixture::new("refuse");
        let (data, packs) = f.dirs();
        let dirs = Dirs { data: &data, packs: &packs };
        let os = Os { refuse: true, ..Os::default() };
        let mut prefs = Prefs::memory(Default::default());
        let mut w = Wallpaper::default();
        assert!(!w.run(&WallAction::TurnOn { pack: "winter".into() }, &os, &dirs, &mut prefs, 700));
        assert_eq!(prefs.get_str(KEY_PACK), None);
        // Unwritable data folder: staging fails.
        let blocked = f.root.join("Wallpapers").join("winter.jpg");
        let bad = Dirs { data: &blocked, packs: &packs };
        assert!(!w.apply(&Os::default(), &bad, "winter.jpg"));
        assert!(!w.apply(&Os::default(), &dirs, "../winter.jpg"));
    }

    #[test]
    fn failed_turn_on_stops_a_running_cycle() {
        let f = Fixture::new("failon");
        let (data, packs) = f.dirs();
        let dirs = Dirs { data: &data, packs: &packs };
        let os = Os::default();
        let mut prefs = Prefs::memory(Default::default());
        let mut w = Wallpaper::default();
        assert!(w.run(&WallAction::TurnOn { pack: "winter".into() }, &os, &dirs, &mut prefs, 750));
        // Another pack without pictures: apply fails, the old cycle is gone.
        assert!(!w.run(&WallAction::TurnOn { pack: "spring".into() }, &os, &dirs, &mut prefs, 750));
        assert_eq!(prefs.get_str(KEY_PACK), None);
        assert_eq!(prefs.get_str(KEY_SHOWN), None);
    }

    #[test]
    fn cycle_turns_on_steps_and_stops() {
        let f = Fixture::new("cycle");
        let (data, packs) = f.dirs();
        let dirs = Dirs { data: &data, packs: &packs };
        let os = Os::default();
        let mut prefs = Prefs::memory(Default::default());
        let mut w = Wallpaper::default();
        // 12:30 -> midday.
        assert!(w.run(&WallAction::TurnOn { pack: "winter".into() }, &os, &dirs, &mut prefs, 750));
        assert_eq!(prefs.get_str(KEY_PACK), Some("winter"));
        assert_eq!(prefs.get_str(KEY_SHOWN), Some("winter-midday.jpg"));
        // Same minute again: nothing to do (our staged copy counts as the shown still).
        assert_eq!(w.step(&os, &dirs, &mut prefs, 751), Action::Nothing);
        assert_eq!(os.set.borrow().len(), 1);
        // 23:00 -> night: a new still.
        assert_eq!(w.step(&os, &dirs, &mut prefs, 1380), Action::Show { file: "winter-night.jpg".into() });
        assert_eq!(prefs.get_str(KEY_SHOWN), Some("winter-night.jpg"));
        // The user picks another wallpaper: the cycle stops.
        *os.current.borrow_mut() = Some(PathBuf::from("C:/Users/x/Pictures/mine.jpg"));
        assert_eq!(w.step(&os, &dirs, &mut prefs, 1381), Action::Stop);
        assert_eq!(prefs.get_str(KEY_PACK), None);
        assert_eq!(prefs.get_str(KEY_SHOWN), None);
        // No cycle: nothing happens.
        assert_eq!(w.step(&os, &dirs, &mut prefs, 1382), Action::Nothing);
        stop_cycle(&mut prefs);
    }

    #[test]
    fn turn_off_freezes_a_still() {
        let f = Fixture::new("off");
        let (data, packs) = f.dirs();
        let dirs = Dirs { data: &data, packs: &packs };
        let os = Os::default();
        let mut prefs = Prefs::memory(Default::default());
        let mut w = Wallpaper::default();
        w.run(&WallAction::TurnOn { pack: "winter".into() }, &os, &dirs, &mut prefs, 750);
        let off = WallAction::TurnOff { pack: "winter".into(), slot: daynight::Slot::Night };
        assert!(w.run(&off, &os, &dirs, &mut prefs, 1380));
        assert_eq!(prefs.get_str(KEY_PACK), None);
    }

    #[test]
    fn with_weather_sets_the_effect() {
        let mut w = Weathers::default();
        let c = with_weather(&mut w, fp_core::effect::EffectKind::Rain);
        assert_ne!(c, Changes::NONE);
        assert_eq!(w.settings.weather, Some(fp_core::effect::EffectKind::Rain));
    }
}
