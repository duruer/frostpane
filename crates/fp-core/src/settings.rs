//! App settings model (core-app 2.5, weather-sources 2.1-2.6).
//!
//! [`Settings`] holds the persisted values with the spec's lenient load rules.
//! [`Weathers`] adds the runtime inputs (outside weather, random/season picks,
//! battery) and implements the mutually exclusive weather sources and the
//! derived values (`shown_weather`, `snow_strength`, ...). Every mutating call
//! returns a [`Changes`] set so the app can persist once and notify listeners.

use crate::effect::{EffectKind, Lang, Visitor};
use crate::wind::Wind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Persisted keys (settings.json). Frostpane-only keys are marked.
pub mod keys {
    pub const WEATHER: &str = "weather";
    pub const SEEN_WELCOME: &str = "seenWelcome";
    pub const HEAVY_SNOW: &str = "heavySnow";
    pub const BEHIND_WINDOWS: &str = "behindWindows";
    pub const KEEP_CLEAR: &str = "keepClear";
    pub const SCREENS_OFF: &str = "screensOff";
    pub const MAIN_SCREEN: &str = "mainScreen";
    pub const SNOWBALLS: &str = "snowballs";
    pub const PAUSE_ON_BATTERY: &str = "pauseOnBattery";
    pub const MATCH_WEATHER: &str = "matchWeather";
    pub const RANDOM_WEATHER: &str = "randomWeather";
    pub const MATCH_SEASON: &str = "matchSeason";
    pub const LIGHTNING: &str = "lightning";
    pub const WIND: &str = "wind";
    pub const VISITORS_AWAY: &str = "visitorsAway";
    pub const OFTEN_VISITS: &str = "oftenVisits";
    pub const LANGUAGE: &str = "language";
    /// Frostpane: city used for "match my weather" (Open-Meteo).
    pub const CITY: &str = "city";
    /// Frostpane: max standing trees and max decorations (each).
    pub const MAX_PLACED: &str = "maxPlaced";
}

/// Strength multiplier of heavy snow.
pub const HEAVY_SHARE: f64 = 2.5;
/// Default for [`Settings::max_placed`].
pub const DEFAULT_MAX_PLACED: u32 = 100;

/// A geocoded place picked by the user (Open-Meteo geocoding result).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct City {
    pub name: String,
    #[serde(default)]
    pub admin: String,
    #[serde(default)]
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Persisted user settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub weather: Option<EffectKind>,
    pub seen_welcome: bool,
    pub heavy_snow: bool,
    pub behind_windows: bool,
    pub keep_clear: bool,
    pub screens_off: Vec<String>,
    pub main_screen: Option<String>,
    pub snowballs: bool,
    pub pause_on_battery: bool,
    pub match_weather: bool,
    pub random_weather: bool,
    pub match_season: bool,
    pub lightning: bool,
    pub wind: Wind,
    /// Sorted, unique.
    pub visitors_away: Vec<Visitor>,
    pub often_visits: bool,
    pub language: Option<Lang>,
    pub city: Option<City>,
    pub max_placed: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            weather: None,
            seen_welcome: false,
            heavy_snow: false,
            behind_windows: true,
            keep_clear: false,
            screens_off: Vec::new(),
            main_screen: None,
            snowballs: true,
            pause_on_battery: false,
            match_weather: false,
            random_weather: false,
            match_season: false,
            lightning: true,
            wind: Wind::None,
            visitors_away: Vec::new(),
            often_visits: false,
            language: None,
            city: None,
            max_placed: DEFAULT_MAX_PLACED,
        }
    }
}

fn get_bool(m: &Map<String, Value>, k: &str) -> bool {
    m.get(k) == Some(&Value::Bool(true))
}

/// Absent, null or non-bool -> true.
fn get_bool_default_true(m: &Map<String, Value>, k: &str) -> bool {
    !matches!(m.get(k), Some(Value::Bool(false)))
}

fn get_str<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

