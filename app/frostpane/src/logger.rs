//! A tiny stderr logger: `FROSTPANE_LOG=info|debug|trace|warn|error|off`
//! (default `warn`). The release build has no console, so this only matters
//! for developers and smoke runs. `info` and `debug` apply to Frostpane's own
//! crates only; libraries stay at `warn` (the shader compiler alone writes
//! thousands of debug lines, slow enough to stall the render threads).
//! `trace` lets every crate through.

use log::{LevelFilter, Log, Metadata, Record};

struct Stderr(LevelFilter);

/// True for log targets of the app and its `fp-*` crates.
fn own(target: &str) -> bool {
    target == "frostpane" || target.starts_with("frostpane::") || target.starts_with("fp_")
}

/// The level a target is logged at under the filter `max`.
fn target_level(max: LevelFilter, target: &str) -> LevelFilter {
    if max == LevelFilter::Trace || own(target) { max } else { max.min(LevelFilter::Warn) }
}

impl Log for Stderr {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= target_level(self.0, m.target())
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

    #[test]
    fn debug_is_for_own_crates_only() {
        let l = Stderr(LevelFilter::Debug);
        let at = |target, level| Metadata::builder().target(target).level(level).build();
        assert!(l.enabled(&at("frostpane", log::Level::Debug)));
        assert!(l.enabled(&at("frostpane::balls", log::Level::Debug)));
        assert!(l.enabled(&at("fp_overlay::win", log::Level::Debug)));
        assert!(!l.enabled(&at("naga::front", log::Level::Debug)));
        assert!(!l.enabled(&at("wgpu_core", log::Level::Info)));
        assert!(l.enabled(&at("wgpu_core", log::Level::Warn)));
        assert!(!l.enabled(&at("frostpaneish", log::Level::Debug)));
        let all = Stderr(LevelFilter::Trace);
        assert!(all.enabled(&at("naga::front", log::Level::Trace)));
        let quiet = Stderr(LevelFilter::Error);
        assert!(!quiet.enabled(&at("wgpu_core", log::Level::Warn)));
    }
}
