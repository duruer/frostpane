//! The UI connection shared by the platform services, the overlay panels and
//! the sprite windows (porting design 3.2), plus the EWMH reads they share.
//! One `XCBConnection` for the process: wgpu needs its raw `xcb_connection_t`
//! and render threads read the pointer through it (x11rb connections are
//! `Sync`). The hook watcher has its own connection (`watch.rs`).

use crate::logic::{self, ClientFacts, Edge, Extents, TreeChild, WinType, WmState};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{
    AtomEnum, Colormap, ColormapAlloc, ConnectionExt as _, MapState, Visualid, Window, WindowClass,
};
use x11rb::xcb_ffi::XCBConnection;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        _NET_CLIENT_LIST,
        _NET_CLIENT_LIST_STACKING,
        _NET_ACTIVE_WINDOW,
        _NET_CURRENT_DESKTOP,
        _NET_WORKAREA,
        _NET_FRAME_EXTENTS,
        _GTK_FRAME_EXTENTS,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_STICKY,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
        _NET_WM_DESKTOP,
        _NET_WM_PID,
        _NET_WM_WINDOW_OPACITY,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_BYPASS_COMPOSITOR,
        _NET_WM_NAME,
        UTF8_STRING,
        WM_STATE,
    }
}

/// Our own override-redirect windows: panels and sprites.
#[derive(Default)]
pub struct Own {
    pub panels: HashSet<Window>,
    /// Sprite -> (topmost, mapped).
    pub sprites: HashMap<Window, (bool, bool)>,
    /// Sprites that currently let clicks through.
    pub click_through: HashSet<Window>,
}

pub struct Xc {
    pub conn: XCBConnection,
    pub screen: usize,
    pub root: Window,
    pub atoms: Atoms,
    /// Depth-32 TrueColor visual and its colormap.
    pub argb: Option<(Visualid, Colormap)>,
    pub pid: u32,
    pub shape: bool,
    pub randr: bool,
    cm: u32,
    names: Mutex<HashMap<u32, String>>,
    own: Mutex<Own>,
}

static X: OnceLock<Option<Xc>> = OnceLock::new();

