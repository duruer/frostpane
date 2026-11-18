//! Pure models behind the main window: tabs, settings panes, screens rows,
//! visitor switches and window sizing (ui 1, 3.5; core-app 2.3).

use fp_core::effect::Visitor;
use fp_core::settings::Settings;
use fp_text::Key;

/// Home tabs (the Friends tab is dropped).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Effects,
    Wallpapers,
    HowTo,
    Changelog,
    Settings,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Effects, Tab::Wallpapers, Tab::HowTo, Tab::Changelog, Tab::Settings];

    pub fn title(self) -> Key {
        match self {
            Tab::Effects => Key::TabEffects,
            Tab::Wallpapers => Key::TabWallpapers,
            Tab::HowTo => Key::TabHowTo,
            Tab::Changelog => Key::TabChangelog,
            Tab::Settings => Key::TabSettings,
        }
    }
}

/// Settings panes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    General,
    Screens,
    About,
    Help,
}

impl Pane {
    pub fn title(self) -> Key {
        match self {
            Pane::General => Key::SetPrefs,
            Pane::Screens => Key::SetScreens,
            Pane::About => Key::SetAbout,
            Pane::Help => Key::SetHelp,
        }
    }

    /// Detail line under the page title (only Screens, About, Help have one).
    pub fn detail(self) -> Option<Key> {
        match self {
            Pane::General => None,
            Pane::Screens => Some(Key::SetScreensDetail),
            Pane::About => Some(Key::SetAboutDetail),
            Pane::Help => Some(Key::SetHelpDetail),
        }
    }

    /// Icon-font style glyph name (drawn as vector glyphs by the widgets).
    pub fn glyph(self) -> Glyph {
        match self {
            Pane::General => Glyph::Gear,
            Pane::Screens => Glyph::Monitor,
            Pane::About => Glyph::Shield,
            Pane::Help => Glyph::Question,
        }
    }
}

/// Small vector glyphs the widgets draw themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Gear,
    Monitor,
    Shield,
    Question,
    Check,
    Chevron,
    Sun,
    Minimise,
    Close,
    Battery,
    Behind,
    Screens,
    SnowOnly,
}

/// Panes shown in the nav: Screens only with 2 or more displays.
pub fn panes(display_count: usize) -> Vec<Pane> {
    let mut v = vec![Pane::General];
    if display_count >= 2 {
        v.push(Pane::Screens);
    }
    v.push(Pane::About);
    v.push(Pane::Help);
    v
}

/// Pane to open: a remembered Screens pane falls back to General with fewer
/// than 2 displays.
pub fn pane_to_open(remembered: Pane, display_count: usize) -> Pane {
    if remembered == Pane::Screens && display_count < 2 { Pane::General } else { remembered }
}

/// One attached display as the host reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
}

/// One row of the Screens pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenRow {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Shown as the "Main" pill (enabled main screen).
    pub is_main: bool,
    /// Shown as the "make main" link (enabled, not main).
    pub can_make_main: bool,
    /// The switch is disabled when this is the only enabled screen.
    pub switch_enabled: bool,
}

/// Main screen id: the stored one if it names a display, else the first.
pub fn main_screen_id<'a>(s: &'a Settings, displays: &'a [DisplayInfo]) -> Option<&'a str> {
    s.main_screen
        .as_deref()
        .filter(|id| displays.iter().any(|d| d.id == *id))
        .or_else(|| displays.first().map(|d| d.id.as_str()))
}

pub fn screen_rows(s: &Settings, displays: &[DisplayInfo]) -> Vec<ScreenRow> {
    let main = main_screen_id(s, displays);
    let enabled_count = displays.iter().filter(|d| s.screen_on(&d.id)).count();
    displays
        .iter()
        .map(|d| {
            let enabled = s.screen_on(&d.id);
            let is_main = enabled && main == Some(d.id.as_str());
            ScreenRow {
                id: d.id.clone(),
                name: d.name.clone(),
                enabled,
                is_main,
                can_make_main: enabled && !is_main,
                switch_enabled: !(enabled && enabled_count <= 1),
            }
        })
        .collect()
}

/// Master visitors switch: on when fewer than all four visitors are away.
pub fn visitors_master_on(s: &Settings) -> bool {
    Visitor::ALL.iter().filter(|v| !s.comes(**v)).count() < Visitor::ALL.len()
}

/// Key of the child switch of a visitor.
pub fn visitor_key(v: Visitor) -> Key {
    match v {
        Visitor::Santa => Key::VisitSanta,
        Visitor::Snowmen => Key::VisitSnowmen,
        Visitor::Yeti => Key::VisitYeti,
        Visitor::Plough => Key::VisitPlough,
    }
}

/// Home window width (ui 1.1).
pub const HOME_WIDTH: f32 = 1078.0;

/// Home window height: `min(735, 0.9 * work height)`.
pub fn home_height(work_height: f32) -> f32 {
    (0.9 * work_height).min(735.0)
}

