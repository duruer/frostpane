//! Linux-only parts: [`Services`], the session services as one object.

use crate::instance::{Acquired, SingleInstance};
use crate::logic::{self, Cmd, Desktop, dbus};
use crate::watcher::{Monitor, Snapshot};
use crate::{bus, logic::AutostartState};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_platform::{
    Driver, Guard, Instance, InstanceGuard, Platform, PowerStatus, Shell, SleepState, SystemEvent, SystemEvents,
    WallpaperScreen,
};
use fp_tracker::{HookEvent, WindowInfo};
use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Duration;

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

/// `org.kde.KWin` has an owner on the session bus.
pub fn kwin_on_bus() -> bool {
    zbus::blocking::Connection::session().is_ok_and(|c| bus::name_has_owner(&c, dbus::KWIN))
}

/// The session services (porting design 3.5). Use one per process, on the UI
/// thread (`Rc`, not `Sync`).
pub struct Services {
    data_dir: PathBuf,
    language: String,
    time_zone: String,
    desktop: Desktop,
    monitor: OnceCell<Monitor>,
    callback: Rc<RefCell<Option<Callback>>>,
    sleep: Cell<SleepState>,
    screens: RefCell<Vec<WallpaperScreen>>,
    last_wallpaper: RefCell<Option<PathBuf>>,
    wallpaper_ok: OnceCell<bool>,
}

type Callback = Box<dyn FnMut(SystemEvent, Option<bool>)>;

/// Dropping it stops event delivery to the `watch` callback.
struct WatchGuard(Rc<RefCell<Option<Callback>>>);

impl Drop for WatchGuard {
    fn drop(&mut self) {
        *self.0.borrow_mut() = None;
    }
}

/// The lock of a process that could not claim the lock file: never signalled.
struct NoGuard;

impl InstanceGuard for NoGuard {
    fn listen(&self, _: Box<dyn Fn() + Send>) {}
}

