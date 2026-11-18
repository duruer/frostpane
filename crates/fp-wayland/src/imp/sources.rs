//! Foreign window geometry (design 3.3): sway IPC, Hyprland IPC and a KWin
//! script reporting over D-Bus. Listings are synchronous IPC requests (sway,
//! Hyprland) or the script's latest report (KWin); an event thread per
//! source nudges the tracker (`HookEvent::Foreground`) when windows or
//! workspaces change, so it polls fast while something happens.

use crate::logic::{self, Snapshot};
use fp_tracker::HookEvent;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Where event threads send their nudges (set by `Platform::hooks`).
pub type Wake = Arc<Mutex<Option<Sender<HookEvent>>>>;

fn nudge(wake: &Wake) {
    if let Some(tx) = wake.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        let _ = tx.send(HookEvent::Foreground);
    }
}

/// One window source.
pub trait Source {
    /// The current windows (top first) and panel space; `None` on failure.
    fn list(&mut self) -> Option<Snapshot>;
}

const IO_TIMEOUT: Duration = Duration::from_millis(500);

fn connect(path: &Path) -> Option<UnixStream> {
    let s = UnixStream::connect(path).ok()?;
    let _ = s.set_read_timeout(Some(IO_TIMEOUT));
    let _ = s.set_write_timeout(Some(IO_TIMEOUT));
    Some(s)
}

// ---- sway ----------------------------------------------------------------

fn sway_read(s: &mut UnixStream) -> Option<(u32, Vec<u8>)> {
    let mut h = [0u8; 14];
    s.read_exact(&mut h).ok()?;
    let (len, kind) = logic::ipc_header(&h)?;
    let mut body = vec![0u8; len as usize];
    s.read_exact(&mut body).ok()?;
    Some((kind, body))
}

struct Sway {
    path: PathBuf,
    stream: Option<UnixStream>,
}

impl Source for Sway {
    fn list(&mut self) -> Option<Snapshot> {
        if self.stream.is_none() {
            self.stream = connect(&self.path);
        }
        let s = self.stream.as_mut()?;
        let reply = s.write_all(&logic::ipc_encode(logic::SWAY_GET_TREE, b"")).ok().and_then(|_| sway_read(s));
        let Some((_, body)) = reply else {
            self.stream = None;
            return None;
        };
        let v: serde_json::Value = serde_json::from_slice(&body).ok()?;
        Some(logic::sway_tree(&v))
    }
}