/// True when the window's inner size (points) is off the size it was fitted
/// to by more than rounding: the fixed-size window was resized by something
/// else (a scale change mid-drag between monitors).
pub fn size_drifted(actual: (f32, f32), want: (f32, f32)) -> bool {
    (actual.0 - want.0).abs() > 1.5 || (actual.1 - want.1).abs() > 1.5
}

/// New top-left when the window changes size while visible: keep the centre,
/// top at least the work-area top; unchanged when both deltas are < 0.5.
pub fn fit(pos: (f32, f32), old: (f32, f32), new: (f32, f32), work_top: f32) -> (f32, f32) {
    if (old.0 - new.0).abs() < 0.5 && (old.1 - new.1).abs() < 0.5 {
        return pos;
    }
    (pos.0 + (old.0 - new.0) / 2.0, work_top.max(pos.1 + (old.1 - new.1) / 2.0))
}

/// Screen the main window shows (core-app 2.3: seenWelcome decides).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Onboarding,
    Home,
}

pub fn pick_screen(seen_welcome: bool) -> Screen {
    if seen_welcome { Screen::Home } else { Screen::Onboarding }
}

/// Theme of a screen: onboarding light, Home dark.
pub fn screen_is_dark(screen: Screen) -> bool {
    screen == Screen::Home
}

#[cfg(test)]
mod tests {
    use super::*;

    fn displays(n: usize) -> Vec<DisplayInfo> {
        (0..n).map(|i| DisplayInfo { id: format!("d{i}"), name: format!("Display {i}") }).collect()
    }

    #[test]
    fn nav_and_fallback() {
        assert_eq!(panes(1), [Pane::General, Pane::About, Pane::Help]);
        assert_eq!(panes(2), [Pane::General, Pane::Screens, Pane::About, Pane::Help]);
        assert_eq!(pane_to_open(Pane::Screens, 1), Pane::General);
        assert_eq!(pane_to_open(Pane::Screens, 2), Pane::Screens);
        assert_eq!(pane_to_open(Pane::Help, 1), Pane::Help);
        assert_eq!(Pane::General.detail(), None);
        for p in [Pane::Screens, Pane::About, Pane::Help] {
            assert!(p.detail().is_some());
        }
        for p in [Pane::General, Pane::Screens, Pane::About, Pane::Help] {
            let _ = (p.title(), p.glyph());
        }
        for t in Tab::ALL {
            let _ = t.title();
        }
    }

    #[test]
    fn screens_rows_rules() {
        let d = displays(3);
        let mut s = Settings::default();
        let rows = screen_rows(&s, &d);
        assert!(rows[0].is_main && !rows[0].can_make_main);
        assert!(rows[1].can_make_main && !rows[1].is_main);
        assert!(rows.iter().all(|r| r.switch_enabled && r.enabled));
        s.main_screen = Some("d2".into());
        assert!(screen_rows(&s, &d)[2].is_main);
        // An unknown stored main falls back to the first display.
        s.main_screen = Some("gone".into());
        assert_eq!(main_screen_id(&s, &d), Some("d0"));
        // Disable two screens: the last enabled one cannot be switched off.
        s.screens_off = vec!["d0".into(), "d1".into()];
        let rows = screen_rows(&s, &d);
        assert!(!rows[0].enabled && !rows[0].is_main && !rows[0].can_make_main && rows[0].switch_enabled);
        assert!(rows[2].enabled && !rows[2].switch_enabled);
        assert_eq!(main_screen_id(&Settings::default(), &[]), None);
    }

    #[test]
    fn visitors_master() {
        let mut s = Settings::default();
        assert!(visitors_master_on(&s));
        s.visitors_away = Visitor::ALL.to_vec();
        assert!(!visitors_master_on(&s));
        s.visitors_away = vec![Visitor::Yeti];
        assert!(visitors_master_on(&s));
        for v in Visitor::ALL {
            let _ = visitor_key(v);
        }
    }

    #[test]
    fn window_sizes() {
        assert_eq!(home_height(1000.0), 735.0);
        assert_eq!(home_height(600.0), 540.0);
        assert_eq!(HOME_WIDTH, 1078.0);
        // Keep the centre.
        assert_eq!(fit((100.0, 100.0), (860.0, 620.0), (1078.0, 735.0), 0.0), (-9.0, 42.5));
        // Never above the work area.
        assert_eq!(fit((100.0, 10.0), (860.0, 620.0), (1078.0, 735.0), 0.0).1, 0.0);
        // Small change is skipped.
        assert_eq!(fit((5.0, 6.0), (100.0, 100.0), (100.2, 100.3), 0.0), (5.0, 6.0));
    }

    #[test]
    fn screens_and_themes() {
        assert_eq!(pick_screen(false), Screen::Onboarding);
        assert_eq!(pick_screen(true), Screen::Home);
        assert!(screen_is_dark(Screen::Home));
        assert!(!screen_is_dark(Screen::Onboarding));
    }
}