/// A JSON array of strings; anything else -> empty.
fn get_strings(m: &Map<String, Value>, k: &str) -> Vec<String> {
    match m.get(k) {
        Some(Value::Array(a)) if a.iter().all(Value::is_string) => {
            a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
        }
        _ => Vec::new(),
    }
}

impl Settings {
    /// Lenient load (core-app 2.5 load rules). The second value is true when
    /// normalisation changed something that should be written back.
    pub fn from_map(m: &Map<String, Value>) -> (Self, bool) {
        let mut s = Settings {
            weather: get_str(m, keys::WEATHER).and_then(EffectKind::parse),
            seen_welcome: get_bool(m, keys::SEEN_WELCOME),
            heavy_snow: get_bool(m, keys::HEAVY_SNOW),
            behind_windows: get_bool_default_true(m, keys::BEHIND_WINDOWS),
            keep_clear: get_bool(m, keys::KEEP_CLEAR),
            screens_off: get_strings(m, keys::SCREENS_OFF),
            main_screen: get_str(m, keys::MAIN_SCREEN).map(str::to_owned),
            snowballs: get_bool_default_true(m, keys::SNOWBALLS),
            pause_on_battery: get_bool(m, keys::PAUSE_ON_BATTERY),
            match_weather: get_bool(m, keys::MATCH_WEATHER),
            random_weather: false,
            match_season: false,
            lightning: get_bool_default_true(m, keys::LIGHTNING),
            wind: get_str(m, keys::WIND).map(Wind::parse).unwrap_or_default(),
            visitors_away: Vec::new(),
            often_visits: get_bool(m, keys::OFTEN_VISITS),
            language: get_str(m, keys::LANGUAGE).and_then(Lang::parse),
            city: m.get(keys::CITY).and_then(|v| serde_json::from_value(v.clone()).ok()),
            max_placed: m
                .get(keys::MAX_PLACED)
                .and_then(Value::as_u64)
                .map(|v| v.clamp(1, 10_000) as u32)
                .unwrap_or(DEFAULT_MAX_PLACED),
        };
        s.random_weather = get_bool(m, keys::RANDOM_WEATHER) && !s.match_weather;
        s.match_season = get_bool(m, keys::MATCH_SEASON) && !s.match_weather && !s.random_weather;
        let mut away: Vec<Visitor> =
            get_strings(m, keys::VISITORS_AWAY).iter().filter_map(|v| Visitor::parse(v)).collect();
        away.sort();
        away.dedup();
        s.visitors_away = away;
        let mut dirty = false;
        if s.any_mode() && s.weather.is_some() {
            s.weather = None;
            dirty = true;
        }
        (s, dirty)
    }

    /// Writes every known key into `m`, keeping unknown keys.
    pub fn write_into(&self, m: &mut Map<String, Value>) {
        let w = self.weather.map_or("none", EffectKind::raw);
        m.insert(keys::WEATHER.into(), Value::from(w));
        let bools = [
            (keys::SEEN_WELCOME, self.seen_welcome),
            (keys::HEAVY_SNOW, self.heavy_snow),
            (keys::BEHIND_WINDOWS, self.behind_windows),
            (keys::KEEP_CLEAR, self.keep_clear),
            (keys::SNOWBALLS, self.snowballs),
            (keys::PAUSE_ON_BATTERY, self.pause_on_battery),
            (keys::MATCH_WEATHER, self.match_weather),
            (keys::RANDOM_WEATHER, self.random_weather),
            (keys::MATCH_SEASON, self.match_season),
            (keys::LIGHTNING, self.lightning),
            (keys::OFTEN_VISITS, self.often_visits),
        ];
        for (k, v) in bools {
            m.insert(k.into(), Value::Bool(v));
        }
        m.insert(keys::SCREENS_OFF.into(), Value::from(self.screens_off.clone()));
        let away: Vec<&str> = self.visitors_away.iter().map(|v| v.raw()).collect();
        m.insert(keys::VISITORS_AWAY.into(), Value::from(away));
        m.insert(keys::WIND.into(), Value::from(self.wind.raw()));
        m.insert(keys::MAX_PLACED.into(), Value::from(self.max_placed));
        let opt = [
            (keys::MAIN_SCREEN, self.main_screen.clone().map(Value::from)),
            (keys::LANGUAGE, self.language.map(|l| Value::from(l.code()))),
            (keys::CITY, self.city.as_ref().and_then(|c| serde_json::to_value(c).ok())),
        ];
        for (k, v) in opt {
            match v {
                Some(v) => m.insert(k.into(), v),
                None => m.remove(k),
            };
        }
    }

