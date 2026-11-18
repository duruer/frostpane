//! X event watcher (porting design 3.2 "Real hooks"): one background thread
//! with its own connection selects `SubstructureNotify | PropertyChange` on
//! the root and `StructureNotify | PropertyChange` on every client, and turns
//! what it sees into tracker hook events (through `fp_tracker::synth::HookSynth`
//! for drags), display changes (RandR) and the rescale flag (`Xft.dpi`).

use crate::logic;
use crate::x::{Atoms, frame_of};
use fp_core::geom::PixelRect;
use fp_platform::SystemEvent;
use fp_tracker::synth::HookSynth;
use fp_tracker::{HookEvent, WindowInfo};
use std::collections::{HashMap, HashSet};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard, Once, PoisonError};
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::randr::{ConnectionExt as _, NotifyMask};
use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, EventMask, Window};
use x11rb::xcb_ffi::XCBConnection;

/// Where hook events go while `Platform::hooks` holds its guard.
static HOOK_TX: Mutex<Option<Sender<HookEvent>>> = Mutex::new(None);
/// System events for the UI thread (drained by the platform).
static SYSTEM: Mutex<Vec<SystemEvent>> = Mutex::new(Vec::new());
/// `Xft.dpi` changed: the overlay must be rebuilt.
pub static RESCALED: AtomicBool = AtomicBool::new(false);
static START: Once = Once::new();

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn set_hooks(tx: Option<Sender<HookEvent>>) {
    *lock(&HOOK_TX) = tx;
}

pub fn take_system() -> Vec<SystemEvent> {
    std::mem::take(&mut *lock(&SYSTEM))
}

fn push_system(e: SystemEvent) {
    let mut q = lock(&SYSTEM);
    if !q.contains(&e) {
        q.push(e);
    }
}

fn send(ev: HookEvent) {
    if let Some(tx) = lock(&HOOK_TX).as_ref() {
        let _ = tx.send(ev);
    }
}

/// Starts the watcher once per process.
pub fn ensure() {
    START.call_once(|| {
        let r = std::thread::Builder::new().name("fp-x11-watch".into()).spawn(|| {
            if let Err(e) = run() {
                log::warn!("x11 watcher stopped: {e}");
            }
        });
        if let Err(e) = r {
            log::warn!("x11 watcher not started: {e}");
        }
    });
}

struct W {
    conn: XCBConnection,
    root: Window,
    a: Atoms,
    /// client -> frame
    clients: HashMap<Window, Window>,
    frames: HashMap<Window, Window>,
    rects: HashMap<Window, PixelRect>,
    hidden: HashSet<Window>,
    synth: HookSynth,
    xft: Option<f64>,
    last_move: Option<Instant>,
    start: Instant,
}

fn run() -> Result<(), String> {
    let (conn, screen) = XCBConnection::connect(None).map_err(|e| e.to_string())?;
    let a = Atoms::new(&conn).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
    let root = conn.setup().roots[screen].root;
    let mask = EventMask::SUBSTRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE;
    conn.change_window_attributes(root, &ChangeWindowAttributesAux::new().event_mask(mask))
        .map_err(|e| e.to_string())?;
    let randr = conn.query_extension(b"RANDR").ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.present);
    if randr {
        let _ = conn.randr_query_version(1, 5).map(|c| c.reply());
        let m = NotifyMask::SCREEN_CHANGE | NotifyMask::CRTC_CHANGE | NotifyMask::OUTPUT_CHANGE;
        let _ = conn.randr_select_input(root, m);
    }
    let mut w = W {
        conn,
        root,
        a,
        clients: HashMap::new(),
        frames: HashMap::new(),
        rects: HashMap::new(),
        hidden: HashSet::new(),
        synth: HookSynth::new(),
        xft: None,
        last_move: None,
        start: Instant::now(),
    };
    w.xft = w.read_xft();
    w.resync();
    if let Some(act) = w.active() {
        w.synth.foreground(i64::from(act));
    }
    w.feed();
    let fd = w.conn.as_raw_fd();
    loop {
        w.conn.flush().map_err(|e| e.to_string())?;
        while let Some(ev) = w.conn.poll_for_event().map_err(|e| e.to_string())? {
            w.handle(ev);
        }
        w.feed();
        let _ = w.conn.flush();
        // Drag end needs a timeout; otherwise sleep until the server talks.
        let moving = w.last_move.is_some_and(|t| t.elapsed().as_secs_f64() < 1.0);
        let timeout = if moving { 50 } else { 1000 };
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd for the duration of the call.
        unsafe { libc::poll(&mut pfd, 1, timeout) };
    }
}

impl W {
    fn prop32(&self, w: Window, atom: u32) -> Option<Vec<u32>> {
        let r = self.conn.get_property(false, w, atom, AtomEnum::ANY, 0, 4096).ok()?.reply().ok()?;
        r.value32().map(|v| v.collect())
    }

    fn first(&self, w: Window, atom: u32) -> Option<u32> {
        self.prop32(w, atom).and_then(|v| v.first().copied())
    }

    fn active(&self) -> Option<Window> {
        self.first(self.root, self.a._NET_ACTIVE_WINDOW).filter(|w| *w != 0)
    }

    fn read_xft(&self) -> Option<f64> {
        let r = self
            .conn
            .get_property(false, self.root, AtomEnum::RESOURCE_MANAGER, AtomEnum::ANY, 0, 1 << 20)
            .ok()?
            .reply()
            .ok()?;
        logic::xft_dpi(&String::from_utf8_lossy(&r.value))
    }

