//! Animation maths (ui 2.5, 3.7, 6.1): quintic ease-out and the toast timeline.

/// `1 - (1 - t)^5`, `t` clamped to 0..=1.
pub fn ease_out_quint(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(5)
}

/// Linear interpolation.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// A value moving from `from` to `to` over `dur` seconds from `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    pub from: f64,
    pub to: f64,
    pub start: f64,
    pub dur: f64,
}

impl Tween {
    pub fn new(from: f64, to: f64, start: f64, dur: f64) -> Self {
        Self { from, to, start, dur }
    }

    /// Eased value at `now`.
    pub fn at(&self, now: f64) -> f64 {
        if self.dur <= 0.0 {
            return self.to;
        }
        lerp(self.from, self.to, ease_out_quint((now - self.start) / self.dur))
    }

    pub fn done(&self, now: f64) -> bool {
        now - self.start >= self.dur
    }
}

/// Seconds the toast stays after the last `show` (ui 3.7).
pub const TOAST_HOLD: f64 = 2.0;
/// Slide and fade time of the toast.
pub const TOAST_FADE: f64 = 0.35;
/// Hidden toast offset (points below its resting place).
pub const TOAST_OFFSET: f64 = 48.0;

/// One toast at a time: a new message replaces the old one and restarts the
/// 2 s hold; the slide reverses smoothly from wherever it is.
#[derive(Clone, Debug, Default)]
pub struct Toast {
    text: String,
    shown_at: Option<f64>,
    /// (progress at the last change, time of the change); progress 0 = hidden.
    from: (f64, f64),
}

impl Toast {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&mut self, text: &str, now: f64) {
        let p = self.progress(now);
        self.text = text.to_string();
        self.from = (p, now);
        self.shown_at = Some(now);
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// 0 = hidden, 1 = fully shown.
    pub fn progress(&self, now: f64) -> f64 {
        let Some(at) = self.shown_at else { return 0.0 };
        let (p0, t0) = self.from;
        if now < at + TOAST_HOLD {
            lerp(p0, 1.0, ease_out_quint((now - t0) / TOAST_FADE))
        } else {
            // The hide starts at `at + HOLD` from whatever progress was reached.
            let reached = lerp(p0, 1.0, ease_out_quint((at + TOAST_HOLD - t0) / TOAST_FADE));
            lerp(reached, 0.0, ease_out_quint((now - at - TOAST_HOLD) / TOAST_FADE))
        }
    }

    /// Opacity and downward offset in points at `now`.
    pub fn look(&self, now: f64) -> (f64, f64) {
        let p = self.progress(now);
        (p, (1.0 - p) * TOAST_OFFSET)
    }

    pub fn visible(&self, now: f64) -> bool {
        self.progress(now) > 0.001
    }

    /// True while a frame must be drawn (animation or hold running).
    pub fn active(&self, now: f64) -> bool {
        self.shown_at.is_some_and(|at| now < at + TOAST_HOLD + TOAST_FADE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quint_curve() {
        assert_eq!(ease_out_quint(0.0), 0.0);
        assert_eq!(ease_out_quint(1.0), 1.0);
        assert!((ease_out_quint(0.5) - (1.0 - 0.5f64.powi(5))).abs() < 1e-12);
        assert_eq!(ease_out_quint(-3.0), 0.0);
        assert_eq!(ease_out_quint(9.0), 1.0);
    }

    #[test]
    fn tween_runs_and_finishes() {
        let t = Tween::new(10.0, 20.0, 1.0, 0.28);
        assert_eq!(t.at(1.0), 10.0);
        assert_eq!(t.at(5.0), 20.0);
        assert!(t.done(1.28) && !t.done(1.1));
        assert_eq!(Tween::new(1.0, 2.0, 0.0, 0.0).at(0.0), 2.0);
    }

    #[test]
    fn toast_timeline() {
        let mut t = Toast::new();
        assert!(!t.visible(0.0) && !t.active(0.0));
        assert_eq!(t.look(0.0), (0.0, 48.0));
        t.show("hi", 10.0);
        assert_eq!(t.text(), "hi");
        assert_eq!(t.progress(10.0), 0.0);
        assert!((t.progress(10.5) - 1.0).abs() < 1e-3);
        assert!(t.visible(11.9));
        assert!(t.progress(12.4) < 1e-3);
        assert!(!t.visible(12.4));
        assert!(t.active(12.2) && !t.active(12.4));
        // A new show during the hide reverses from the current progress.
        t.show("again", 12.1);
        let p = t.progress(12.1);
        assert!(p > 0.0 && p < 1.0);
        assert!(t.progress(12.5) > p);
        // The hold restarts: still visible 1.9 s after the last show.
        assert!(t.visible(12.1 + 1.9));
    }
}