    /// Any automatic weather source enabled.
    pub fn any_mode(&self) -> bool {
        self.match_weather || self.random_weather || self.match_season
    }

    /// Does visitor `v` come on its own?
    pub fn comes(&self, v: Visitor) -> bool {
        !self.visitors_away.contains(&v)
    }

    /// Is the monitor with this id enabled?
    pub fn screen_on(&self, id: &str) -> bool {
        !self.screens_off.iter().any(|s| s == id)
    }
}

/// Set of changed properties (persisted fields plus derived values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes(pub u32);

impl Changes {
    pub const NONE: Changes = Changes(0);
    pub const WEATHER: Changes = Changes(1);
    pub const HEAVY_SNOW: Changes = Changes(1 << 1);
    pub const LIGHTNING: Changes = Changes(1 << 2);
    pub const MATCH_WEATHER: Changes = Changes(1 << 3);
    pub const RANDOM_WEATHER: Changes = Changes(1 << 4);
    pub const MATCH_SEASON: Changes = Changes(1 << 5);
    pub const PAUSE_ON_BATTERY: Changes = Changes(1 << 6);
    pub const SHOWN_WEATHER: Changes = Changes(1 << 7);
    pub const SNOW_STRENGTH: Changes = Changes(1 << 8);
    pub const LIGHTNING_SHOWN: Changes = Changes(1 << 9);
    pub const WEATHER_ON: Changes = Changes(1 << 10);
    pub const PAUSED_FOR_BATTERY: Changes = Changes(1 << 11);
    pub const VISITORS: Changes = Changes(1 << 12);
    pub const SCREENS: Changes = Changes(1 << 13);
    pub const OTHER: Changes = Changes(1 << 14);
    /// Bits that mean "settings.json must be rewritten".
    pub const PERSISTED: Changes = Changes(0x7F | (1 << 12) | (1 << 13) | (1 << 14));

    pub fn contains(self, o: Changes) -> bool {
        self.0 & o.0 == o.0 && o.0 != 0
    }
    pub fn any(self, o: Changes) -> bool {
        self.0 & o.0 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// True when something persisted changed.
    pub fn needs_save(self) -> bool {
        self.any(Changes::PERSISTED)
    }
}

impl std::ops::BitOr for Changes {
    type Output = Changes;
    fn bitor(self, o: Changes) -> Changes {
        Changes(self.0 | o.0)
    }
}

impl std::ops::BitOrAssign for Changes {
    fn bitor_assign(&mut self, o: Changes) {
        self.0 |= o.0;
    }
}

/// A "match my weather" result from the weather source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outside {
    pub weather: Option<EffectKind>,
    pub heavy: bool,
    pub lightning: bool,
}

/// Settings plus runtime weather inputs; owner of all weather-source rules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Weathers {
    pub settings: Settings,
    pub outside: Option<EffectKind>,
    pub outside_heavy: bool,
    pub outside_lightning: bool,
    pub random_pick: Option<EffectKind>,
    pub season_pick: Option<EffectKind>,
    pub on_battery: bool,
}

#[derive(PartialEq)]
struct Snap {
    s: Settings,
    shown: Option<EffectKind>,
    strength: f64,
    lightning_shown: bool,
    on: bool,
    paused: bool,
}

