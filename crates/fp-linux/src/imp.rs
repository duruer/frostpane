//! Linux-only parts: environment reads and libc.

use crate::logic;
use fp_platform::null::NullPlatform;
use std::path::PathBuf;

/// Local time minus UTC in seconds, from `localtime_r`.
pub fn utc_offset_s() -> i32 {
    // SAFETY: `time(NULL)` has no preconditions; `localtime_r` writes into
    // the zeroed `tm` we own and returns null on failure.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i32
    }
}

/// `org.kde.KWin` is on the session bus. Not implemented yet (needs zbus).
pub fn kwin_on_bus() -> bool {
    false
}

/// The session services known without D-Bus.
pub fn services() -> NullPlatform {
    let var = |k: &str| std::env::var(k).ok();
    let data = logic::data_dir(var("XDG_DATA_HOME").as_deref(), var("HOME").as_deref())
        .unwrap_or_else(|| std::env::temp_dir().join("frostpane"));
    let mut p = NullPlatform::new(data);
    p.language = logic::language(var("LC_ALL").as_deref(), var("LC_MESSAGES").as_deref(), var("LANG").as_deref());
    let link: Option<PathBuf> = std::fs::read_link("/etc/localtime").ok();
    p.time_zone = logic::time_zone(var("TZ").as_deref(), link.as_deref());
    p.utc_offset = utc_offset_s;
    p
}
