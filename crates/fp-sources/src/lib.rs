//! Which effect is shown, decided without the OS.
//!
//! Spec: weather-sources 2.2-2.5 (manual rules live in `fp_core::settings`),
//! 2.3 replaced by Open-Meteo (README open question 3), 2.4 random pick,
//! 2.5 season table + hemisphere, 2.6 battery (in `fp_core::settings`),
//! 7 parameters; other-weather 2.6 DayAndNight slot schedule; core-app 2.11
//! update notice (GitHub releases, notice only); snowman-fight 2.1 /
//! social 2.13 are NOT here (DailyVisits lives in `fp-characters`).
//!
//! Network I/O is behind [`Fetch`]; `fp-net` implements it with ureq on a
//! worker thread. Everything here is pure and unit-testable.

use fp_core::effect::EffectKind;
use fp_core::rng::Rng;
use fp_core::settings::{City, Outside};

/// Blocking HTTP GET returning the body; implemented by `fp-net`.
pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<String, String>;
}

/// Period of every automatic mode timer (3 h).
pub const MODE_PERIOD_S: f64 = 10_800.0;

/// Random weather (2.4): uniform over all effects except `previous`.
pub fn random_pick(previous: Option<EffectKind>, rng: &mut impl Rng) -> EffectKind {
    let pool: Vec<EffectKind> = EffectKind::ALL.into_iter().filter(|e| Some(*e) != previous).collect();
    *rng.pick(&pool).unwrap_or(&EffectKind::Snow)
}

/// Season pick (2.5).
pub mod season {
    use super::EffectKind;

    /// Northern-hemisphere table, index = month - 1.
    pub const NORTH: [EffectKind; 12] = [
        EffectKind::Snow,
        EffectKind::Snow,
        EffectKind::Sakura,
        EffectKind::Sakura,
        EffectKind::Sakura,
        EffectKind::Rain,
        EffectKind::Fireflies,
        EffectKind::Starfall,
        EffectKind::Leaves,
        EffectKind::Leaves,
        EffectKind::Leaves,
        EffectKind::Snow,
    ];

    /// Windows zone ids (without the " Standard Time" suffix) in the south.
    const SOUTH: [&str; 29] = [
        "Easter Island",
        "Central Brazilian",
        "Pacific SA",
        "Tocantins",
        "Paraguay",
        "E. South America",
        "Argentina",
        "Montevideo",
        "Magallanes",
        "Bahia",
        "South Africa",
        "Namibia",
        "Mauritius",
        "W. Australia",
        "Aus Central W.",
        "Cen. Australia",
        "AUS Central",
        "E. Australia",
        "AUS Eastern",
        "Tasmania",
        "Lord Howe",
        "Bougainville",
        "Norfolk",
        "New Zealand",
        "Fiji",
        "Chatham Islands",
        "Tonga",
        "Samoa",
        "Marquesas",
    ];

    /// Zones straddling the equator (without the suffix).
    const SPLIT: [&str; 11] = [
        "SA Pacific",
        "SA Western",
        "SA Eastern",
        "E. Africa",
        "W. Central Africa",
        "Sao Tome",
        "SE Asia",
        "Singapore",
        "West Pacific",
        "Central Pacific",
        "Hawaiian",
    ];

    /// Split zones whose id is the whole name.
    const SPLIT_FULL: [&str; 6] =
        ["UTC-11", "UTC-09", "UTC+12", "UTC+13", "Line Islands Standard Time", "Dateline Standard Time"];

    /// Effect for a 1-based month; the south shifts six months. Months
    /// outside 1..=12 wrap.
    pub fn effect_for(month: u32, south: bool) -> EffectKind {
        let shift = if south { 5 } else { 11 };
        NORTH[((month % 12 + shift) % 12) as usize]
    }

    fn named(id: &str, list: &[&str]) -> bool {
        let lower = id.to_ascii_lowercase();
        let base = lower.strip_suffix(" standard time").unwrap_or(&lower);
        list.iter().any(|z| z.eq_ignore_ascii_case(base))
    }

    /// Southern hemisphere? Frostpane prefers the chosen city's latitude;
    /// without a city it uses the Windows time-zone id lists from 2.5.
    /// A non-finite city latitude counts as no city.
    pub fn is_south(city_latitude: Option<f64>, tz_id: &str, region_latitude: Option<f64>) -> bool {
        if let Some(lat) = city_latitude.filter(|l| l.is_finite()) {
            return lat < 0.0;
        }
        let id = tz_id.trim();
        if named(id, &SOUTH) {
            return true;
        }
        let split = named(id, &SPLIT) || SPLIT_FULL.iter().any(|z| z.eq_ignore_ascii_case(id));
        split && region_latitude.is_some_and(|l| l < 0.0)
    }
}

/// Open-Meteo forecast + geocoding (replaces the vendor `api/weather`).
pub mod openmeteo {
    use super::{City, EffectKind, Outside, season};
    use serde_json::Value;

