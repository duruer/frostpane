//! A tiny stderr logger: `FROSTPANE_LOG=info|debug|trace|warn|error|off`
//! (default `warn`). The release build has no console, so this only matters
//! for developers and smoke runs.

use log::{LevelFilter, Log, Metadata, Record};

struct Stderr(LevelFilter);

impl Log for Stderr {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= self.0
    }

    fn log(&self, r: &Record<'_>) {
        if self.enabled(r.metadata()) {
            eprintln!("[{} {}] {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

/// Parse a level name; unknown names give `warn`.
pub fn level(name: Option<&str>) -> LevelFilter {
    match name.map(str::to_ascii_lowercase).as_deref() {
        Some("off") => LevelFilter::Off,
        Some("error") => LevelFilter::Error,
        Some("info") => LevelFilter::Info,
        Some("debug") => LevelFilter::Debug,
        Some("trace") => LevelFilter::Trace,
        _ => LevelFilter::Warn,
    }
}

/// Install the logger once; later calls are ignored.
pub fn init() {
    let filter = level(std::env::var("FROSTPANE_LOG").ok().as_deref());
    if log::set_boxed_logger(Box::new(Stderr(filter))).is_ok() {
        log::set_max_level(filter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level(None), LevelFilter::Warn);
        assert_eq!(level(Some("nonsense")), LevelFilter::Warn);
        assert_eq!(level(Some("OFF")), LevelFilter::Off);
        assert_eq!(level(Some("error")), LevelFilter::Error);
        assert_eq!(level(Some("info")), LevelFilter::Info);
        assert_eq!(level(Some("debug")), LevelFilter::Debug);
        assert_eq!(level(Some("trace")), LevelFilter::Trace);
    }

    #[test]
    fn logger_filters_and_prints() {
        let l = Stderr(LevelFilter::Warn);
        let warn = Metadata::builder().level(log::Level::Warn).build();
        let info = Metadata::builder().level(log::Level::Info).build();
        assert!(l.enabled(&warn));
        assert!(!l.enabled(&info));
        l.log(&Record::builder().args(format_args!("x")).level(log::Level::Warn).build());
        l.flush();
        init();
        init();
    }
}
