//! Crash marker (core-app 2.13): `running.json` while the app runs, a local
//! `crash.txt` written by the panic hook. Nothing is sent anywhere.

use fp_core::marker::RunMarker;
use std::path::{Path, PathBuf};

const CRASH_LIMIT: usize = 20_000;

/// What the previous run left behind.
#[derive(Debug, Default, PartialEq)]
pub struct Previous {
    /// A stale `running.json`: the previous run ended without a clean exit.
    pub marker: Option<RunMarker>,
    /// Text of `crash.txt`, if any (the file is deleted).
    pub crash: Option<String>,
}

fn marker_path(dir: &Path) -> PathBuf {
    dir.join("running.json")
}

fn crash_path(dir: &Path) -> PathBuf {
    dir.join("crash.txt")
}

/// Start (first instance only): read and delete the old crash text, read
/// the old marker, write the new marker. Failures skip the rest silently.
pub fn start(dir: &Path, session: &str, launched: i64, version: &str) -> Previous {
    let mut prev = Previous::default();
    if std::fs::create_dir_all(dir).is_err() {
        return prev;
    }
    prev.crash = std::fs::read_to_string(crash_path(dir)).ok();
    let _ = std::fs::remove_file(crash_path(dir));
    prev.marker = std::fs::read_to_string(marker_path(dir)).ok().and_then(|s| RunMarker::from_json(&s));
    let now = RunMarker { session: session.to_string(), launched, version: version.to_string() };
    let _ = std::fs::write(marker_path(dir), now.to_json());
    prev
}

/// Clean exit: delete the marker.
pub fn finish(dir: &Path) {
    let _ = std::fs::remove_file(marker_path(dir));
}

/// Write `"<place>: <text>"`, cut to 20000 characters.
pub fn write_crash(dir: &Path, place: &str, text: &str) {
    let full = format!("{place}: {text}");
    let cut: String = full.chars().take(CRASH_LIMIT).collect();
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(crash_path(dir), cut);
}

/// Panic hook: write the panic with a backtrace into `crash.txt`, then run
/// the previous hook. The marker stays, so the next start sees the unclean exit.
pub fn install_panic_hook(dir: PathBuf) {
    let old = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info.location().map_or_else(|| "panic".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let trace = std::backtrace::Backtrace::force_capture();
        write_crash(&dir, &place, &format!("{info}\n{trace}"));
        old(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn marker_lifecycle() {
        let d = temp("marker");
        let first = start(&d, "S1", 100, "0.1.0");
        assert_eq!(first, Previous::default());
        // A second start without finish sees the stale marker.
        let second = start(&d, "S2", 200, "0.1.0");
        assert_eq!(second.marker.as_ref().map(|m| m.session.as_str()), Some("S1"));
        assert_eq!(second.marker.map(|m| m.launched), Some(100));
        finish(&d);
        assert!(!marker_path(&d).exists());
        assert_eq!(start(&d, "S3", 300, "0.1.0").marker, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn crash_text_is_read_once_and_cut() {
        let d = temp("crash");
        write_crash(&d, "main.rs:1", &"x".repeat(30_000));
        let prev = start(&d, "S", 1, "0.1.0");
        let text = prev.crash.expect("crash text");
        assert_eq!(text.chars().count(), CRASH_LIMIT);
        assert!(text.starts_with("main.rs:1: x"));
        assert_eq!(start(&d, "T", 2, "0.1.0").crash, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unwritable_dir_is_silent() {
        let d = temp("file");
        std::fs::write(&d, b"not a dir").unwrap();
        assert_eq!(start(&d.join("sub"), "S", 1, "v"), Previous::default());
        write_crash(&d.join("sub"), "p", "t");
        finish(&d.join("sub"));
        let _ = std::fs::remove_file(&d);
    }
}