    /// Current conditions we use.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Current {
        /// WMO weather interpretation code.
        pub weather_code: u32,
        pub is_day: bool,
        pub temperature_c: f64,
        /// Snowfall rate in cm per hour (the reply's interval sum scaled).
        pub snowfall_cm: f64,
        pub wind_kmh: f64,
    }

    /// Snowfall (cm/h) from which snow counts as heavy.
    pub const HEAVY_SNOWFALL_CM: f64 = 1.0;

    pub fn forecast_url(lat: f64, lon: f64) -> String {
        format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=weather_code,is_day,temperature_2m,snowfall,wind_speed_10m"
        )
    }

    fn num(v: &Value, key: &str) -> f64 {
        v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
    }

    /// Parse the forecast body; `None` when `current.weather_code` is absent
    /// or not a number in 0..=1000. Other fields default (day, 0).
    /// `current.snowfall` is the sum over `current.interval` seconds (15 min
    /// on Open-Meteo); it is scaled to cm/h. Without a positive interval the
    /// value is taken as hourly.
    pub fn parse_forecast(json: &str) -> Option<Current> {
        let root: Value = serde_json::from_str(json).ok()?;
        let cur = root.get("current")?;
        let code = cur.get("weather_code")?.as_f64()?;
        if !(0.0..=1000.0).contains(&code) {
            return None;
        }
        let is_day = match cur.get("is_day") {
            Some(Value::Bool(b)) => *b,
            Some(v) => v.as_f64().is_none_or(|n| n != 0.0),
            None => true,
        };
        Some(Current {
            weather_code: code as u32,
            is_day,
            temperature_c: num(cur, "temperature_2m"),
            snowfall_cm: num(cur, "snowfall") * hourly_factor(num(cur, "interval")),
            wind_kmh: num(cur, "wind_speed_10m"),
        })
    }

    fn hourly_factor(interval_s: f64) -> f64 {
        if interval_s > 0.0 { 3600.0 / interval_s } else { 1.0 }
    }

    /// Percent-encode everything except unreserved characters.
    fn encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    pub fn geocode_url(query: &str, lang: &str) -> String {
        format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=10&language={}&format=json",
            encode(query.trim()),
            encode(lang)
        )
    }

    fn text(v: &Value, key: &str) -> String {
        v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn city(v: &Value) -> Option<City> {
        let name = text(v, "name");
        let latitude = v.get("latitude")?.as_f64()?;
        let longitude = v.get("longitude")?.as_f64()?;
        if name.is_empty() {
            return None;
        }
        Some(City { name, admin: text(v, "admin1"), country: text(v, "country"), latitude, longitude })
    }

    /// Parse a geocoding reply; malformed input or no results give an empty list.
    pub fn parse_geocode(json: &str) -> Vec<City> {
        let Ok(root) = serde_json::from_str::<Value>(json) else {
            return Vec::new();
        };
        let Some(list) = root.get("results").and_then(Value::as_array) else {
            return Vec::new();
        };
        list.iter().filter_map(city).collect()
    }

    /// Frostpane's condition -> effect mapping (documented in ARCHITECTURE):
    /// snow codes -> snow (heavy for 75/86 or snowfall >= 1 cm/h); rain,
    /// drizzle, showers -> rain; thunderstorm 95-99 -> rain + lightning;
    /// clear/partly cloudy at night -> starfall, by day -> season pick;
    /// fog and anything else -> season pick.
    pub fn map_condition(c: &Current, month: u32, south: bool) -> Outside {
        let plain = |weather| Outside { weather: Some(weather), heavy: false, lightning: false };
        match c.weather_code {
            71..=77 | 85 | 86 => Outside {
                weather: Some(EffectKind::Snow),
                heavy: matches!(c.weather_code, 75 | 86) || c.snowfall_cm >= HEAVY_SNOWFALL_CM,
                lightning: false,
            },
            51..=67 | 80..=82 => plain(EffectKind::Rain),
            95..=99 => Outside { weather: Some(EffectKind::Rain), heavy: false, lightning: true },
            0..=3 if !c.is_day => plain(EffectKind::Starfall),
            _ => plain(season::effect_for(month, south)),
        }
    }
}

/// 3 h mode timers with immediate first pick and resume handling (2.3-2.5).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModeTimer {
    next: Option<f64>,
}

impl ModeTimer {
    /// Start (or restart) when the mode turns on, so the first `due` is
    /// immediate; `on == false` stops.
    pub fn run(&mut self, on: bool, now: f64) {
        self.next = if on { Some(now) } else { None };
    }

    /// True when a check/pick is due; re-arms for `MODE_PERIOD_S`.
    pub fn due(&mut self, now: f64) -> bool {
        match self.next {
            Some(t) if now >= t => {
                self.next = Some(now + MODE_PERIOD_S);
                true
            }
            _ => false,
        }
    }

