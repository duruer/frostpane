//! Win32 implementation of the platform seam.

mod desk;
mod display;
mod host;
mod reg;
mod shell;

pub use desk::{
    cursor_or_zero, desktop_host, full_screens, is_above, is_topmost, key_down, order, point_in_own_process, root_at,
    taskbar_above, window_frames,
};
pub use host::{Driver, HookSet, SystemWatcher, pump_messages};
pub use shell::{
    build_number, region_latitude, small_icon_size, tray_icon_promoted, tray_icon_spot, wallpaper_current,
    wallpaper_screens, wallpaper_set, wallpaper_set_each,
};

use super::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::Time::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LBUTTON, VK_RBUTTON};
use windows::core::{PCWSTR, w};

/// Must run before any window is created.
pub fn set_dpi_awareness() {
    // SAFETY: plain Win32 call; fails harmlessly if already set.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub(crate) fn utc_offset_s() -> i32 {
    let mut tzi = TIME_ZONE_INFORMATION::default();
    // SAFETY: tzi is a valid out pointer.
    let id = unsafe { GetTimeZoneInformation(&mut tzi) };
    let bias = match id {
        2 => tzi.Bias + tzi.DaylightBias,
        1 => tzi.Bias + tzi.StandardBias,
        _ => tzi.Bias,
    };
    -bias * 60
}

/// Named-mutex single instance (core-app 2.2). Second launches signal the
/// first one's event and exit.
pub struct SingleInstance {
    mutex: HANDLE,
    event: HANDLE,
}

// SAFETY: kernel handles are usable from any thread.
unsafe impl Send for SingleInstance {}
unsafe impl Sync for SingleInstance {}

const MUTEX: PCWSTR = w!("Local\\FrostpaneRunning");
const EVENT: PCWSTR = w!("Local\\FrostpaneOpenWindow");

pub enum Instance {
    First(SingleInstance),
    /// Another instance runs; it was asked to open its window unless quiet.
    Second,
}

impl SingleInstance {
    pub fn acquire(quiet: bool) -> Instance {
        Self::acquire_named(MUTEX, EVENT, quiet)
    }

    fn acquire_named(mutex_name: PCWSTR, event_name: PCWSTR, quiet: bool) -> Instance {
        // SAFETY: Win32 handle APIs with valid arguments; handles closed on drop.
        unsafe {
            let Ok(mutex) = CreateMutexW(None, true, mutex_name) else { return Instance::Second };
            let first = GetLastError() != ERROR_ALREADY_EXISTS;
            let event = CreateEventW(None, false, false, event_name).unwrap_or_default();
            if first {
                Instance::First(SingleInstance { mutex, event })
            } else {
                if !quiet && !event.is_invalid() {
                    let _ = SetEvent(event);
                }
                let _ = CloseHandle(event);
                let _ = CloseHandle(mutex);
                Instance::Second
            }
        }
    }

    /// Spawns the "second launch" listener; `on_open` runs on that thread
    /// (marshal to the UI thread yourself). The thread ends with the process.
    pub fn listen(&self, on_open: impl Fn() + Send + 'static) {
        let ev = self.event.0 as isize;
        std::thread::Builder::new()
            .name("frostpane second launch".into())
            .spawn(move || {
                loop {
                    // SAFETY: the event handle lives as long as the process
                    // (SingleInstance is kept in main until exit).
                    let r = unsafe { WaitForSingleObject(HANDLE(ev as *mut _), INFINITE) };
                    if r != WAIT_OBJECT_0 {
                        break;
                    }
                    on_open();
                }
            })
            .ok();
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: we own the mutex handle; the event stays open for the listener thread.
        unsafe {
            let _ = ReleaseMutex(self.mutex);
            let _ = CloseHandle(self.mutex);
        }
    }
}

/// The Windows implementation of [`Platform`] and [`Shell`].
#[derive(Default)]
pub struct Win;

impl Platform for Win {
    fn monitors(&self) -> Vec<MonitorInfo> {
        display::monitors()
    }

    fn data_dir(&self) -> PathBuf {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Frostpane")
    }

    fn os_language(&self) -> String {
        shell::os_language()
    }

