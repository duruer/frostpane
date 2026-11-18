//! Linux session services shared by `fp-x11` and `fp-wayland` (porting
//! design 3.5). Skeleton: the services are `fp_platform::null::NullPlatform`
//! with the facts that need no D-Bus filled in (data folder, language, time
//! zone, UTC offset). The parsers in [`logic`] are platform-free and tested
//! on every OS; the rest compiles only on Linux.

pub mod logic;

#[cfg(target_os = "linux")]
mod imp;
#[cfg(target_os = "linux")]
pub use imp::{kwin_on_bus, services, utc_offset_s};
