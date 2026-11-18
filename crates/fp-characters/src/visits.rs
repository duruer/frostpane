//! DailyVisits scheduler (social 2.13, characters 1.1).

use fp_core::clock::WallTime;
use fp_core::rng::Rng;

/// `often` divides every waiting time by this.
const OFTEN_SHARE: f64 = 6.0;

/// Visit scheduler: Santa 3600..10800 s, snowmen 7200..14400 s, `often`
/// divides by 6. `now` is the monotonic snow clock in seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct DailyVisits {
    pub low: f64,
    pub high: f64,
    next: f64,
    check_at: f64,
    queued_day: Option<(i32, u32, u32)>,
}

impl DailyVisits {
    /// A stopped scheduler with this waiting range.
    pub fn new(low: f64, high: f64) -> Self {
        Self { low, high, next: f64::INFINITY, check_at: 0.0, queued_day: None }
    }

    /// Santa: 3600..10800 s.
    pub fn santa() -> Self {
        Self::new(3600.0, 10800.0)
    }

    /// Snowman fights: 7200..14400 s.
    pub fn snowmen() -> Self {
        Self::new(7200.0, 14400.0)
    }

    fn share(often: bool) -> f64 {
        if often { OFTEN_SHARE } else { 1.0 }
    }

    pub fn start(&mut self, now: f64, often: bool, rng: &mut impl Rng) {
        self.next = now + rng.range(self.low, self.high) / Self::share(often);
        self.check_at = now;
        self.queued_day = None;
    }

    /// After a visit: wait again.
    pub fn wait_again(&mut self, now: f64, often: bool, rng: &mut impl Rng) {
        self.next = now + rng.range(self.low, self.high) / Self::share(often);
    }

    /// Stopped: never due until [`start`](Self::start).
    pub fn stop(&mut self) {
        self.next = f64::INFINITY;
    }

    /// Time of the next visit (infinite while stopped).
    pub fn next_at(&self) -> f64 {
        self.next
    }

    pub fn tick(&mut self, now: f64, wall: WallTime, last_visit_unix: Option<i64>, often: bool, rng: &mut impl Rng) {
        if self.next.is_infinite() || now < self.check_at {
            return;
        }
        self.check_at = now + 60.0;
        if often {
            self.next = self.next.min(now + self.high / OFTEN_SHARE);
        }
        let today = wall.local_date();
        let key = (today.year, today.month, today.day);
        let visited_today = last_visit_unix.is_some_and(|u| WallTime::new(u, wall.utc_offset_s).local_date() == today);
        if !visited_today && self.queued_day != Some(key) {
            self.queued_day = Some(key);
            self.next = self.next.min(now + rng.range(1500.0, 2700.0));
        }
    }

    pub fn due(&self, now: f64, comes: bool) -> bool {
        comes && now >= self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::rng::Lcg64;

    const DAY: i64 = 86_400;

    #[test]
    fn start_waits_inside_range() {
        let mut rng = Lcg64::new(3);
        let mut v = DailyVisits::santa();
        assert!(!v.due(1e9, true));
        v.start(100.0, false, &mut rng);
        assert!(v.next_at() >= 3700.0 && v.next_at() < 10900.0);
        v.start(100.0, true, &mut rng);
        assert!(v.next_at() >= 100.0 + 600.0 && v.next_at() < 100.0 + 1800.0);
        assert!(v.due(100.0 + 1800.0, true));
        assert!(!v.due(100.0 + 1800.0, false));
    }

    #[test]
    fn first_visit_of_day_is_pulled_in() {
        let mut rng = Lcg64::new(5);
        let mut v = DailyVisits::snowmen();
        v.start(0.0, false, &mut rng);
        assert!(v.next_at() >= 7200.0);
        let wall = WallTime::new(10 * DAY + 3600, 0);
        v.tick(0.0, wall, None, false, &mut rng);
        assert!(v.next_at() < 2700.001);
        // acts at most every 60 s
        let n = v.next_at();
        v.tick(30.0, wall, None, false, &mut rng);
        assert_eq!(v.next_at(), n);
        // already queued today: untouched even after 60 s
        v.next = 99_999.0;
        v.tick(61.0, wall, None, false, &mut rng);
        assert_eq!(v.next_at(), 99_999.0);
        // a new day queues again
        let tomorrow = WallTime::new(11 * DAY + 3600, 0);
        v.tick(200.0, tomorrow, None, false, &mut rng);
        assert!(v.next_at() < 200.0 + 2700.001);
    }

    #[test]
    fn visited_today_keeps_schedule_and_often_clamps() {
        let mut rng = Lcg64::new(9);
        let mut v = DailyVisits::santa();
        v.start(0.0, false, &mut rng);
        let wall = WallTime::new(5 * DAY + 100, 3600);
        v.next = 50_000.0;
        v.tick(0.0, wall, Some(5 * DAY + 50), false, &mut rng);
        assert_eq!(v.next_at(), 50_000.0);
        v.tick(60.0, wall, Some(5 * DAY + 50), true, &mut rng);
        assert_eq!(v.next_at(), 60.0 + 1800.0);
    }

    #[test]
    fn stopped_scheduler_ignores_ticks() {
        let mut rng = Lcg64::new(2);
        let mut v = DailyVisits::santa();
        v.start(0.0, false, &mut rng);
        v.stop();
        v.tick(10.0, WallTime::new(0, 0), None, true, &mut rng);
        assert!(v.next_at().is_infinite());
        v.start(10.0, false, &mut rng);
        v.wait_again(500.0, false, &mut rng);
        assert!(v.next_at() >= 4100.0);
    }
}
