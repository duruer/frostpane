//! Background D-Bus watchers: one thread on the system bus (logind, UPower,
//! power profiles) and one on the session bus (portal settings, screen
//! savers). They keep a cached snapshot and queue events for the UI thread.

use crate::bus;
use crate::logic::{self, Change, Val, dbus};
use fp_platform::SystemEvent;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use zbus::blocking::{Connection, MessageIterator};

/// Session facts kept up to date by the watcher threads.
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub on_battery: bool,
    pub has_battery: bool,
    pub power_saver: bool,
    pub color_scheme: u32,
    pub reduced_motion: bool,
}

impl Snapshot {
    fn apply(&mut self, c: &Change) {
        match *c {
            Change::OnBattery(b) => self.on_battery = b,
            Change::HasBattery(b) => self.has_battery = b,
            Change::PowerSaver(b) => self.power_saver = b,
            Change::ColorScheme(n) => self.color_scheme = n,
            Change::ReducedMotion(b) => self.reduced_motion = b,
        }
    }
}

#[derive(Default)]
struct Shared {
    /// The snapshot and how many threads finished their first reads.
    state: Mutex<(Snapshot, u8)>,
    ready: Condvar,
}

impl Shared {
    fn change(&self, c: &Change) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0.apply(c);
    }
    fn mark_ready(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).1 += 1;
        self.ready.notify_all();
    }
}

type Watch = fn(&Shared, &Sender<SystemEvent>);

/// The two watcher threads and the event queue.
pub struct Monitor {
    shared: Arc<Shared>,
    rx: Receiver<SystemEvent>,
}

impl Monitor {
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = channel();
        for (name, watch) in [("fp-dbus-system", system as Watch), ("fp-dbus-session", session as Watch)] {
            let (s, t) = (shared.clone(), tx.clone());
            let spawned = std::thread::Builder::new().name(name.into()).spawn(move || watch(&s, &t));
            if spawned.is_err() {
                shared.mark_ready();
            }
        }
        Self { shared, rx }
    }

    /// The cached facts; waits up to `wait` for both threads' first reads.
    pub fn snapshot(&self, wait: Duration) -> Snapshot {
        let guard = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) =
            self.shared.ready.wait_timeout_while(guard, wait, |s| s.1 < 2).unwrap_or_else(|e| e.into_inner());
        guard.0
    }

    pub fn try_event(&self) -> Option<SystemEvent> {
        self.rx.try_recv().ok()
    }
}

fn run_loop(conn: &Connection, shared: &Shared, tx: &Sender<SystemEvent>) {
    for msg in MessageIterator::from(conn) {
        let Ok(msg) = msg else { break };
        let Some(sig) = bus::decode(&msg) else { continue };
        let (events, changes) = logic::react(&sig);
        for c in &changes {
            shared.change(c);
        }
        for e in events {
            if tx.send(e).is_err() {
                return;
            }
        }
    }
}

fn system(shared: &Shared, tx: &Sender<SystemEvent>) {
    let Ok(conn) = Connection::system() else {
        shared.mark_ready();
        return;
    };
    if let Some(Val::Bool(b)) = bus::get_prop(&conn, dbus::UPOWER, dbus::UPOWER_PATH, dbus::UPOWER, "OnBattery") {
        shared.change(&Change::OnBattery(b));
    }
    if let Some(Val::Bool(b)) =
        bus::get_prop(&conn, dbus::UPOWER, dbus::UPOWER_DISPLAY, dbus::UPOWER_DEVICE, "IsPresent")
    {
        shared.change(&Change::HasBattery(b));
    }
    let profile = bus::get_prop(&conn, dbus::PPD_NEW, dbus::PPD_NEW_PATH, dbus::PPD_NEW, "ActiveProfile")
        .or_else(|| bus::get_prop(&conn, dbus::PPD, dbus::PPD_PATH, dbus::PPD, "ActiveProfile"));
    if let Some(Val::Str(s)) = profile {
        shared.change(&Change::PowerSaver(s == "power-saver"));
    }
    let path = bus::session_path(&conn);
    bus::add_rules(&conn, &dbus::system_rules(path.as_deref()));
    shared.mark_ready();
    run_loop(&conn, shared, tx);
}

fn session(shared: &Shared, tx: &Sender<SystemEvent>) {
    let Ok(conn) = Connection::session() else {
        shared.mark_ready();
        return;
    };
    bus::add_rules(&conn, &dbus::session_rules());
    if bus::name_has_owner(&conn, dbus::PORTAL) || bus::name_activatable(&conn, dbus::PORTAL) {
        for (ns, key) in [
            (dbus::APPEARANCE, "color-scheme"),
            (dbus::GNOME_INTERFACE, "enable-animations"),
            (dbus::KDE_GLOBALS, "AnimationDurationFactor"),
        ] {
            if let Some(c) = bus::portal_read(&conn, ns, key).and_then(|v| logic::portal_change(ns, key, &v)) {
                shared.change(&c);
            }
        }
    }
    shared.mark_ready();
    run_loop(&conn, shared, tx);
}