/// The process-wide UI connection, opened on first use; `None` when the
/// display cannot be opened.
pub fn get() -> Option<&'static Xc> {
    X.get_or_init(|| match Xc::open() {
        Ok(x) => Some(x),
        Err(e) => {
            log::warn!("x11: cannot open the display: {e}");
            None
        }
    })
    .as_ref()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Xc {
    fn open() -> Result<Xc, String> {
        let (conn, screen) = XCBConnection::connect(None).map_err(|e| e.to_string())?;
        let atoms = Atoms::new(&conn).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?;
        let cm = conn
            .intern_atom(false, format!("_NET_WM_CM_S{screen}").as_bytes())
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        let s = &conn.setup().roots[screen];
        let root = s.root;
        let visual = s
            .allowed_depths
            .iter()
            .filter(|d| d.depth == 32)
            .flat_map(|d| d.visuals.iter())
            .find(|v| v.class == x11rb::protocol::xproto::VisualClass::TRUE_COLOR)
            .map(|v| v.visual_id);
        let argb = match visual {
            Some(v) => {
                let cmap = conn.generate_id().map_err(|e| e.to_string())?;
                conn.create_colormap(ColormapAlloc::NONE, cmap, root, v).map_err(|e| e.to_string())?;
                Some((v, cmap))
            }
            None => None,
        };
        let ext = |name: &str| {
            conn.query_extension(name.as_bytes()).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.present)
        };
        let shape = ext("SHAPE");
        let randr = ext("RANDR");
        if randr {
            // Version negotiation is required before RandR 1.5 requests.
            let _ = conn.randr_query_version(1, 5).map(|c| c.reply());
        }
        if shape {
            use x11rb::protocol::shape::ConnectionExt as _;
            let _ = conn.shape_query_version().map(|c| c.reply());
        }
        let _ = conn.flush();
        Ok(Xc {
            conn,
            screen,
            root,
            atoms,
            argb,
            pid: std::process::id(),
            shape,
            randr,
            cm,
            names: Mutex::new(HashMap::new()),
            own: Mutex::new(Own::default()),
        })
    }

    pub fn own(&self) -> MutexGuard<'_, Own> {
        lock(&self.own)
    }

    /// A compositing manager owns `_NET_WM_CM_S<screen>` (ARGB windows are
    /// only transparent with one).
    pub fn composited(&self) -> bool {
        self.conn.get_selection_owner(self.cm).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.owner != 0)
    }

    /// Drops queued events and errors (the UI connection selects none).
    pub fn drain(&self) {
        let _ = self.conn.flush();
        while let Ok(Some(_)) = self.conn.poll_for_event() {}
    }

    pub fn prop32(&self, w: Window, atom: u32) -> Option<Vec<u32>> {
        let r = self.conn.get_property(false, w, atom, AtomEnum::ANY, 0, 4096).ok()?.reply().ok()?;
        let v: Vec<u32> = r.value32()?.collect();
        (!v.is_empty()).then_some(v)
    }

    pub fn prop_bytes(&self, w: Window, atom: impl Into<u32>) -> Option<Vec<u8>> {
        let r = self.conn.get_property(false, w, atom.into(), AtomEnum::ANY, 0, 1 << 20).ok()?.reply().ok()?;
        (r.format == 8 && !r.value.is_empty()).then_some(r.value)
    }

    /// Atom name, cached.
    pub fn name(&self, atom: u32) -> String {
        if let Some(n) = lock(&self.names).get(&atom) {
            return n.clone();
        }
        let n = self
            .conn
            .get_atom_name(atom)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.name).into_owned())
            .unwrap_or_default();
        lock(&self.names).insert(atom, n.clone());
        n
    }

    pub fn types(&self, atoms: &[u32]) -> Vec<WinType> {
        atoms.iter().map(|a| WinType::from_name(&self.name(*a))).collect()
    }

    pub fn state(&self, atoms: &[u32]) -> WmState {
        let names: Vec<String> = atoms.iter().map(|a| self.name(*a)).collect();
        WmState::from_names(names.iter().map(String::as_str))
    }

    pub fn current_desktop(&self) -> Option<u32> {
        self.prop32(self.root, self.atoms._NET_CURRENT_DESKTOP).and_then(|v| v.first().copied())
    }

    pub fn active_window(&self) -> Option<Window> {
        self.prop32(self.root, self.atoms._NET_ACTIVE_WINDOW).and_then(|v| v.first().copied()).filter(|w| *w != 0)
    }

    /// Direct children of the root, bottom to top.
    pub fn stack(&self) -> Vec<Window> {
        self.conn.query_tree(self.root).ok().and_then(|c| c.reply().ok()).map(|r| r.children).unwrap_or_default()
    }

    /// Managed clients bottom to top: `_NET_CLIENT_LIST_STACKING`, else the
    /// viewable, non-override-redirect children of the root (no EWMH WM).
    pub fn clients(&self) -> Vec<Window> {
        if let Some(v) = self.prop32(self.root, self.atoms._NET_CLIENT_LIST_STACKING) {
            return v;
        }
        if let Some(v) = self.prop32(self.root, self.atoms._NET_CLIENT_LIST) {
            return v;
        }
        let kids = self.stack();
        let cookies: Vec<_> = kids.iter().map(|w| self.conn.get_window_attributes(*w).ok()).collect();
        let tree: Vec<TreeChild> = kids
            .iter()
            .zip(cookies)
            .filter_map(|(w, c)| {
                let a = c?.reply().ok()?;
                Some(TreeChild {
                    id: *w,
                    viewable: a.map_state == MapState::VIEWABLE,
                    override_redirect: a.override_redirect,
                    input_output: a.class == WindowClass::INPUT_OUTPUT,
                })
            })
            .collect();
        logic::tree_clients(&tree)
    }

    /// The root child holding `w` (the WM frame, or `w` itself).
    pub fn frame_of(&self, w: Window) -> Window {
        frame_of(&self.conn, self.root, w)
    }

    /// Facts of every client in `ids`, requests pipelined.
    pub fn facts(&self, ids: &[Window]) -> Vec<ClientFacts> {
        let c = &self.conn;
        let a = &self.atoms;
        let any = AtomEnum::ANY;
        let prop = |w: Window, atom: u32, len: u32| c.get_property(false, w, atom, any, 0, len).ok();
        let cookies: Vec<_> = ids
            .iter()
            .map(|&w| {
                (
                    c.get_geometry(w).ok(),
                    c.translate_coordinates(w, self.root, 0, 0).ok(),
                    c.get_window_attributes(w).ok(),
                    prop(w, a._NET_FRAME_EXTENTS, 4),
                    prop(w, a._GTK_FRAME_EXTENTS, 4),
                    prop(w, a._NET_WM_STATE, 64),
                    prop(w, a.WM_STATE, 2),
                    prop(w, a._NET_WM_WINDOW_TYPE, 16),
                    prop(w, a._NET_WM_DESKTOP, 1),
                    prop(w, a._NET_WM_PID, 1),
                    prop(w, a._NET_WM_WINDOW_OPACITY, 1),
                    prop(w, AtomEnum::WM_CLASS.into(), 64),
                )
            })
            .collect();
        let v32 = |c: Option<x11rb::cookie::Cookie<'_, XCBConnection, x11rb::protocol::xproto::GetPropertyReply>>| {
            c.and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|i| i.collect::<Vec<u32>>()))
        };
        ids.iter()
            .zip(cookies)
            .map(|(&id, (geo, tr, attr, net, gtk, st, wms, ty, desk, pid, op, class))| {
                let geo = geo.and_then(|c| c.reply().ok());
                let tr = tr.and_then(|c| c.reply().ok());
                let geometry = match (geo, tr) {
                    (Some(g), Some(t)) => {
                        Some((i32::from(t.dst_x), i32::from(t.dst_y), u32::from(g.width), u32::from(g.height)))
                    }
                    _ => None,
                };
                let mapped = attr.and_then(|c| c.reply().ok()).is_some_and(|a| a.map_state == MapState::VIEWABLE);
                let first = |v: Option<Vec<u32>>| v.and_then(|v| v.first().copied());
                let state_atoms = v32(st).unwrap_or_default();
                let type_atoms = v32(ty).unwrap_or_default();
                ClientFacts {
                    id,
                    geometry,
                    net_extents: v32(net).and_then(|v| Extents::parse(&v)),
                    gtk_extents: v32(gtk).and_then(|v| Extents::parse(&v)),
                    mapped,
                    iconic_state: first(v32(wms)) == Some(3),
                    state: self.state(&state_atoms),
                    types: self.types(&type_atoms),
                    desktop: first(v32(desk)),
                    pid: first(v32(pid)),
                    opacity: first(v32(op)),
                    owner: class
                        .and_then(|c| c.reply().ok())
                        .map(|r| logic::wm_class_owner(&r.value))
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Tracker listing, top-down (porting design 3.2 "Window source").
    pub fn windows(&self) -> Vec<fp_tracker::WindowInfo> {
        let ids = logic::top_down(&self.clients());
        let current = self.current_desktop();
        self.facts(&ids).iter().filter_map(|f| logic::client_info(f, current, self.pid)).collect()
    }

    fn root_size(&self) -> (u32, u32) {
        self.conn
            .get_geometry(self.root)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|g| (u32::from(g.width), u32::from(g.height)))
            .unwrap_or((0, 0))
    }

    /// Clients whose window type is DOCK, bottom to top.
    pub fn docks(&self) -> Vec<Window> {
        self.clients()
            .into_iter()
            .filter(|w| {
                self.prop32(*w, self.atoms._NET_WM_WINDOW_TYPE)
                    .is_some_and(|t| logic::primary_type(&self.types(&t)) == WinType::Dock)
            })
            .collect()
    }

    /// Strut rects of every dock.
    pub fn reserved(&self) -> Vec<(Edge, PixelRect)> {
        let root = self.root_size();
        self.docks()
            .iter()
            .flat_map(|d| {
                let p = self.prop32(*d, self.atoms._NET_WM_STRUT_PARTIAL);
                let l = self.prop32(*d, self.atoms._NET_WM_STRUT);
                logic::strut_rects(p.as_deref(), l.as_deref(), root)
            })
            .collect()
    }

    fn xft(&self) -> Option<f64> {
        let raw = self.prop_bytes(self.root, AtomEnum::RESOURCE_MANAGER)?;
        logic::xft_dpi(&String::from_utf8_lossy(&raw))
    }

    /// RandR monitors with work areas and winit-compatible scales.
    pub fn monitors(&self) -> Vec<MonitorInfo> {
        let env = std::env::var("WINIT_X11_SCALE_FACTOR").ok();
        let xft = self.xft();
        let reserved = self.reserved();
        let mut raw: Vec<(String, PixelRect, bool, (u32, u32))> = Vec::new();
        if self.randr
            && let Some(r) = self.conn.randr_get_monitors(self.root, true).ok().and_then(|c| c.reply().ok())
        {
            for m in r.monitors {
                let px = PixelRect::new(
                    i32::from(m.x),
                    i32::from(m.y),
                    i32::from(m.x) + i32::from(m.width),
                    i32::from(m.y) + i32::from(m.height),
                );
                if px.width() > 0 && px.height() > 0 {
                    raw.push((self.name(m.name), px, m.primary, (m.width_in_millimeters, m.height_in_millimeters)));
                }
            }
        }
        if raw.is_empty() {
            let (w, h) = self.root_size();
            if w == 0 || h == 0 {
                return Vec::new();
            }
            let s = &self.conn.setup().roots[self.screen];
            let mm = (u32::from(s.width_in_millimeters), u32::from(s.height_in_millimeters));
            raw.push(("screen".into(), PixelRect::new(0, 0, w as i32, h as i32), true, mm));
        }
        if !raw.iter().any(|m| m.2) {
            raw[0].2 = true;
        }
        logic::monitor_order(&mut raw, |m| (m.2, m.1.left));
        let single = raw.len() == 1;
        let workarea = if single && reserved.is_empty() {
            let d = self.current_desktop().unwrap_or(0);
            self.prop32(self.root, self.atoms._NET_WORKAREA).and_then(|v| logic::net_workarea(&v, d))
        } else {
            None
        };
        raw.into_iter()
            .map(|(name, px, primary, mm)| {
                let mut work = logic::work_area(px, &reserved);
                if let Some(wa) = workarea {
                    let c = PixelRect::new(
                        wa.left.max(px.left),
                        wa.top.max(px.top),
                        wa.right.min(px.right),
                        wa.bottom.min(px.bottom),
                    );
                    if c.width() > 0 && c.height() > 0 {
                        work = c;
                    }
                }
                let size = (px.width() as u32, px.height() as u32);
                MonitorInfo {
                    id: name.clone(),
                    name,
                    pixels: px,
                    work,
                    scale: logic::monitor_scale(env.as_deref(), xft, size, mm),
                    primary,
                }
            })
            .collect()
    }

    pub fn shelves(&self) -> Vec<PixelRect> {
        let mons: Vec<PixelRect> = self.monitors().iter().map(|m| m.pixels).collect();
        logic::shelves(&self.reserved(), &mons)
    }

    /// Pointer `(x, y, mask)` on the root, and the root child under it.
    pub fn pointer(&self) -> Option<(i32, i32, u16, Window)> {
        let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        r.same_screen.then_some((i32::from(r.root_x), i32::from(r.root_y), u16::from(r.mask), r.child))
    }

    /// Frames (root children) of this process's managed windows; `above` = only
    /// those with `_NET_WM_STATE_ABOVE` (cards).
    pub fn own_frames(&self, above: bool) -> Vec<Window> {
        let ids = self.clients();
        self.facts(&ids)
            .iter()
            .filter(|f| f.pid == Some(self.pid) && f.mapped && (!above || f.state.above))
            .map(|f| self.frame_of(f.id))
            .collect()
    }

    /// Frame of the `_NET_WM_WINDOW_TYPE_DESKTOP` client, if any.
    pub fn desktop_frame(&self) -> Option<Window> {
        self.clients()
            .into_iter()
            .find(|w| {
                self.prop32(*w, self.atoms._NET_WM_WINDOW_TYPE)
                    .is_some_and(|t| logic::primary_type(&self.types(&t)) == WinType::Desktop)
            })
            .map(|w| self.frame_of(w))
    }

    /// Restacks `w` (no sibling = absolute top or bottom).
    pub fn restack(&self, w: Window, sibling: Option<Window>, mode: x11rb::protocol::xproto::StackMode) {
        let mut aux = x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(mode);
        if let Some(s) = sibling {
            aux = aux.sibling(s);
        }
        let _ = self.conn.configure_window(w, &aux);
    }

    /// Marks an own override-redirect window: dock type (no compositor
    /// shadows by common configs), above/sticky/skip states, class and name.
    pub fn tag_own(&self, w: Window, name: &str) {
        use x11rb::protocol::xproto::PropMode;
        use x11rb::wrapper::ConnectionExt as _;
        let a = &self.atoms;
        let c = &self.conn;
        let _ = c.change_property32(
            PropMode::REPLACE,
            w,
            a._NET_WM_WINDOW_TYPE,
            AtomEnum::ATOM,
            &[a._NET_WM_WINDOW_TYPE_DOCK],
        );
        let st =
            [a._NET_WM_STATE_ABOVE, a._NET_WM_STATE_STICKY, a._NET_WM_STATE_SKIP_TASKBAR, a._NET_WM_STATE_SKIP_PAGER];
        let _ = c.change_property32(PropMode::REPLACE, w, a._NET_WM_STATE, AtomEnum::ATOM, &st);
        let _ = c.change_property32(PropMode::REPLACE, w, a._NET_WM_PID, AtomEnum::CARDINAL, &[self.pid]);
        let _ =
            c.change_property8(PropMode::REPLACE, w, AtomEnum::WM_CLASS, AtomEnum::STRING, b"frostpane\0Frostpane\0");
        let _ = c.change_property8(PropMode::REPLACE, w, a._NET_WM_NAME, a.UTF8_STRING, name.as_bytes());
    }
}

/// The root child holding `w`, walking up at most 16 parents.
pub fn frame_of(conn: &XCBConnection, root: Window, w: Window) -> Window {
    let mut cur = w;
    for _ in 0..16 {
        let Some(r) = conn.query_tree(cur).ok().and_then(|c| c.reply().ok()) else { return w };
        if r.parent == root || r.parent == 0 {
            return cur;
        }
        cur = r.parent;
    }
    w
}
