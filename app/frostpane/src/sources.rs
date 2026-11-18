//! Timers and HTTP jobs: match-my-weather (3 h), random and season picks
//! (3 h), city search, and the release check (20 s after start, then 4 h).
//! The decisions live in `fp-sources`; the transport is the `fp-net` worker.
//! This module only keeps the clocks and routes replies.

use fp_core::settings::{City, Outside, Settings};
use fp_net::Worker;
use fp_sources::ModeTimer;
use fp_sources::openmeteo;
use fp_sources::update::{self, Release};
use std::collections::BTreeMap;

/// Where the latest release is read from (notice only, never downloaded).
pub const RELEASE_URL: &str = "https://api.github.com/repos/duruer/frostpane/releases/latest";
/// First release check after start (core-app 2.11).
pub const FIRST_CHECK_S: f64 = 20.0;
/// Period of the release check after the first one.
pub const CHECK_EVERY_S: f64 = 4.0 * 3600.0;

#[derive(Clone, Debug, PartialEq)]
enum Job {
    Forecast,
    Geocode(String),
    Update { asked: bool },
}

/// Result of one release check.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateOutcome {
    Newer(Release),
    Latest,
    Failed,
}

/// A finished job.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A forecast mapped to an effect (the host applies it only while the mode is on).
    Outside(Outside),
    /// The forecast request failed or could not be read: nothing changes.
    ForecastFailed,
    Cities {
        query: String,
        result: Result<Vec<City>, ()>,
    },
    Update {
        asked: bool,
        outcome: UpdateOutcome,
    },
}

/// Which timers fired.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Due {
    pub forecast: bool,
    pub random: bool,
    pub season: bool,
    pub update: bool,
}

pub struct Sources {
    worker: Worker,
    pending: BTreeMap<u64, Job>,
    next_id: u64,
    match_timer: ModeTimer,
    random_timer: ModeTimer,
    season_timer: ModeTimer,
    update_next: Option<f64>,
    current_version: String,
}

impl Sources {
    pub fn new(worker: Worker, now: f64, current_version: &str) -> Self {
        Self {
            worker,
            pending: BTreeMap::new(),
            next_id: 1,
            match_timer: ModeTimer::default(),
            random_timer: ModeTimer::default(),
            season_timer: ModeTimer::default(),
            update_next: Some(now + FIRST_CHECK_S),
            current_version: current_version.to_string(),
        }
    }

    /// Settings-change hook: start or stop a timer only when its mode flipped.
    pub fn sync(&mut self, s: &Settings, now: f64) {
        self.match_timer.sync(s.match_weather, now);
        self.random_timer.sync(s.random_weather, now);
        self.season_timer.sync(s.match_season, now);
    }

    /// Wake from sleep (weather-sources 2.3, 2.5): match and season restart
    /// and run at once. Random has no resume hook.
    pub fn resume(&mut self, s: &Settings, now: f64) {
        self.match_timer.run(s.match_weather, now);
        self.season_timer.run(s.match_season, now);
    }

    /// Check again right now (a new city was chosen).
    pub fn refresh_forecast(&mut self, s: &Settings, now: f64) {
        self.match_timer.run(s.match_weather, now);
    }

    /// Which timers fire at `now`.
    pub fn due(&mut self, now: f64) -> Due {
        let update = match self.update_next {
            Some(t) if now >= t => {
                self.update_next = Some(now + CHECK_EVERY_S);
                true
            }
            _ => false,
        };
        Due {
            forecast: self.match_timer.due(now),
            random: self.random_timer.due(now),
            season: self.season_timer.due(now),
            update,
        }
    }

    fn submit(&mut self, job: Job, url: String) -> bool {
        let id = self.next_id;
        self.next_id += 1;
        if self.worker.request(id, url) {
            self.pending.insert(id, job);
            true
        } else {
            false
        }
    }

    /// Ask Open-Meteo for the weather at `city`.
    pub fn request_forecast(&mut self, city: &City) -> bool {
        self.submit(Job::Forecast, openmeteo::forecast_url(city.latitude, city.longitude))
    }

    /// Search places by name.
    pub fn search(&mut self, query: &str, lang: &str) -> bool {
        self.submit(Job::Geocode(query.to_string()), openmeteo::geocode_url(query, lang))
    }

    /// True while a release check is running.
    pub fn working(&self) -> bool {
        self.pending.values().any(|j| matches!(j, Job::Update { .. }))
    }

    /// Start a release check. Ignored while one runs (core-app 2.11).
    pub fn check_update(&mut self, asked: bool) -> bool {
        if self.working() {
            return false;
        }
        self.submit(Job::Update { asked }, RELEASE_URL.to_string())
    }

    /// Finished jobs. `env` gives `(month, south)` for the season pick of a
    /// forecast; it is only called when a forecast arrived.
    pub fn poll(&mut self, env: impl Fn() -> (u32, bool)) -> Vec<Event> {
        let mut out = Vec::new();
        while let Some(reply) = self.worker.poll() {
            let Some(job) = self.pending.remove(&reply.id) else { continue };
            out.push(match job {
                Job::Forecast => match reply.body.ok().as_deref().and_then(openmeteo::parse_forecast) {
                    Some(cur) => {
                        let (month, south) = env();
                        let o = openmeteo::map_condition(&cur, month, south);
                        Event::Outside(Outside { weather: o.weather, heavy: o.heavy, lightning: o.lightning })
                    }
                    None => Event::ForecastFailed,
                },
                Job::Geocode(query) => {
                    let result = reply.body.map(|b| openmeteo::parse_geocode(&b)).map_err(|_| ());
                    Event::Cities { query, result }
                }
                Job::Update { asked } => {
                    let outcome = match reply.body.ok().as_deref().and_then(update::parse_latest) {
                        Some(r) if update::is_newer(&r, &self.current_version) => UpdateOutcome::Newer(r),
                        Some(_) => UpdateOutcome::Latest,
                        None => UpdateOutcome::Failed,
                    };
                    Event::Update { asked, outcome }
                }
            });
        }
        out
    }