/// How long `power()` and friends wait for the first D-Bus reads.
const FIRST_READ: Duration = Duration::from_millis(500);

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn run(cmd: &Cmd) -> bool {
    Command::new(cmd.program)
        .args(&cmd.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn capture(cmd: &Cmd) -> Option<String> {
    let out = Command::new(cmd.program).args(&cmd.args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Starts `xdg-open arg` detached (reaped by a helper thread, no shell).
fn xdg_open(arg: &OsStr) {
    let child =
        Command::new("xdg-open").arg(arg).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    match child {
        Ok(mut c) => {
            let _ = std::thread::Builder::new().name("fp-xdg-open".into()).spawn(move || {
                let _ = c.wait();
            });
        }
        Err(e) => log::warn!("xdg-open: {e}"),
    }
}

fn has_tool(name: &str) -> bool {
    let path = env("PATH").unwrap_or_default();
    logic::find_in_path(name, &path, |p| p.is_file()).is_some()
}

impl Services {
    pub fn new() -> Self {
        let data = logic::data_dir(env("XDG_DATA_HOME").as_deref(), env("HOME").as_deref())
            .unwrap_or_else(|| std::env::temp_dir().join("frostpane"));
        let link = std::fs::read_link("/etc/localtime").ok();
        Self {
            data_dir: data,
            language: logic::language(env("LC_ALL").as_deref(), env("LC_MESSAGES").as_deref(), env("LANG").as_deref()),
            time_zone: logic::time_zone(env("TZ").as_deref(), link.as_deref()),
            desktop: logic::detect_desktop(
                env("XDG_CURRENT_DESKTOP").as_deref(),
                env("DESKTOP_SESSION").as_deref(),
                env("SWAYSOCK").is_some(),
                env("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
            ),
            monitor: OnceCell::new(),
            callback: Rc::new(RefCell::new(None)),
            sleep: Cell::new(SleepState::default()),
            screens: RefCell::new(Vec::new()),
            last_wallpaper: RefCell::new(None),
            wallpaper_ok: OnceCell::new(),
        }
    }

    /// The detected desktop environment.
    pub fn desktop(&self) -> Desktop {
        self.desktop
    }

    /// The display backend's monitors, for [`Shell::wallpaper_screens`] on
    /// desktops that cannot list them (everything except sway and Hyprland).
    pub fn set_screens(&self, screens: Vec<WallpaperScreen>) {
        *self.screens.borrow_mut() = screens;
    }

    /// Delivers queued system events to the `watch` callback. Call on the UI
    /// thread every tick; returns how many events were delivered.
    pub fn pump(&self) -> usize {
        let Some(m) = self.monitor.get() else { return 0 };
        let mut n = 0;
        while let Some(ev) = m.try_event() {
            let mut state = self.sleep.get();
            let asleep = state.apply(ev);
            self.sleep.set(state);
            if let Some(cb) = self.callback.borrow_mut().as_mut() {
                cb(ev, asleep);
                n += 1;
            }
        }
        n
    }

    fn monitor(&self) -> &Monitor {
        self.monitor.get_or_init(Monitor::start)
    }

    fn snapshot(&self) -> Snapshot {
        self.monitor().snapshot(FIRST_READ)
    }

    fn autostart_file(&self) -> Option<PathBuf> {
        logic::autostart_dir(env("XDG_CONFIG_HOME").as_deref(), env("HOME").as_deref())
            .map(|d| d.join("frostpane.desktop"))
    }

    fn autostart_state(&self) -> Option<AutostartState> {
        let text = std::fs::read_to_string(self.autostart_file()?).ok()?;
        Some(logic::parse_autostart(&text))
    }

    fn wallpaper_works(&self) -> bool {
        *self.wallpaper_ok.get_or_init(|| {
            let plasma = self.desktop == Desktop::Kde
                && zbus::blocking::Connection::session().is_ok_and(|c| bus::name_has_owner(&c, dbus::PLASMA));
            logic::wallpaper_supported(self.desktop, has_tool, plasma)
        })
    }

    /// Sets wallpapers; `items` are `(monitor id, file)`, `None` = every monitor.
    fn set_wallpapers(&self, items: &[(Option<&str>, &Path)]) -> bool {
        if !self.wallpaper_works() || items.is_empty() || items.iter().any(|(_, p)| !p.is_absolute()) {
            return false;
        }
        let ok = if self.desktop == Desktop::Kde {
            let all = items.iter().find(|(id, _)| id.is_none()).map(|(_, p)| *p);
            let each: Vec<(usize, PathBuf)> = items
                .iter()
                .filter_map(|(id, p)| Some((id.as_ref()?.parse::<usize>().ok()?, p.to_path_buf())))
                .collect();
            plasma_eval(&logic::plasma_script(all, &each))
        } else {
            let props = if self.desktop == Desktop::Xfce {
                let list = capture(&Cmd {
                    program: "xfconf-query",
                    args: vec!["-c".into(), "xfce4-desktop".into(), "-l".into()],
                });
                logic::xfce_image_props(&list.unwrap_or_default())
            } else {
                Vec::new()
            };
            let cmds = logic::wallpaper_cmds(self.desktop, items, &props, has_tool("feh"));
            // Some commands may fail on older setups (no dark key); one success counts.
            cmds.iter().map(run).collect::<Vec<_>>().contains(&true)
        };
        if ok {
            *self.last_wallpaper.borrow_mut() = Some(items[0].1.to_path_buf());
        }
        ok
    }
}

impl Default for Services {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs a script in the Plasma shell.
fn plasma_eval(script: &str) -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else { return false };
    let reply =
        conn.call_method(Some(dbus::PLASMA), dbus::PLASMA_PATH, Some(dbus::PLASMA_IFACE), "evaluateScript", &(script,));
    match reply {
        Ok(m) => m.body().deserialize::<String>().map_or(true, |s| logic::plasma_reply_ok(&s)),
        Err(e) => {
            log::warn!("plasma evaluateScript: {e}");
            false
        }
    }
}

/// The session services without D-Bus started yet (they start on first use).
pub fn services() -> Services {
    Services::new()
}

impl Platform for Services {
    fn monitors(&self) -> Vec<MonitorInfo> {
        Vec::new()
    }
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
    fn os_language(&self) -> String {
        self.language.clone()
    }
    fn power(&self) -> PowerStatus {
        let s = self.snapshot();
        PowerStatus { on_battery: s.on_battery, has_battery: s.has_battery, saving: s.on_battery || s.power_saver }
    }
    fn reduced_motion(&self) -> bool {
        self.snapshot().reduced_motion
    }
    fn cursor(&self) -> Option<(i32, i32)> {
        None
    }
    fn left_button_down(&self) -> bool {
        false
    }
    fn right_button_down(&self) -> bool {
        false
    }
    fn context_menu(&self, _: &str, _: (i32, i32)) -> bool {
        false
    }
    fn windows(&self) -> Vec<WindowInfo> {
        Vec::new()
    }
    fn taskbar_shelves(&self) -> Vec<PixelRect> {
        Vec::new()
    }
    fn time_zone_id(&self) -> String {
        self.time_zone.clone()
    }
    fn window_corner(&self) -> f64 {
        0.0
    }
    fn utc_offset_s(&self) -> i32 {
        utc_offset_s()
    }
    fn uptime_ms(&self) -> u64 {
        std::fs::read_to_string("/proc/uptime").ok().and_then(|t| logic::uptime_ms(&t)).unwrap_or(u64::MAX)
    }
    fn over_own(&self, _: (i32, i32)) -> bool {
        false
    }
    fn hooks(&self, _: Sender<HookEvent>) -> Option<Guard> {
        None
    }
    fn has_geometry(&self) -> bool {
        false
    }
    fn shelves_reader(&self) -> Box<dyn FnOnce() -> Vec<PixelRect> + Send> {
        Box::new(Vec::new)
    }
    fn single_instance(&self, quiet: bool) -> Instance {
        // SAFETY: `getuid` has no preconditions.
        let uid = unsafe { libc::getuid() };
        let (lock, sock) = logic::instance_paths(env("XDG_RUNTIME_DIR").as_deref(), uid);
        match SingleInstance::acquire_at(&lock, &sock, quiet) {
            Ok(Acquired::First(g)) => Instance::First(Box::new(g)),
            Ok(Acquired::Second) => Instance::Second,
            Err(e) => {
                log::warn!("single-instance lock unavailable: {e}");
                Instance::First(Box::new(NoGuard))
            }
        }
    }
}

impl Shell for Services {
    fn open_url(&self, url: &str) {
        if logic::url_allowed(url) {
            xdg_open(OsStr::new(url.trim()));
        }
    }
    fn login_is_on(&self) -> bool {
        self.autostart_state().is_some_and(|s| s.on)
    }
    fn login_blocked(&self) -> bool {
        self.autostart_state().is_some_and(|s| s.blocked)
    }
    fn login_set(&self, on: bool) -> bool {
        let Some(file) = self.autostart_file() else { return false };
        if !on {
            // A file someone disabled stays as it is.
            if !self.login_blocked() {
                let _ = std::fs::remove_file(&file);
            }
            return true;
        }
        if self.login_blocked() {
            return false;
        }
        let exe = env("APPIMAGE")
            .filter(|a| !a.is_empty())
            .or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()));
        let Some(exe) = exe else { return false };
        let write = || {
            std::fs::create_dir_all(file.parent()?).ok()?;
            std::fs::write(&file, logic::autostart_entry(&exe)).ok()
        };
        write().is_some()
    }
    fn show_startup_apps(&self) {
        if let Some(dir) = self.autostart_file().and_then(|f| f.parent().map(Path::to_path_buf)) {
            let _ = std::fs::create_dir_all(&dir);
            xdg_open(dir.as_os_str());
        }
    }
    fn taskbar_light(&self) -> bool {
        logic::scheme_is_light(self.snapshot().color_scheme)
    }
    fn tray_icon_promoted(&self) -> bool {
        true
    }
    fn tray_icon_spot(&self, _: PixelRect) -> Option<PixelRect> {
        None
    }
    fn region_latitude(&self) -> Option<f64> {
        None
    }
    fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
        let listed = match self.desktop {
            Desktop::Sway => {
                capture(&Cmd { program: "swaymsg", args: vec!["-t".into(), "get_outputs".into(), "-r".into()] })
                    .map(|j| logic::parse_sway_outputs(&j))
            }
            Desktop::Hyprland => capture(&Cmd { program: "hyprctl", args: vec!["monitors".into(), "-j".into()] })
                .map(|j| logic::parse_hypr_monitors(&j)),
            _ => None,
        };
        match listed.filter(|l| !l.is_empty()) {
            Some(l) => l.into_iter().map(|(id, width, height)| WallpaperScreen { id, width, height }).collect(),
            None => {
                let given = self.screens.borrow().clone();
                if given.is_empty() {
                    vec![WallpaperScreen { id: "0".into(), width: 1920, height: 1080 }]
                } else {
                    given
                }
            }
        }
    }
    fn wallpaper_set(&self, path: &Path) -> bool {
        self.set_wallpapers(&[(None, path)])
    }
    fn wallpaper_set_each(&self, items: &[(String, PathBuf)]) -> bool {
        let items: Vec<(Option<&str>, &Path)> = items.iter().map(|(id, p)| (Some(id.as_str()), p.as_path())).collect();
        self.set_wallpapers(&items)
    }
    fn wallpaper_current(&self) -> Option<PathBuf> {
        let read = match self.desktop {
            Desktop::Kde => {
                let home = env("HOME")?;
                let rc = Path::new(&home).join(".config/plasma-org.kde.plasma.desktop-appletsrc");
                std::fs::read_to_string(rc).ok().and_then(|t| logic::parse_plasma_image(&t))
            }
            d => {
                let xfce_prop = (d == Desktop::Xfce)
                    .then(|| {
                        let list = capture(&Cmd {
                            program: "xfconf-query",
                            args: vec!["-c".into(), "xfce4-desktop".into(), "-l".into()],
                        })?;
                        logic::xfce_image_props(&list).into_iter().next()
                    })
                    .flatten();
                let dark = self.snapshot().color_scheme == 1;
                logic::wallpaper_read_cmd(d, dark, xfce_prop.as_deref())
                    .and_then(|c| capture(&c))
                    .and_then(|out| logic::parse_wallpaper_read(d, &out))
            }
        };
        read.or_else(|| self.last_wallpaper.borrow().clone())
    }
    fn wallpaper_supported(&self) -> bool {
        self.wallpaper_works()
    }
    fn open_folder(&self, dir: &Path) {
        if dir.is_absolute() {
            xdg_open(dir.as_os_str());
        } else {
            xdg_open(Path::new(".").join(dir).as_os_str());
        }
    }
}

impl SystemEvents for Services {
    fn watch(&self, on: Box<dyn FnMut(SystemEvent, Option<bool>)>) -> Option<Guard> {
        self.monitor();
        *self.callback.borrow_mut() = Some(on);
        Some(Box::new(WatchGuard(self.callback.clone())))
    }
}

impl Driver for Services {
    /// No OS timer: the app's event loop calls the tick (and [`Services::pump`]).
    fn start(&self, _: u32, _: Box<dyn FnMut()>) -> Option<Guard> {
        None
    }
}