    fn hidden_now(&self, w: Window) -> bool {
        let atoms = self.prop32(w, self.a._NET_WM_STATE).unwrap_or_default();
        atoms.iter().any(|a| {
            self.conn
                .get_atom_name(*a)
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|r| r.name == b"_NET_WM_STATE_HIDDEN")
        })
    }

    /// Re-reads the client list: watches new clients, forgets gone ones.
    fn resync(&mut self) {
        let list = self
            .prop32(self.root, self.a._NET_CLIENT_LIST_STACKING)
            .or_else(|| self.prop32(self.root, self.a._NET_CLIENT_LIST))
            .unwrap_or_default();
        let now: HashSet<Window> = list.iter().copied().collect();
        let gone: Vec<Window> = self.clients.keys().filter(|c| !now.contains(c)).copied().collect();
        for c in gone {
            self.forget(c);
        }
        let mask = EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE;
        for c in list {
            if self.clients.contains_key(&c) {
                continue;
            }
            let _ = self.conn.change_window_attributes(c, &ChangeWindowAttributesAux::new().event_mask(mask));
            self.track(c);
            if self.hidden_now(c) {
                self.hidden.insert(c);
            }
        }
    }

    /// (Re)learns the frame of a client and its rect.
    fn track(&mut self, c: Window) {
        if let Some(old) = self.clients.remove(&c) {
            self.frames.remove(&old);
        }
        let f = frame_of(&self.conn, self.root, c);
        self.clients.insert(c, f);
        self.frames.insert(f, c);
        if let Some(g) = self.conn.get_geometry(f).ok().and_then(|c| c.reply().ok()) {
            let bw = i32::from(g.border_width) * 2;
            let (x, y) = (i32::from(g.x), i32::from(g.y));
            self.rects.insert(c, PixelRect::new(x, y, x + i32::from(g.width) + bw, y + i32::from(g.height) + bw));
        }
    }

    fn forget(&mut self, c: Window) {
        if let Some(f) = self.clients.remove(&c) {
            self.frames.remove(&f);
        }
        self.rects.remove(&c);
        self.hidden.remove(&c);
    }

    fn feed(&mut self) {
        let now = self.start.elapsed().as_secs_f64();
        let list: Vec<WindowInfo> = self
            .rects
            .iter()
            .map(|(id, r)| WindowInfo { hwnd: i64::from(*id), bounds: Some(*r), ..Default::default() })
            .collect();
        for ev in self.synth.poll(now, &list) {
            send(ev);
        }
    }

    fn handle(&mut self, ev: Event) {
        match ev {
            Event::ConfigureNotify(e) => {
                let Some(&c) = self.frames.get(&e.window) else { return };
                if e.event != self.root {
                    return;
                }
                let bw = i32::from(e.border_width) * 2;
                let (x, y) = (i32::from(e.x), i32::from(e.y));
                let r = PixelRect::new(x, y, x + i32::from(e.width) + bw, y + i32::from(e.height) + bw);
                if self.rects.insert(c, r) != Some(r) {
                    self.last_move = Some(Instant::now());
                }
            }
            Event::ReparentNotify(e) => {
                if self.clients.contains_key(&e.window) {
                    self.track(e.window);
                }
            }
            Event::UnmapNotify(e) => {
                if e.event != e.window || !self.clients.contains_key(&e.window) {
                    return;
                }
                let desk = self.first(e.window, self.a._NET_WM_DESKTOP);
                let cur = self.first(self.root, self.a._NET_CURRENT_DESKTOP);
                let hidden = self.hidden.contains(&e.window) || self.hidden_now(e.window);
                if let Some(ev) = logic::unmap_event(e.window, desk, cur, hidden) {
                    send(ev);
                }
            }
            Event::DestroyNotify(e) => {
                // Gone from `rects`: the next feed reports `Destroy`.
                if self.clients.contains_key(&e.window) {
                    self.forget(e.window);
                }
            }
            Event::PropertyNotify(e) if e.window == self.root => {
                let a = &self.a;
                if e.atom == a._NET_ACTIVE_WINDOW {
                    if let Some(w) = self.active()
                        && let Some(ev) = self.synth.foreground(i64::from(w))
                    {
                        send(ev);
                    }
                } else if e.atom == a._NET_CLIENT_LIST_STACKING || e.atom == a._NET_CLIENT_LIST {
                    self.resync();
                    // Restack or new window: poll fast for a moment.
                    send(HookEvent::Foreground);
                } else if e.atom == a._NET_CURRENT_DESKTOP {
                    send(HookEvent::Foreground);
                } else if e.atom == u32::from(AtomEnum::RESOURCE_MANAGER) {
                    let x = self.read_xft();
                    if x != self.xft {
                        self.xft = x;
                        RESCALED.store(true, Ordering::SeqCst);
                    }
                }
            }
            Event::PropertyNotify(e) if self.clients.contains_key(&e.window) => {
                if e.atom == self.a._NET_WM_STATE {
                    let h = self.hidden_now(e.window);
                    if h && self.hidden.insert(e.window) {
                        send(HookEvent::MinimizeStart(i64::from(e.window)));
                    } else if !h {
                        self.hidden.remove(&e.window);
                    }
                } else if e.atom == self.a._NET_WM_DESKTOP {
                    send(HookEvent::Foreground);
                }
            }
            Event::RandrScreenChangeNotify(_) | Event::RandrNotify(_) => push_system(SystemEvent::DisplaysChanged),
            _ => {}
        }
    }
}