impl Weathers {
    pub fn new(settings: Settings, on_battery: bool) -> Self {
        Self { settings, on_battery, ..Default::default() }
    }

    /// What the overlay renders (weather-sources 2.1).
    pub fn shown_weather(&self) -> Option<EffectKind> {
        let s = &self.settings;
        if s.pause_on_battery && self.on_battery {
            None
        } else if s.match_weather {
            self.outside
        } else if s.random_weather {
            self.random_pick
        } else if s.match_season {
            self.season_pick
        } else {
            s.weather
        }
    }

    /// Snow amount multiplier: 2.5 for heavy snow, else 1.
    pub fn snow_strength(&self) -> f64 {
        let heavy = if self.settings.match_weather { self.outside_heavy } else { self.settings.heavy_snow };
        if self.shown_weather() == Some(EffectKind::Snow) && heavy { HEAVY_SHARE } else { 1.0 }
    }

    pub fn lightning_shown(&self) -> bool {
        if self.settings.match_weather { self.outside_lightning } else { self.settings.lightning }
    }

    /// Manual weather set or any automatic mode on (ignores battery pause).
    pub fn weather_on(&self) -> bool {
        self.settings.weather.is_some() || self.settings.any_mode()
    }

    pub fn paused_for_battery(&self) -> bool {
        self.settings.pause_on_battery && self.on_battery && self.weather_on()
    }

    fn snap(&self) -> Snap {
        Snap {
            s: self.settings.clone(),
            shown: self.shown_weather(),
            strength: self.snow_strength(),
            lightning_shown: self.lightning_shown(),
            on: self.weather_on(),
            paused: self.paused_for_battery(),
        }
    }

    fn diff(&self, b: Snap) -> Changes {
        let a = self.snap();
        let (x, y) = (&a.s, &b.s);
        let mut c = Changes::NONE;
        let flags = [
            (x.weather != y.weather, Changes::WEATHER),
            (x.heavy_snow != y.heavy_snow, Changes::HEAVY_SNOW),
            (x.lightning != y.lightning, Changes::LIGHTNING),
            (x.match_weather != y.match_weather, Changes::MATCH_WEATHER),
            (x.random_weather != y.random_weather, Changes::RANDOM_WEATHER),
            (x.match_season != y.match_season, Changes::MATCH_SEASON),
            (x.pause_on_battery != y.pause_on_battery, Changes::PAUSE_ON_BATTERY),
            (a.shown != b.shown, Changes::SHOWN_WEATHER),
            (a.strength != b.strength, Changes::SNOW_STRENGTH),
            (a.lightning_shown != b.lightning_shown, Changes::LIGHTNING_SHOWN),
            (a.on != b.on, Changes::WEATHER_ON),
            (a.paused != b.paused, Changes::PAUSED_FOR_BATTERY),
            (x.visitors_away != y.visitors_away, Changes::VISITORS),
            (x.screens_off != y.screens_off || x.main_screen != y.main_screen, Changes::SCREENS),
        ];
        for (hit, flag) in flags {
            if hit {
                c |= flag;
            }
        }
        c
    }

    fn edit(&mut self, f: impl FnOnce(&mut Self)) -> Changes {
        let before = self.snap();
        f(self);
        self.diff(before)
    }

    /// Manual pick. A value turns the three automatic modes off.
    pub fn set_weather(&mut self, w: Option<EffectKind>) -> Changes {
        self.edit(|me| me.apply_weather(w))
    }

    fn apply_weather(&mut self, w: Option<EffectKind>) {
        self.settings.weather = w;
        if w.is_some() {
            self.apply_match_weather(false);
            self.apply_random(false);
            self.apply_season(false);
        }
    }

    fn apply_match_weather(&mut self, on: bool) {
        self.settings.match_weather = on;
        if on {
            self.settings.weather = None;
            self.settings.heavy_snow = false;
            self.apply_random(false);
            self.apply_season(false);
        } else {
            self.outside_heavy = false;
            self.outside_lightning = false;
            self.outside = None;
        }
    }

