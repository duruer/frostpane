use super::*;
use daynight::{Action, Slot};
use fp_core::clock::WallTime;
use fp_core::rng::Lcg64;
use openmeteo::Current;

#[test]
fn random_never_repeats_and_covers_all() {
    let mut r = Lcg64::new(3);
    let mut prev = None;
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..300 {
        let p = random_pick(prev, &mut r);
        assert_ne!(Some(p), prev);
        seen.insert(p.raw());
        prev = Some(p);
    }
    assert_eq!(seen.len(), 6);
    let first = random_pick(None, &mut Lcg64::new(9));
    assert!(EffectKind::ALL.contains(&first));
}

#[test]
fn season_tables() {
    use season::effect_for;
    let north: Vec<_> = (1..=12).map(|m| effect_for(m, false)).collect();
    assert_eq!(north, season::NORTH.to_vec());
    let south: Vec<_> = (1..=12).map(|m| effect_for(m, true)).collect();
    use EffectKind::*;
    assert_eq!(
        south,
        vec![Fireflies, Starfall, Leaves, Leaves, Leaves, Snow, Snow, Snow, Sakura, Sakura, Sakura, Rain]
    );
    assert_eq!(effect_for(13, false), Snow);
    assert_eq!(effect_for(0, false), Snow);
    assert_eq!(effect_for(18, true), Snow);
    // No overflow on absurd input; u32::MAX % 12 == 3 -> March.
    assert_eq!(effect_for(u32::MAX, false), Sakura);
    assert_eq!(effect_for(u32::MAX, true), Leaves);
}

#[test]
fn random_is_roughly_uniform() {
    let mut r = Lcg64::new(42);
    let idx = |e: EffectKind| EffectKind::ALL.iter().position(|x| *x == e).unwrap();
    let mut counts = [0u32; 6];
    for _ in 0..6000 {
        let p = random_pick(Some(EffectKind::Rain), &mut r);
        counts[idx(p)] += 1;
    }
    assert_eq!(counts[idx(EffectKind::Rain)], 0);
    for (i, c) in counts.iter().enumerate() {
        if i != idx(EffectKind::Rain) {
            assert!((1000..1400).contains(c), "{i}: {c}");
        }
    }
}

#[test]
fn hemisphere() {
    use season::is_south;
    assert!(is_south(Some(-33.9), "GMT Standard Time", None));
    assert!(!is_south(Some(48.0), "AUS Eastern Standard Time", Some(-30.0)));
    assert!(is_south(None, "AUS Eastern Standard Time", None));
    assert!(is_south(None, " new zealand standard time ", None));
    assert!(is_south(None, "Marquesas", None));
    assert!(!is_south(None, "W. Europe Standard Time", Some(-10.0)));
    assert!(is_south(None, "SE Asia Standard Time", Some(-6.0)));
    assert!(!is_south(None, "SE Asia Standard Time", Some(13.0)));
    assert!(!is_south(None, "Singapore Standard Time", None));
    assert!(is_south(None, "UTC+12", Some(-1.0)));
    assert!(is_south(None, "dateline standard time", Some(-1.0)));
    assert!(!is_south(None, "Line Islands Standard Time", Some(1.0)));
    assert!(is_south(Some(f64::NAN), "Tasmania Standard Time", None));
    assert!(!is_south(Some(f64::INFINITY), "GMT Standard Time", None));
    assert!(!is_south(None, "Hawaiian Standard Time", Some(f64::NAN)));
    assert!(!is_south(None, "UTC-11 Standard Time", Some(-5.0)));
}

fn cur(code: u32, day: bool, snow: f64) -> Current {
    Current { weather_code: code, is_day: day, temperature_c: 0.0, snowfall_cm: snow, wind_kmh: 0.0 }
}

