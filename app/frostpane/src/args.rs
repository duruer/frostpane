//! Command line (core-app 2.1 step 1): `--quiet`, `--seconds N`.

/// Parsed command line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Args {
    /// Start in the tray without the window (honoured once onboarding is done).
    pub quiet: bool,
    /// Exit by itself after this many seconds (smoke runs).
    pub seconds: Option<f64>,
}

/// Parse the arguments after the program name. Unknown flags are ignored; a
/// missing, negative or non-finite `--seconds` value counts as absent.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Args {
    let v: Vec<String> = args.into_iter().collect();
    let mut out = Args::default();
    let mut i = 0;
    while i < v.len() {
        let a = v[i].as_str();
        if a == "--quiet" {
            out.quiet = true;
        } else if a == "--seconds" {
            out.seconds = v.get(i + 1).and_then(|s| number(s));
            i += 1;
        } else if let Some(s) = a.strip_prefix("--seconds=") {
            out.seconds = number(s);
        }
        i += 1;
    }
    out
}

/// When a `--seconds` run must end, counted from `from`. Values too large
/// for the clock give no deadline instead of a panic.
pub fn deadline(seconds: Option<f64>, from: std::time::Instant) -> Option<std::time::Instant> {
    let d = std::time::Duration::try_from_secs_f64(seconds?).ok()?;
    from.checked_add(d)
}

fn number(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: &[&str]) -> Args {
        parse(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn flags() {
        assert_eq!(p(&[]), Args::default());
        assert_eq!(p(&["--quiet"]), Args { quiet: true, seconds: None });
        assert_eq!(p(&["--seconds", "2.5", "--quiet"]), Args { quiet: true, seconds: Some(2.5) });
        assert_eq!(p(&["--seconds=7"]).seconds, Some(7.0));
        assert!(p(&["--other", "--quiet"]).quiet);
    }

    #[test]
    fn bad_seconds_are_absent() {
        assert_eq!(p(&["--seconds"]).seconds, None);
        assert_eq!(p(&["--seconds", "x"]).seconds, None);
        assert_eq!(p(&["--seconds", "-3"]).seconds, None);
        assert_eq!(p(&["--seconds=inf"]).seconds, None);
        assert_eq!(p(&["--seconds", "NaN"]).seconds, None);
    }

    #[test]
    fn deadline_never_panics() {
        let t = std::time::Instant::now();
        assert_eq!(deadline(None, t), None);
        assert_eq!(deadline(Some(2.5), t), Some(t + std::time::Duration::from_millis(2500)));
        assert_eq!(deadline(Some(0.0), t), Some(t));
        // Parsed fine, but far beyond what the clock can hold.
        let huge = p(&["--seconds", "1e300"]).seconds;
        assert_eq!(huge, Some(1e300));
        assert_eq!(deadline(huge, t), None);
    }
}
