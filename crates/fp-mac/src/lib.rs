//! macOS backend (porting design 3.1). Skeleton: on macOS [`backend`]
//! returns the null backends with the data folder, language and UTC offset
//! filled in, so the app runs with its windows and tray but without overlay
//! or object windows. Empty on other targets.

#[cfg(target_os = "macos")]
pub use imp::backend;

#[cfg(target_os = "macos")]
mod imp {
    use fp_platform::null::{NullOverlay, NullPlatform, NullSprites};
    use fp_platform::{Limits, Parts};
    use std::path::PathBuf;
    use std::rc::Rc;

    /// Local time minus UTC in seconds, from `localtime_r`.
    fn utc_offset_s() -> i32 {
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

    pub fn backend() -> Parts<NullOverlay> {
        log::warn!("macOS backend not yet supported: running without overlay and object windows");
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let mut p = NullPlatform::new(home.join("Library/Application Support/Frostpane"));
        p.utc_offset = utc_offset_s;
        if let Some(code) = std::env::var("LANG").ok().map(|l| l.chars().take(2).collect::<String>())
            && code.len() == 2
            && code.chars().all(|c| c.is_ascii_alphabetic())
        {
            p.language = code.to_ascii_lowercase();
        }
        Parts {
            services: Rc::new(p),
            sprites: Box::new(NullSprites::default()),
            overlay: NullOverlay,
            limits: Limits::NULL,
        }
    }
}