    /// Settings-change hook (2.3-2.5): restart only when `on` differs from
    /// "timer running", so redundant sets keep the 3 h clock.
    pub fn sync(&mut self, on: bool, now: f64) {
        if on != self.running() {
            self.run(on, now);
        }
    }

    pub fn running(&self) -> bool {
        self.next.is_some()
    }
}

/// Day-and-night wallpaper schedule (other-weather 2.6).
pub mod daynight {
    /// Time-of-day slots with their start minute.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Slot {
        Dawn,
        Sunrise,
        Morning,
        Midday,
        Golden,
        Sunset,
        Dusk,
        Night,
    }

    impl Slot {
        pub const ALL: [Slot; 8] = [
            Slot::Dawn,
            Slot::Sunrise,
            Slot::Morning,
            Slot::Midday,
            Slot::Golden,
            Slot::Sunset,
            Slot::Dusk,
            Slot::Night,
        ];

        pub fn start_minute(self) -> u32 {
            match self {
                Slot::Dawn => 360,
                Slot::Sunrise => 420,
                Slot::Morning => 540,
                Slot::Midday => 720,
                Slot::Golden => 930,
                Slot::Sunset => 1020,
                Slot::Dusk => 1080,
                Slot::Night => 1170,
            }
        }

        /// Lowercase slot name used in wallpaper file names.
        pub fn name(self) -> &'static str {
            match self {
                Slot::Dawn => "dawn",
                Slot::Sunrise => "sunrise",
                Slot::Morning => "morning",
                Slot::Midday => "midday",
                Slot::Golden => "golden",
                Slot::Sunset => "sunset",
                Slot::Dusk => "dusk",
                Slot::Night => "night",
            }
        }
    }

    /// Slot for minutes since local midnight (before 06:00 -> night).
    pub fn slot_at(minutes: u32) -> Slot {
        Slot::ALL.into_iter().rev().find(|s| s.start_minute() <= minutes).unwrap_or(Slot::Night)
    }

    /// Still file name for a pack and slot: `<pack>-<slot>.jpg`.
    pub fn file_name(pack: &str, slot: Slot) -> String {
        format!("{pack}-{}.jpg", slot.name())
    }

    /// What the scheduler wants done this minute.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Action {
        Nothing,
        Show { file: String },
        Stop,
    }

    fn base(path: &str) -> &str {
        path.rsplit(['/', '\\']).next().unwrap_or(path)
    }

    /// One minute step: `enabled_pack`, last shown file name, current OS
    /// wallpaper file name (if known), available packs. File names are
    /// compared by name only, ignoring case.
    pub fn step(
        enabled_pack: Option<&str>,
        shown: Option<&str>,
        os_current: Option<&str>,
        minutes: u32,
        packs: &[String],
    ) -> Action {
        let Some(pack) = enabled_pack else {
            return Action::Nothing;
        };
        if !packs.iter().any(|p| p == pack) {
            return Action::Nothing;
        }
        if let (Some(shown), Some(os)) = (shown, os_current)
            && !base(shown).eq_ignore_ascii_case(base(os))
        {
            return Action::Stop;
        }
        let want = file_name(pack, slot_at(minutes));
        match shown {
            Some(s) if base(s).eq_ignore_ascii_case(&want) => Action::Nothing,
            _ => Action::Show { file: want },
        }
    }
}

/// GitHub "latest release" parsing for the update notice (never installs).
pub mod update {
    use serde_json::Value;

    /// Seconds between update prompts (24 h).
    pub const ASK_EVERY_S: i64 = 86_400;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Release {
        pub tag: String,
        pub url: String,
    }

    /// Parse `{tag_name, html_url}`. Both must be non-empty strings, the URL
    /// must be https, and drafts/prereleases are ignored.
    pub fn parse_latest(json: &str) -> Option<Release> {
        let root: Value = serde_json::from_str(json).ok()?;
        let flag = |k: &str| root.get(k).and_then(Value::as_bool).unwrap_or(false);
        if flag("draft") || flag("prerelease") {
            return None;
        }
        let tag = root.get("tag_name")?.as_str()?.trim();
        let url = root.get("html_url")?.as_str()?.trim();
        if tag.is_empty() || !url.starts_with("https://") {
            return None;
        }
        Some(Release { tag: tag.to_string(), url: url.to_string() })
    }

    /// True when the release is newer than `current` (`fp_core::version`).
    pub fn is_newer(release: &Release, current: &str) -> bool {
        fp_core::version::is_newer(&release.tag, current)
    }

    /// Show the prompt only if the user asked, or after onboarding when the
    /// last prompt (`updateAskedAt`, ISO UTC) is missing or >= 24 h old.
    pub fn should_prompt(asked: bool, seen_welcome: bool, asked_at: Option<&str>, now_unix: i64) -> bool {
        if asked {
            return true;
        }
        if !seen_welcome {
            return false;
        }
        match asked_at.and_then(fp_core::clock::WallTime::parse_iso_utc) {
            Some(t) => now_unix.saturating_sub(t) >= ASK_EVERY_S,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests;