    fn apply_random(&mut self, on: bool) {
        self.settings.random_weather = on;
        if on {
            self.settings.weather = None;
            self.settings.heavy_snow = false;
            self.settings.match_weather = false;
            self.outside = None;
            self.outside_heavy = false;
            self.outside_lightning = false;
            self.settings.match_season = false;
            self.season_pick = None;
        } else {
            self.random_pick = None;
        }
    }

    fn apply_season(&mut self, on: bool) {
        self.settings.match_season = on;
        if on {
            self.settings.weather = None;
            self.settings.heavy_snow = false;
            self.settings.match_weather = false;
            self.outside = None;
            self.outside_heavy = false;
            self.outside_lightning = false;
            self.settings.random_weather = false;
            self.random_pick = None;
        } else {
            self.season_pick = None;
        }
    }

    pub fn set_match_weather(&mut self, on: bool) -> Changes {
        self.edit(|me| me.apply_match_weather(on))
    }

    pub fn set_random_weather(&mut self, on: bool) -> Changes {
        self.edit(|me| me.apply_random(on))
    }

    pub fn set_match_season(&mut self, on: bool) -> Changes {
        self.edit(|me| me.apply_season(on))
    }

    pub fn set_heavy_snow(&mut self, on: bool) -> Changes {
        self.edit(|me| me.settings.heavy_snow = on)
    }

    pub fn set_lightning(&mut self, on: bool) -> Changes {
        self.edit(|me| me.settings.lightning = on)
    }

    pub fn set_pause_on_battery(&mut self, on: bool) -> Changes {
        self.edit(|me| me.settings.pause_on_battery = on)
    }

    /// "Off": manual none and all modes off.
    pub fn turn_off(&mut self) -> Changes {
        self.edit(|me| {
            me.settings.weather = None;
            me.apply_match_weather(false);
            me.apply_random(false);
            me.apply_season(false);
        })
    }

    /// Apply a "match my weather" result; ignored when the mode is off
    /// (late replies are dropped, weather-sources 2.3).
    pub fn set_outside(&mut self, o: Outside) -> Changes {
        self.edit(|me| {
            if me.settings.match_weather {
                me.outside_heavy = o.heavy;
                me.outside_lightning = o.lightning;
                me.outside = o.weather;
            }
        })
    }

    /// Ignored unless random mode is on.
    pub fn set_random_pick(&mut self, p: Option<EffectKind>) -> Changes {
        self.edit(|me| {
            if me.settings.random_weather {
                me.random_pick = p;
            }
        })
    }

    /// Ignored unless season mode is on.
    pub fn set_season_pick(&mut self, p: Option<EffectKind>) -> Changes {
        self.edit(|me| {
            if me.settings.match_season {
                me.season_pick = p;
            }
        })
    }

    pub fn set_on_battery(&mut self, on: bool) -> Changes {
        self.edit(|me| me.on_battery = on)
    }

    /// Tray "snow" / "heavy snow" items (core-app 2.6 variant clicks).
    pub fn tray_snow(&mut self, heavy_item: bool) -> Changes {
        self.edit(|me| {
            let manual_snow = me.settings.weather == Some(EffectKind::Snow);
            let heavy_active = manual_snow && me.settings.heavy_snow;
            let normal_active = manual_snow && !me.settings.heavy_snow;
            if heavy_item {
                me.settings.heavy_snow = !heavy_active;
                me.apply_weather(if heavy_active { None } else { Some(EffectKind::Snow) });
            } else {
                me.settings.heavy_snow = false;
                me.apply_weather(if normal_active { None } else { Some(EffectKind::Snow) });
            }
        })
    }