#[test]
fn urls() {
    let u = openmeteo::forecast_url(52.52, -13.405);
    assert_eq!(
        u,
        "https://api.open-meteo.com/v1/forecast?latitude=52.5200&longitude=-13.4050&current=weather_code,is_day,temperature_2m,snowfall,wind_speed_10m"
    );
    assert_eq!(
        openmeteo::geocode_url(" Sao Paulo/é ", "en"),
        "https://geocoding-api.open-meteo.com/v1/search?name=Sao%20Paulo%2F%C3%A9&count=10&language=en&format=json"
    );
    assert!(openmeteo::geocode_url("a-b_c.d~e", "ja").contains("name=a-b_c.d~e&"));
}

#[test]
fn forecast_parsing() {
    let ok = r#"{"current":{"weather_code":75,"is_day":0,"temperature_2m":-3.5,"snowfall":1.2,"wind_speed_10m":9}}"#;
    let c = openmeteo::parse_forecast(ok).unwrap();
    assert_eq!(c, Current { weather_code: 75, is_day: false, temperature_c: -3.5, snowfall_cm: 1.2, wind_kmh: 9.0 });
    // 15-minute interval sum is scaled to cm/h.
    let q = openmeteo::parse_forecast(r#"{"current":{"weather_code":71,"interval":900,"snowfall":0.3}}"#).unwrap();
    assert!((q.snowfall_cm - 1.2).abs() < 1e-9);
    assert!(openmeteo::map_condition(&q, 1, false).heavy);
    let z = openmeteo::parse_forecast(r#"{"current":{"weather_code":71,"interval":0,"snowfall":0.3}}"#).unwrap();
    assert!((z.snowfall_cm - 0.3).abs() < 1e-9);
    let min = openmeteo::parse_forecast(r#"{"current":{"weather_code":0}}"#).unwrap();
    assert!(min.is_day);
    assert_eq!(min.temperature_c, 0.0);
    let b = openmeteo::parse_forecast(r#"{"current":{"weather_code":1,"is_day":false}}"#).unwrap();
    assert!(!b.is_day);
    let s = openmeteo::parse_forecast(r#"{"current":{"weather_code":1,"is_day":"x"}}"#).unwrap();
    assert!(s.is_day);
    let d = openmeteo::parse_forecast(r#"{"current":{"weather_code":1,"is_day":1}}"#).unwrap();
    assert!(d.is_day);
    for bad in [
        "nope",
        "{}",
        r#"{"current":{}}"#,
        r#"{"current":{"weather_code":"x"}}"#,
        r#"{"current":{"weather_code":-1}}"#,
        r#"{"current":{"weather_code":5000}}"#,
    ] {
        assert_eq!(openmeteo::parse_forecast(bad), None, "{bad}");
    }
}

#[test]
fn geocode_parsing() {
    let json = r#"{"results":[
        {"name":"Oslo","admin1":"Oslo","country":"Norway","latitude":59.9,"longitude":10.7},
        {"name":"","latitude":1,"longitude":2},
        {"name":"NoLat","longitude":2},
        {"name":"NoLon","latitude":2},
        {"name":"Bare","latitude":1.5,"longitude":2.5}
    ]}"#;
    let v = openmeteo::parse_geocode(json);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].country, "Norway");
    assert_eq!(v[0].admin, "Oslo");
    assert_eq!(v[1].name, "Bare");
    assert_eq!(v[1].admin, "");
    assert!(openmeteo::parse_geocode("{}").is_empty());
    assert!(openmeteo::parse_geocode("garbage").is_empty());
    assert!(openmeteo::parse_geocode(r#"{"results":5}"#).is_empty());
}

#[test]
fn mapping() {
    use EffectKind::*;
    let m = |c: &Current| openmeteo::map_condition(c, 7, false);
    for code in [71, 73, 77, 85] {
        let o = m(&cur(code, true, 0.2));
        assert_eq!((o.weather, o.heavy, o.lightning), (Some(Snow), false, false), "{code}");
    }
    for code in [75, 86] {
        assert!(m(&cur(code, true, 0.0)).heavy);
    }
    assert!(m(&cur(71, false, 1.0)).heavy);
    assert!(!m(&cur(71, false, 0.99)).heavy);
    for code in [51, 55, 61, 67, 80, 82] {
        let o = m(&cur(code, true, 5.0));
        assert_eq!((o.weather, o.heavy, o.lightning), (Some(Rain), false, false), "{code}");
    }
    for code in [95, 96, 99] {
        let o = m(&cur(code, false, 0.0));
        assert_eq!((o.weather, o.lightning), (Some(Rain), true), "{code}");
    }
    for code in 0..=3 {
        assert_eq!(m(&cur(code, false, 0.0)).weather, Some(Starfall));
        assert_eq!(m(&cur(code, true, 0.0)).weather, Some(Fireflies));
    }
    assert_eq!(m(&cur(45, false, 0.0)).weather, Some(Fireflies));
    assert_eq!(m(&cur(48, true, 0.0)).weather, Some(Fireflies));
    assert_eq!(m(&cur(200, true, 0.0)).weather, Some(Fireflies));
    assert_eq!(openmeteo::map_condition(&cur(45, true, 0.0), 1, true).weather, Some(Fireflies));
    assert_eq!(openmeteo::map_condition(&cur(45, true, 0.0), 1, false).weather, Some(Snow));
    assert!(!m(&cur(45, true, 0.0)).lightning);
}

#[test]
fn timer() {
    let mut t = ModeTimer::default();
    assert!(!t.running());
    assert!(!t.due(0.0));
    t.run(true, 100.0);
    assert!(t.running());
    assert!(t.due(100.0));
    assert!(!t.due(100.0 + MODE_PERIOD_S - 1.0));
    assert!(t.due(100.0 + MODE_PERIOD_S));
    t.run(true, 50_000.0);
    assert!(t.due(50_000.0));
    t.run(false, 0.0);
    assert!(!t.running());
    assert!(!t.due(1e9));
    // Settings hook: redundant `true` keeps the clock, a change restarts.
    let mut s = ModeTimer::default();
    s.sync(true, 0.0);
    assert!(s.due(0.0));
    s.sync(true, 5000.0);
    assert!(!s.due(5000.0));
    assert!(s.due(MODE_PERIOD_S));
    s.sync(false, 1.0);
    assert!(!s.running());
    s.sync(false, 2.0);
    assert!(!s.running());
    // Late tick re-arms from the tick time.
    let mut l = ModeTimer::default();
    l.run(true, 0.0);
    assert!(l.due(0.0));
    assert!(l.due(MODE_PERIOD_S + 500.0));
    assert!(!l.due(2.0 * MODE_PERIOD_S + 499.0));
    assert!(l.due(2.0 * MODE_PERIOD_S + 500.0));
}

#[test]
fn slots() {
    use daynight::slot_at;
    assert_eq!(slot_at(0), Slot::Night);
    assert_eq!(slot_at(359), Slot::Night);
    assert_eq!(slot_at(360), Slot::Dawn);
    assert_eq!(slot_at(419), Slot::Dawn);
    assert_eq!(slot_at(420), Slot::Sunrise);
    assert_eq!(slot_at(600), Slot::Morning);
    assert_eq!(slot_at(720), Slot::Midday);
    assert_eq!(slot_at(930), Slot::Golden);
    assert_eq!(slot_at(1020), Slot::Sunset);
    assert_eq!(slot_at(1100), Slot::Dusk);
    assert_eq!(slot_at(1170), Slot::Night);
    assert_eq!(slot_at(1439), Slot::Night);
    assert_eq!(slot_at(u32::MAX), Slot::Night);
    for s in Slot::ALL {
        assert_eq!(slot_at(s.start_minute()), s);
        assert_eq!(
            slot_at(s.start_minute() - 1),
            Slot::ALL.into_iter().rev().find(|p| p.start_minute() < s.start_minute()).unwrap_or(Slot::Night)
        );
    }
    let names: Vec<_> = Slot::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(names.join(","), "dawn,sunrise,morning,midday,golden,sunset,dusk,night");
    assert_eq!(daynight::file_name("winter", Slot::Dusk), "winter-dusk.jpg");
}

#[test]
fn schedule_steps() {
    let packs = vec!["winter".to_string(), "rain".to_string()];
    let step = |p: Option<&str>, shown: Option<&str>, os: Option<&str>, m| daynight::step(p, shown, os, m, &packs);
    assert_eq!(step(None, None, None, 600), Action::Nothing);
    assert_eq!(step(Some("gone"), None, None, 600), Action::Nothing);
    assert_eq!(step(Some("winter"), None, None, 600), Action::Show { file: "winter-morning.jpg".into() });
    assert_eq!(
        step(Some("winter"), Some("winter-dawn.jpg"), Some("C:\\x\\Winter-Dawn.JPG"), 600),
        Action::Show { file: "winter-morning.jpg".into() }
    );
    assert_eq!(
        step(Some("winter"), Some("D:/a/winter-morning.jpg"), Some("C:\\w\\winter-morning.jpg"), 600),
        Action::Nothing
    );
    assert_eq!(step(Some("winter"), Some("winter-morning.jpg"), None, 600), Action::Nothing);
    assert_eq!(step(Some("winter"), Some("winter-morning.jpg"), Some("other.jpg"), 600), Action::Stop);
}

#[test]
fn release_parsing() {
    let ok = r#"{"tag_name":"v1.2.3","html_url":"https://github.com/x/y/releases/tag/v1.2.3","draft":false}"#;
    let r = update::parse_latest(ok).unwrap();
    assert_eq!(r.tag, "v1.2.3");
    assert!(update::is_newer(&r, "1.2.2"));
    assert!(!update::is_newer(&r, "1.2.3"));
    for bad in [
        "x",
        "{}",
        r#"{"tag_name":"v1"}"#,
        r#"{"tag_name":" ","html_url":"https://a"}"#,
        r#"{"tag_name":"v1","html_url":"http://a"}"#,
        r#"{"tag_name":"v1","html_url":"https://a","draft":true}"#,
        r#"{"tag_name":"v1","html_url":"https://a","prerelease":true}"#,
        r#"{"tag_name":3,"html_url":"https://a"}"#,
    ] {
        assert_eq!(update::parse_latest(bad), None, "{bad}");
    }
}

#[test]
fn prompt_throttle() {
    let now = 1_800_000_000;
    let at = |ago: i64| WallTime::new(now - ago, 0).iso_utc();
    assert!(update::should_prompt(true, false, None, now));
    assert!(!update::should_prompt(false, false, None, now));
    assert!(update::should_prompt(false, true, None, now));
    assert!(update::should_prompt(false, true, Some("junk"), now));
    assert!(!update::should_prompt(false, true, Some(&at(3600)), now));
    assert!(update::should_prompt(false, true, Some(&at(update::ASK_EVERY_S)), now));
    assert!(!update::should_prompt(false, true, Some(&at(update::ASK_EVERY_S - 1)), now));
    assert!(!update::should_prompt(false, false, Some(&at(update::ASK_EVERY_S * 3)), now));
    assert!(!update::should_prompt(false, true, Some(&at(-3600)), now));
    assert!(!update::should_prompt(false, true, Some(&at(0)), i64::MIN));
}

struct Canned;
impl Fetch for Canned {
    fn get(&self, url: &str) -> Result<String, String> {
        if url.is_empty() { Err("empty".into()) } else { Ok(url.to_string()) }
    }
}

#[test]
fn fetch_trait_object() {
    let f: &dyn Fetch = &Canned;
    assert_eq!(f.get("u"), Ok("u".to_string()));
    assert!(f.get("").is_err());
}