fn sway_events(path: PathBuf, wake: Wake) {
    loop {
        if let Ok(mut s) = UnixStream::connect(&path) {
            let sub = logic::ipc_encode(logic::SWAY_SUBSCRIBE, br#"["window","workspace","output"]"#);
            if s.write_all(&sub).is_ok() {
                while let Some((kind, _)) = sway_read(&mut s) {
                    if logic::sway_event_relevant(kind) {
                        nudge(&wake);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

// ---- Hyprland ------------------------------------------------------------

struct Hypr {
    dir: PathBuf,
}

impl Hypr {
    fn request(&self, cmd: &str) -> Option<serde_json::Value> {
        let mut s = connect(&self.dir.join(".socket.sock"))?;
        s.write_all(cmd.as_bytes()).ok()?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).ok()?;
        serde_json::from_slice(&buf).ok()
    }
}

impl Source for Hypr {
    fn list(&mut self) -> Option<Snapshot> {
        let (shown, reserved) = logic::hypr_monitors(&self.request("j/monitors")?);
        let windows = logic::hypr_clients(&self.request("j/clients")?, &shown);
        Some(Snapshot { windows, reserved })
    }
}

fn hypr_events(path: PathBuf, wake: Wake) {
    loop {
        if let Ok(s) = UnixStream::connect(&path) {
            for line in BufReader::new(s).lines() {
                let Ok(line) = line else { break };
                if logic::hypr_event_relevant(&line) {
                    nudge(&wake);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// `$XDG_RUNTIME_DIR/hypr/<sig>` (Hyprland >= 0.40), else `/tmp/hypr/<sig>`.
fn hypr_dir(sig: &str) -> PathBuf {
    let run = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    if let Some(d) = run.map(|r| r.join("hypr").join(sig)).filter(|d| d.join(".socket.sock").exists()) {
        return d;
    }
    Path::new("/tmp/hypr").join(sig)
}

// ---- KWin ----------------------------------------------------------------

const KWIN_PLUGIN: &str = "frostpane-geometry";

struct Receiver {
    latest: Arc<Mutex<Option<Snapshot>>>,
    wake: Wake,
}

#[zbus::interface(name = "org.frostpane.Geometry")]
impl Receiver {
    fn report(&self, json: String) {
        if let Some(s) = logic::kwin_report(&json) {
            *self.latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(s);
            nudge(&self.wake);
        }
    }
}

struct KWin {
    conn: zbus::blocking::Connection,
    latest: Arc<Mutex<Option<Snapshot>>>,
    file: PathBuf,
}

fn scripting(conn: &zbus::blocking::Connection) -> zbus::Result<zbus::blocking::Proxy<'static>> {
    zbus::blocking::Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting")
}

impl KWin {
    fn start(wake: Wake) -> zbus::Result<KWin> {
        let latest = Arc::new(Mutex::new(None));
        let conn = zbus::blocking::Connection::session()?;
        conn.object_server().at(logic::KWIN_PATH, Receiver { latest: latest.clone(), wake })?;
        let name = conn.unique_name().map(|n| n.to_string()).unwrap_or_default();
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let file = dir.join(format!("frostpane-kwin-{}.js", std::process::id()));
        std::fs::write(&file, logic::kwin_script(&name)).map_err(|e| zbus::Error::Failure(e.to_string()))?;
        let p = scripting(&conn)?;
        let _: zbus::Result<bool> = p.call("unloadScript", &(KWIN_PLUGIN,));
        let id: i32 = p.call("loadScript", &(file.to_string_lossy().as_ref(), KWIN_PLUGIN))?;
        let _: zbus::Result<()> = p.call("start", &());
        log::info!("KWin geometry script loaded (id {id})");
        Ok(KWin { conn, latest, file })
    }
}

impl Source for KWin {
    fn list(&mut self) -> Option<Snapshot> {
        self.latest.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for KWin {
    fn drop(&mut self) {
        if let Ok(p) = scripting(&self.conn) {
            let _: zbus::Result<bool> = p.call("unloadScript", &(KWIN_PLUGIN,));
        }
        let _ = std::fs::remove_file(&self.file);
    }
}

/// `org.kde.KWin` has an owner on the session bus.
pub fn kwin_on_bus() -> bool {
    let Ok(conn) = zbus::blocking::Connection::session() else { return false };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return false };
    let Ok(name) = zbus::names::BusName::try_from("org.kde.KWin") else { return false };
    dbus.name_has_owner(name).unwrap_or(false)
}

// ---- selection -----------------------------------------------------------

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name(name.into()).spawn(f);
}

/// Opens the source the selection picked; `None` when it is unavailable.
pub fn open(source: fp_platform::select::WindowSource, wake: Wake) -> Option<Box<dyn Source>> {
    use fp_platform::select::WindowSource as W;
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    match source {
        W::Sway => {
            let path = PathBuf::from(var("SWAYSOCK")?);
            let mut s = Sway { path: path.clone(), stream: None };
            s.list()?;
            spawn("frostpane sway events", move || sway_events(path, wake));
            Some(Box::new(s))
        }
        W::Hyprland => {
            let dir = hypr_dir(&var("HYPRLAND_INSTANCE_SIGNATURE")?);
            let mut h = Hypr { dir: dir.clone() };
            h.list()?;
            spawn("frostpane hyprland events", move || hypr_events(dir.join(".socket2.sock"), wake));
            Some(Box::new(h))
        }
        W::KWin => match KWin::start(wake) {
            Ok(k) => Some(Box::new(k)),
            Err(e) => {
                log::warn!("KWin script source unavailable: {e}");
                None
            }
        },
        W::None => None,
    }
}