    #[cfg(test)]
    fn wait_events(&mut self, n: usize, env: impl Fn() -> (u32, bool)) -> Vec<Event> {
        let mut all = Vec::new();
        for _ in 0..400 {
            all.extend(self.poll(&env));
            if all.len() >= n {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::effect::EffectKind;
    use fp_sources::Fetch;

    struct Fake;

    impl Fetch for Fake {
        fn get(&self, url: &str) -> Result<String, String> {
            if url.contains("geocoding-api") {
                if url.contains("name=Nowhere") {
                    Err("offline".into())
                } else {
                    Ok(r#"{"results":[{"name":"Oslo","admin1":"Oslo","country":"Norway","latitude":59.9,"longitude":10.7}]}"#
                        .to_string())
                }
            } else if url.contains("api.open-meteo.com") {
                Ok(r#"{"current":{"weather_code":75,"is_day":1,"temperature_2m":-3,"snowfall":0.1,"interval":900,"wind_speed_10m":4}}"#
                    .to_string())
            } else if url == RELEASE_URL {
                Ok(r#"{"tag_name":"v9.0.0","html_url":"https://example.org/r"}"#.to_string())
            } else {
                Err("unexpected".into())
            }
        }
    }

    fn sources() -> Sources {
        Sources::new(Worker::with_fetch(Fake, || {}), 0.0, "0.1.0")
    }

    fn city() -> City {
        City { name: "Oslo".into(), admin: String::new(), country: "Norway".into(), latitude: 59.9, longitude: 10.7 }
    }

    #[test]
    fn timers_follow_modes() {
        let mut s = sources();
        let mut set = Settings { match_weather: true, ..Settings::default() };
        s.sync(&set, 100.0);
        let d = s.due(100.0);
        assert!(d.forecast && !d.random && !d.season);
        // Redundant sync keeps the 3 h clock.
        s.sync(&set, 200.0);
        assert!(!s.due(200.0).forecast);
        assert!(s.due(100.0 + 10_800.0).forecast);
        set.match_weather = false;
        set.random_weather = true;
        set.match_season = true;
        s.sync(&set, 300.0);
        let d = s.due(300.0);
        assert!(!d.forecast && d.random && d.season);
        // Resume restarts match and season, not random.
        set.match_weather = true;
        s.resume(&set, 400.0);
        let d = s.due(400.0);
        assert!(d.forecast && d.season && !d.random);
        s.refresh_forecast(&set, 500.0);
        assert!(s.due(500.0).forecast);
    }

    #[test]
    fn update_timer_first_20s_then_4h() {
        let mut s = sources();
        assert!(!s.due(19.9).update);
        assert!(s.due(20.0).update);
        assert!(!s.due(20.0 + CHECK_EVERY_S - 1.0).update);
        assert!(s.due(20.0 + CHECK_EVERY_S).update);
    }

    #[test]
    fn forecast_maps_to_outside() {
        let mut s = sources();
        assert!(s.request_forecast(&city()));
        let ev = s.wait_events(1, || (1, false));
        assert_eq!(
            ev,
            vec![Event::Outside(Outside { weather: Some(EffectKind::Snow), heavy: true, lightning: false })]
        );
    }

    #[test]
    fn broken_forecast_changes_nothing() {
        let mut s = Sources::new(Worker::with_fetch(Broken, || {}), 0.0, "0.1.0");
        assert!(s.request_forecast(&city()));
        assert_eq!(s.wait_events(1, || (1, false)), vec![Event::ForecastFailed]);
        assert!(s.search("x", "en"));
        let ev = s.wait_events(1, || (1, false));
        assert_eq!(ev, vec![Event::Cities { query: "x".into(), result: Ok(Vec::new()) }]);
        assert!(s.check_update(true));
        assert!(s.working());
        let ev = s.wait_events(1, || (1, false));
        assert_eq!(ev, vec![Event::Update { asked: true, outcome: UpdateOutcome::Failed }]);
        assert!(!s.working());
    }

    struct Broken;

    impl Fetch for Broken {
        fn get(&self, _: &str) -> Result<String, String> {
            Ok("not json".into())
        }
    }

    #[test]
    fn city_search_results_and_errors() {
        let mut s = sources();
        s.search("Oslo", "en");
        s.search("Nowhere", "en");
        let ev = s.wait_events(2, || (1, false));
        match &ev[0] {
            Event::Cities { query, result } => {
                assert_eq!(query, "Oslo");
                assert_eq!(result.as_ref().unwrap()[0].name, "Oslo");
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(ev[1], Event::Cities { query: "Nowhere".into(), result: Err(()) });
    }

    #[test]
    fn update_check_newer_and_single_flight() {
        let mut s = sources();
        assert!(s.check_update(false));
        assert!(!s.check_update(true), "ignored while running");
        let ev = s.wait_events(1, || (1, false));
        match &ev[0] {
            Event::Update { asked: false, outcome: UpdateOutcome::Newer(r) } => assert_eq!(r.tag, "v9.0.0"),
            e => panic!("{e:?}"),
        }
        let mut same = Sources::new(Worker::with_fetch(Fake, || {}), 0.0, "9.0.0");
        same.check_update(true);
        assert_eq!(
            same.wait_events(1, || (1, false)),
            vec![Event::Update { asked: true, outcome: UpdateOutcome::Latest }]
        );
    }
}
