//! Frame timing helpers: dt clamping (desktop-native 3.8, snowballs 3.1) and
//! periodic gates.

/// Largest dt any simulation sees.
pub const MAX_DT: f64 = 0.05;
/// dt of the very first frame.
pub const FIRST_DT: f64 = 1.0 / 60.0;

/// Produces clamped frame deltas from monotonic timestamps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameTimer {
    prev: Option<f64>,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self::default()
    }

    /// dt for a frame at `now`: first frame 1/60, then `min(now - prev, 0.05)`,
    /// never negative.
    pub fn tick(&mut self, now: f64) -> f64 {
        let dt = match self.prev {
            Some(p) => (now - p).clamp(0.0, MAX_DT),
            None => FIRST_DT,
        };
        self.prev = Some(now);
        dt
    }

    pub fn reset(&mut self) {
        self.prev = None;
    }
}

/// "At most every N seconds" gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Every {
    pub period: f64,
    last: f64,
}

impl Every {
    /// The first [`Every::due`] call at or after `start + period` fires.
    pub fn new(period: f64, start: f64) -> Self {
        Self { period, last: start }
    }

    /// Fires immediately on the first call.
    pub fn immediate(period: f64) -> Self {
        Self { period, last: f64::NEG_INFINITY }
    }

    /// Returns true and re-arms when `now - last >= period`.
    pub fn due(&mut self, now: f64) -> bool {
        if now - self.last >= self.period {
            self.last = now;
            true
        } else {
            false
        }
    }

    /// Seconds since the last firing.
    pub fn since(&self, now: f64) -> f64 {
        now - self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_clamps() {
        let mut t = FrameTimer::new();
        assert_eq!(t.tick(10.0), FIRST_DT);
        assert!((t.tick(10.01) - 0.01).abs() < 1e-9);
        assert_eq!(t.tick(11.0), MAX_DT);
        assert_eq!(t.tick(10.0), 0.0);
        t.reset();
        assert_eq!(t.tick(0.0), FIRST_DT);
    }

    #[test]
    fn every() {
        let mut e = Every::new(1.0, 0.0);
        assert!(!e.due(0.5));
        assert!(e.due(1.0));
        assert!(!e.due(1.5));
        assert_eq!(e.since(1.5), 0.5);
        let mut i = Every::immediate(3.0);
        assert!(i.due(0.0));
        assert!(!i.due(2.9));
    }
}