    fn power(&self) -> PowerStatus {
        shell::power()
    }

    fn reduced_motion(&self) -> bool {
        shell::reduced_motion()
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT::default();
        // SAFETY: valid out pointer.
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }

    fn left_button_down(&self) -> bool {
        key_down(VK_LBUTTON.0)
    }

    fn right_button_down(&self) -> bool {
        key_down(VK_RBUTTON.0)
    }

    fn context_menu(&self, label: &str, at: (i32, i32)) -> bool {
        shell::context_menu(label, at)
    }

    fn windows(&self) -> Vec<fp_tracker::WindowInfo> {
        desk::windows()
    }

    fn taskbar_shelves(&self) -> Vec<PixelRect> {
        desk::taskbar_shelves()
    }

    fn time_zone_id(&self) -> String {
        shell::time_zone_id()
    }

    fn window_corner(&self) -> f64 {
        shell::window_corner()
    }
}

impl Shell for Win {
    fn open_url(&self, url: &str) {
        shell::open_url(url);
    }
    fn login_is_on(&self) -> bool {
        shell::login_is_on()
    }
    fn login_blocked(&self) -> bool {
        shell::login_blocked()
    }
    fn login_set(&self, on: bool) -> bool {
        shell::login_set(on)
    }
    fn show_startup_apps(&self) {
        shell::open_url("ms-settings:startupapps");
    }
    fn taskbar_light(&self) -> bool {
        shell::taskbar_light()
    }
    fn tray_icon_promoted(&self) -> bool {
        std::env::current_exe().ok().and_then(|e| shell::tray_icon_promoted(&e)).unwrap_or(true)
    }
    fn tray_icon_spot(&self, rect: fp_core::geom::PixelRect) -> Option<fp_core::geom::PixelRect> {
        shell::tray_icon_spot_in_taskbar(rect)
    }
    fn region_latitude(&self) -> Option<f64> {
        shell::region_latitude()
    }
    fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
        shell::wallpaper_screens()
    }
    fn wallpaper_set(&self, path: &Path) -> bool {
        shell::wallpaper_set(path)
    }
    fn wallpaper_set_each(&self, items: &[(String, PathBuf)]) -> bool {
        shell::wallpaper_set_each(items)
    }
    fn wallpaper_current(&self) -> Option<PathBuf> {
        shell::wallpaper_current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_second_is_detected() {
        let m = w!("Local\\FrostpaneTestMutex");
        let e = w!("Local\\FrostpaneTestEvent");
        let first = SingleInstance::acquire_named(m, e, false);
        let Instance::First(f) = first else { panic!("expected first") };
        let (tx, rx) = std::sync::mpsc::channel();
        f.listen(move || {
            let _ = tx.send(());
        });
        assert!(matches!(SingleInstance::acquire_named(m, e, true), Instance::Second));
        assert!(rx.recv_timeout(std::time::Duration::from_millis(200)).is_err());
        // The mutex is already owned by this process, so a re-acquire is still "second".
        assert!(matches!(SingleInstance::acquire_named(m, e, false), Instance::Second));
        assert!(rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok());
    }

    #[test]
    fn platform_reads_work() {
        let w = Win;
        assert!(!w.os_language().is_empty());
        assert!(!w.time_zone_id().is_empty());
        let _ = w.power();
        let _ = w.reduced_motion();
        let _ = w.window_corner();
        let _ = w.taskbar_shelves();
        let _ = w.windows();
        let _ = w.taskbar_light();
        assert!(w.data_dir().ends_with("Frostpane"));
        assert!(small_icon_size() > 0);
    }

    #[test]
    fn driver_ticks_and_stops() {
        use std::cell::Cell;
        use std::rc::Rc;
        let n = Rc::new(Cell::new(0));
        let n2 = n.clone();
        let d = Driver::new(10, move || n2.set(n2.get() + 1)).expect("driver");
        let start = std::time::Instant::now();
        while n.get() < 3 && start.elapsed().as_secs() < 3 {
            pump_messages(20);
        }
        assert!(n.get() >= 3);
        drop(d);
        let at = n.get();
        pump_messages(50);
        assert_eq!(n.get(), at);
    }
}