    /// Tray "rain" / "thunderstorm" items.
    pub fn tray_rain(&mut self, storm_item: bool) -> Changes {
        self.edit(|me| {
            let active = me.settings.weather == Some(EffectKind::Rain) && me.settings.lightning == storm_item;
            me.settings.lightning = storm_item;
            me.apply_weather(if active { None } else { Some(EffectKind::Rain) });
        })
    }

    /// Toggle a weather on/off (tray items for other effects, effects grid tiles).
    pub fn toggle_weather(&mut self, e: EffectKind) -> Changes {
        self.edit(|me| {
            let next = if me.settings.weather == Some(e) { None } else { Some(e) };
            me.apply_weather(next);
        })
    }

    /// Effects grid two-way toggles: set the variant and make the effect active.
    pub fn grid_variant(&mut self, e: EffectKind, second: bool) -> Changes {
        self.edit(|me| {
            match e {
                EffectKind::Rain => me.settings.lightning = second,
                _ => me.settings.heavy_snow = second,
            }
            if me.settings.weather != Some(e) {
                me.apply_weather(Some(e));
            }
        })
    }

    /// Add/remove a visitor from the away set (kept sorted).
    pub fn set_visitor(&mut self, v: Visitor, comes: bool) -> Changes {
        self.edit(|me| {
            let away = &mut me.settings.visitors_away;
            away.retain(|x| *x != v);
            if !comes {
                away.push(v);
                away.sort();
            }
        })
    }

    /// Enable/disable a monitor by id.
    pub fn set_screen(&mut self, id: &str, on: bool) -> Changes {
        self.edit(|me| {
            let off = &mut me.settings.screens_off;
            off.retain(|x| x != id);
            if !on {
                off.push(id.to_owned());
            }
        })
    }

