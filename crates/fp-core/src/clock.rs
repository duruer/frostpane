//! Injected time. Monotonic seconds for simulation, wall time for daily
//! schedules (Santa visits, gifts, seasons, day and night).

use std::cell::Cell;

/// A wall-clock instant: UTC unix seconds plus the local UTC offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WallTime {
    pub unix: i64,
    pub utc_offset_s: i32,
}

impl WallTime {
    pub fn new(unix: i64, utc_offset_s: i32) -> Self {
        Self { unix, utc_offset_s }
    }

    fn local_secs(&self) -> i64 {
        self.unix + self.utc_offset_s as i64
    }

    pub fn local_date(&self) -> CivilDate {
        CivilDate::from_days(self.local_secs().div_euclid(86_400))
    }

    pub fn utc_date(&self) -> CivilDate {
        CivilDate::from_days(self.unix.div_euclid(86_400))
    }

    /// Minutes since local midnight, `0..1440`.
    pub fn local_minutes(&self) -> u32 {
        (self.local_secs().rem_euclid(86_400) / 60) as u32
    }

    /// ISO-8601 UTC, e.g. `2026-11-17T09:30:12Z`.
    pub fn iso_utc(&self) -> String {
        let d = self.utc_date();
        let s = self.unix.rem_euclid(86_400);
        format!("{}T{:02}:{:02}:{:02}Z", d.iso(), s / 3600, s / 60 % 60, s % 60)
    }

    /// Parse `YYYY-MM-DDTHH:MM:SS[.fff]Z` (fraction ignored).
    pub fn parse_iso_utc(s: &str) -> Option<i64> {
        let (date, rest) = s.split_once('T')?;
        let d = CivilDate::parse(date)?;
        let rest = rest.strip_suffix('Z')?;
        let rest = rest.split('.').next()?;
        let mut it = rest.split(':').map(|p| p.parse::<i64>().ok());
        let (h, m, sec) = (it.next()??, it.next()??, it.next()??);
        if it.next().is_some() || h > 23 || m > 59 || sec > 60 {
            return None;
        }
        Some(d.to_days() * 86_400 + h * 3600 + m * 60 + sec)
    }
}

/// Proleptic Gregorian date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl CivilDate {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    /// Days since 1970-01-01 (Howard Hinnant's algorithm).
    pub fn to_days(&self) -> i64 {
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
        Self { year, month, day }
    }

    /// `yyyy-MM-dd`.
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.split('-');
        let y = it.next()?.parse().ok()?;
        let m = it.next()?.parse().ok()?;
        let d = it.next()?.parse().ok()?;
        if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        Some(Self::new(y, m, d))
    }
}

/// Time source injected into schedulers and controllers.
pub trait Clock {
    /// Monotonic seconds since an arbitrary origin.
    fn now(&self) -> f64;
    /// Current wall time.
    fn wall(&self) -> WallTime;
}

/// Test clock; advance it by hand.
#[derive(Debug, Default)]
pub struct ManualClock {
    now: Cell<f64>,
    wall: Cell<WallTime>,
}

impl ManualClock {
    pub fn new(now: f64, wall: WallTime) -> Self {
        Self { now: Cell::new(now), wall: Cell::new(wall) }
    }

    /// Advances both clocks by `dt` seconds (wall time in whole seconds).
    pub fn advance(&self, dt: f64) {
        let before = self.now.get();
        self.now.set(before + dt);
        let w = self.wall.get();
        let whole = (before + dt).floor() as i64 - before.floor() as i64;
        self.wall.set(WallTime::new(w.unix + whole, w.utc_offset_s));
    }

    pub fn set_wall(&self, w: WallTime) {
        self.wall.set(w);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> f64 {
        self.now.get()
    }
    fn wall(&self) -> WallTime {
        self.wall.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for days in [-800_000i64, -1, 0, 59, 60, 10_957, 20_774, 2_000_000] {
            assert_eq!(CivilDate::from_days(days).to_days(), days);
        }
        assert_eq!(CivilDate::from_days(0), CivilDate::new(1970, 1, 1));
        assert_eq!(CivilDate::new(2026, 11, 17).iso(), "2026-11-17");
        assert_eq!(CivilDate::parse("2026-11-17"), Some(CivilDate::new(2026, 11, 17)));
        assert_eq!(CivilDate::parse("2026-13-01"), None);
        assert_eq!(CivilDate::parse("2026-01-00"), None);
        assert_eq!(CivilDate::parse("2026-01-01-1"), None);
        assert_eq!(CivilDate::parse("2026-01"), None);
        assert_eq!(CivilDate::parse("x-01-01"), None);
    }

    #[test]
    fn wall_time() {
        // 2026-11-17 23:30:00 UTC, local offset +3h -> 2026-11-18 02:30
        let unix = CivilDate::new(2026, 11, 17).to_days() * 86_400 + 23 * 3600 + 30 * 60;
        let w = WallTime::new(unix, 3 * 3600);
        assert_eq!(w.local_date(), CivilDate::new(2026, 11, 18));
        assert_eq!(w.utc_date(), CivilDate::new(2026, 11, 17));
        assert_eq!(w.local_minutes(), 150);
        let iso = w.iso_utc();
        assert_eq!(iso, "2026-11-17T23:30:00Z");
        assert_eq!(WallTime::parse_iso_utc(&iso), Some(unix));
        assert_eq!(WallTime::parse_iso_utc("2026-11-17T23:30:00.1234567Z"), Some(unix));
        assert_eq!(WallTime::parse_iso_utc("2026-11-17T23:30:00"), None);
        assert_eq!(WallTime::parse_iso_utc("2026-11-17"), None);
        assert_eq!(WallTime::parse_iso_utc("2026-11-17T24:00:00Z"), None);
        assert_eq!(WallTime::parse_iso_utc("2026-11-17T23:30:00:00Z"), None);
        assert_eq!(WallTime::parse_iso_utc("2026-11-17T23:30Z"), None);
        assert_eq!(WallTime::parse_iso_utc("2026-11-17Tab:30:00Z"), None);
        assert_eq!(WallTime::parse_iso_utc("bad-11-17T23:30:00Z"), None);
    }

    #[test]
    fn manual_clock() {
        let c = ManualClock::new(0.5, WallTime::new(100, 0));
        c.advance(0.4);
        assert_eq!(c.wall().unix, 100);
        c.advance(0.2);
        assert_eq!(c.wall().unix, 101);
        assert!((c.now() - 1.1).abs() < 1e-12);
        c.set_wall(WallTime::new(5, 60));
        assert_eq!(c.wall(), WallTime::new(5, 60));
        assert_eq!(ManualClock::default().now(), 0.0);
    }
}