    pub fn set_main_screen(&mut self, id: Option<String>) -> Changes {
        self.edit(|me| me.settings.main_screen = id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn defaults_from_empty() {
        let (s, dirty) = Settings::from_map(&Map::new());
        assert_eq!(s, Settings::default());
        assert!(!dirty);
        assert!(s.behind_windows && s.snowballs && s.lightning);
    }

    #[test]
    fn lenient_load() {
        let (s, dirty) = Settings::from_map(&map(json!({
            "weather": "rain", "seenWelcome": true, "heavySnow": "yes",
            "behindWindows": null, "snowballs": 3, "lightning": false,
            "screensOff": ["a", 1], "mainScreen": "m", "wind": "strongLeft",
            "visitorsAway": ["yeti", "santa", "elf", "yeti"], "oftenVisits": true,
            "language": "ko", "matchWeather": false, "randomWeather": true, "matchSeason": true,
            "city": {"name": "Oslo", "latitude": 59.9, "longitude": 10.7},
            "maxPlaced": 0, "unknown": 1
        })));
        assert!(dirty, "weather cleared because random mode is on");
        assert_eq!(s.weather, None);
        assert!(s.seen_welcome && !s.heavy_snow);
        assert!(s.behind_windows && s.snowballs && !s.lightning);
        assert!(s.screens_off.is_empty());
        assert_eq!(s.main_screen.as_deref(), Some("m"));
        assert_eq!(s.wind, Wind::StrongLeft);
        assert_eq!(s.visitors_away, vec![Visitor::Santa, Visitor::Yeti]);
        assert_eq!(s.language, Some(Lang::Ko));
        assert!(s.random_weather && !s.match_season);
        assert_eq!(s.city.as_ref().unwrap().name, "Oslo");
        assert_eq!(s.max_placed, 1);
        assert!(!s.comes(Visitor::Yeti) && s.comes(Visitor::Plough));
        assert!(s.screen_on("a"));
        let mut t = s.clone();
        t.screens_off = vec!["b".into()];
        assert!(!t.screen_on("b") && t.screen_on("a"));
    }

    #[test]
    fn match_weather_beats_others_on_load() {
        let (s, _) = Settings::from_map(&map(json!({
            "matchWeather": true, "randomWeather": true, "matchSeason": true, "screensOff": "x"
        })));
        assert!(s.match_weather && !s.random_weather && !s.match_season);
        let (s, _) = Settings::from_map(&map(json!({"matchSeason": true})));
        assert!(s.match_season);
    }

    #[test]
    fn write_round_trip_keeps_unknown() {
        let mut m = map(json!({"unknown": 5, "mainScreen": "old", "language": "en", "city": {}}));
        let mut s = Settings {
            weather: Some(EffectKind::Leaves),
            screens_off: vec!["d1".into()],
            visitors_away: vec![Visitor::Santa],
            ..Settings::default()
        };
        s.write_into(&mut m);
        assert_eq!(m["unknown"], json!(5));
        assert!(!m.contains_key("mainScreen") && !m.contains_key("language") && !m.contains_key("city"));
        assert_eq!(m["weather"], json!("leaves"));
        assert_eq!(Settings::from_map(&m).0, s);
        s.weather = None;
        s.main_screen = Some("d2".into());
        s.language = Some(Lang::Ja);
        s.city = Some(City { name: "A".into(), admin: "".into(), country: "B".into(), latitude: 1.0, longitude: 2.0 });
        s.write_into(&mut m);
        assert_eq!(m["weather"], json!("none"));
        assert_eq!(Settings::from_map(&m).0, s);
    }

    #[test]
    fn changes_ops() {
        let c = Changes::WEATHER | Changes::SHOWN_WEATHER;
        assert!(c.contains(Changes::WEATHER));
        assert!(!c.contains(Changes::NONE));
        assert!(c.any(Changes::SHOWN_WEATHER));
        assert!(!c.is_empty() && Changes::NONE.is_empty());
        assert!(c.needs_save());
        assert!(!Changes::SHOWN_WEATHER.needs_save());
        assert!(!(Changes::SNOW_STRENGTH | Changes::LIGHTNING_SHOWN | Changes::WEATHER_ON).needs_save());
        assert!(!Changes::PAUSED_FOR_BATTERY.needs_save());
        for f in [Changes::VISITORS, Changes::SCREENS, Changes::OTHER, Changes::PAUSE_ON_BATTERY] {
            assert!(f.needs_save());
        }
    }

    #[test]
    fn exclusive_modes() {
        let mut w = Weathers::new(Settings::default(), false);
        let c = w.set_weather(Some(EffectKind::Snow));
        assert!(c.contains(Changes::WEATHER | Changes::SHOWN_WEATHER | Changes::WEATHER_ON));
        assert_eq!(w.shown_weather(), Some(EffectKind::Snow));

        let c = w.set_match_weather(true);
        assert!(c.contains(Changes::MATCH_WEATHER | Changes::WEATHER));
        assert_eq!(w.shown_weather(), None);
        w.set_outside(Outside { weather: Some(EffectKind::Snow), heavy: true, lightning: false });
        assert_eq!(w.shown_weather(), Some(EffectKind::Snow));
        assert_eq!(w.snow_strength(), HEAVY_SHARE);
        assert!(!w.lightning_shown());

        w.set_random_weather(true);
        assert!(!w.settings.match_weather && w.outside.is_none());
        w.set_random_pick(Some(EffectKind::Rain));
        assert_eq!(w.shown_weather(), Some(EffectKind::Rain));
        assert!(w.lightning_shown());

        w.set_match_season(true);
        assert!(!w.settings.random_weather && w.random_pick.is_none());
        w.set_season_pick(Some(EffectKind::Leaves));
        assert_eq!(w.shown_weather(), Some(EffectKind::Leaves));

        w.set_random_weather(true);
        assert!(!w.settings.match_season && w.season_pick.is_none());

        // picks are ignored when their mode is off
        w.set_match_season(false);
        w.set_season_pick(Some(EffectKind::Snow));
        assert_eq!(w.season_pick, None);
        w.set_random_weather(false);
        w.set_random_pick(Some(EffectKind::Snow));
        assert_eq!(w.random_pick, None);
        w.set_outside(Outside { weather: Some(EffectKind::Rain), heavy: false, lightning: true });
        assert_eq!(w.outside, None);

        w.set_weather(Some(EffectKind::Rain));
        w.set_match_season(true);
        w.set_weather(Some(EffectKind::Sakura));
        assert!(!w.settings.match_season);
        w.set_match_weather(true);
        let c = w.turn_off();
        assert!(c.contains(Changes::MATCH_WEATHER | Changes::WEATHER_ON));
        assert!(!w.weather_on());
    }

    #[test]
    fn battery_and_strength() {
        let mut w = Weathers::new(Settings::default(), true);
        w.set_weather(Some(EffectKind::Snow));
        assert_eq!(w.snow_strength(), 1.0);
        w.set_heavy_snow(true);
        assert_eq!(w.snow_strength(), HEAVY_SHARE);
        let c = w.set_pause_on_battery(true);
        assert!(c.contains(Changes::SHOWN_WEATHER | Changes::PAUSED_FOR_BATTERY | Changes::SNOW_STRENGTH));
        assert!(w.paused_for_battery());
        assert_eq!(w.snow_strength(), 1.0);
        let c = w.set_on_battery(false);
        assert!(c.contains(Changes::SHOWN_WEATHER) && !c.needs_save());
        let c = w.set_lightning(false);
        assert!(c.contains(Changes::LIGHTNING | Changes::LIGHTNING_SHOWN));
    }

    #[test]
    fn tray_variants() {
        let mut w = Weathers::default();
        w.tray_snow(false);
        assert_eq!((w.settings.weather, w.settings.heavy_snow), (Some(EffectKind::Snow), false));
        w.tray_snow(true);
        assert_eq!((w.settings.weather, w.settings.heavy_snow), (Some(EffectKind::Snow), true));
        w.tray_snow(true);
        assert_eq!((w.settings.weather, w.settings.heavy_snow), (None, false));
        w.tray_snow(false);
        w.tray_snow(false);
        assert_eq!(w.settings.weather, None);

        w.tray_rain(true);
        assert_eq!((w.settings.weather, w.settings.lightning), (Some(EffectKind::Rain), true));
        w.tray_rain(false);
        assert_eq!((w.settings.weather, w.settings.lightning), (Some(EffectKind::Rain), false));
        w.tray_rain(false);
        assert_eq!(w.settings.weather, None);

        w.toggle_weather(EffectKind::Fireflies);
        assert_eq!(w.settings.weather, Some(EffectKind::Fireflies));
        w.toggle_weather(EffectKind::Fireflies);
        assert_eq!(w.settings.weather, None);

        w.grid_variant(EffectKind::Rain, true);
        assert_eq!((w.settings.weather, w.settings.lightning), (Some(EffectKind::Rain), true));
        w.grid_variant(EffectKind::Rain, false);
        assert!(!w.settings.lightning);
        w.grid_variant(EffectKind::Snow, true);
        assert_eq!((w.settings.weather, w.settings.heavy_snow), (Some(EffectKind::Snow), true));
    }

    #[test]
    fn visitors_and_screens() {
        let mut w = Weathers::default();
        assert!(w.set_visitor(Visitor::Yeti, false).contains(Changes::VISITORS));
        w.set_visitor(Visitor::Santa, false);
        assert_eq!(w.settings.visitors_away, vec![Visitor::Santa, Visitor::Yeti]);
        assert!(w.set_visitor(Visitor::Santa, false).is_empty());
        w.set_visitor(Visitor::Santa, true);
        assert_eq!(w.settings.visitors_away, vec![Visitor::Yeti]);
        assert!(w.set_screen("d1", false).contains(Changes::SCREENS));
        assert!(w.set_screen("d1", false).is_empty());
        assert!(w.set_screen("d1", true).contains(Changes::SCREENS));
        assert!(w.set_main_screen(Some("d2".into())).contains(Changes::SCREENS));
    }
}
