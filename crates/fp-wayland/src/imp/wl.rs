//! The backend's own Wayland connection (separate from winit's): globals,
//! outputs, the pointer over our surfaces, layer-surface configures and the
//! shm pool. Everything runs on the UI thread; events are dispatched
//! without blocking from the backends' calls (`pump`, `cursor`, ...).

use crate::logic;
use sctk::compositor::{CompositorHandler, CompositorState, Region};
use sctk::output::{OutputHandler, OutputState};
use sctk::registry::{ProvidesRegistryState, RegistryState};
use sctk::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use sctk::seat::{Capability, SeatHandler, SeatState};
use sctk::shell::WaylandSurface;
use sctk::shell::wlr_layer::{LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use sctk::shm::slot::SlotPool;
use sctk::shm::{Shm, ShmHandler};
use sctk::subcompositor::SubcompositorState;
use sctk::{
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_subcompositor, registry_handlers,
};
use smithay_client_toolkit as sctk;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};

/// Linux input codes of the buttons we report.
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

/// Pointer over one of our surfaces, in pixels; read by render threads.
static CURSOR: AtomicU64 = AtomicU64::new(0);
static CURSOR_ON: AtomicBool = AtomicBool::new(false);

/// Last pointer position over our own surfaces (virtual-desktop pixels).
pub fn cursor() -> Option<(i32, i32)> {
    if !CURSOR_ON.load(Ordering::Relaxed) {
        return None;
    }
    let v = CURSOR.load(Ordering::Relaxed);
    Some(((v >> 32) as u32 as i32, v as u32 as i32))
}

fn set_cursor(p: Option<(i32, i32)>) {
    if let Some((x, y)) = p {
        CURSOR.store((u64::from(x as u32) << 32) | u64::from(y as u32), Ordering::Relaxed);
    }
    CURSOR_ON.store(p.is_some(), Ordering::Relaxed);
}

/// Dispatch state of the connection.
pub struct State {
    registry: RegistryState,
    outputs: OutputState,
    pub compositor: CompositorState,
    pub subcompositor: Option<SubcompositorState>,
    pub layer: LayerShell,
    shm: Shm,
    seat: SeatState,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    pointer: Option<wl_pointer::WlPointer>,
    pub pool: Option<SlotPool>,
    /// Last configure size per layer surface (by its `wl_surface`).
    pub configured: HashMap<ObjectId, (u32, u32)>,
    /// Layer surfaces the compositor closed.
    pub closed: HashSet<ObjectId>,
    /// Logical origin of our input surfaces (pointer to global pixels).
    pub origins: HashMap<ObjectId, (f64, f64)>,
    /// Pixel rects of our shown sprites (`Platform::over_own`).
    pub own_rects: HashMap<ObjectId, fp_core::geom::PixelRect>,
    /// Global scale of the synthetic pixel space.
    pub scale: f64,
    focus: Option<ObjectId>,
    pub left: bool,
    pub right: bool,
    /// Outputs were added, removed or changed since the last read.
    pub outputs_dirty: bool,
}

impl State {
    fn pointer_at(&self, surface: &wl_surface::WlSurface, local: (f64, f64)) -> Option<(i32, i32)> {
        let origin = self.origins.get(&surface.id())?;
        Some(logic::local_to_pixels(*origin, local, self.scale))
    }
}

/// The connection with its queue and state.
pub struct Wl {
    pub conn: Connection,
    queue: EventQueue<State>,
    pub qh: QueueHandle<State>,
    pub st: State,
}

impl Wl {
    /// Connects to `$WAYLAND_DISPLAY` and binds what the backend needs;
    /// fails without `zwlr_layer_shell_v1`.
    pub fn connect() -> Result<Wl, String> {
        let conn = Connection::connect_to_env().map_err(|e| format!("wayland connect: {e}"))?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("wayland registry: {e}"))?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
        let subcompositor = SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &qh).ok();
        let layer = LayerShell::bind(&globals, &qh).map_err(|e| format!("zwlr_layer_shell_v1: {e}"))?;
        let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
        let viewporter = globals.bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ()).ok();
        let mut st = State {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            compositor,
            subcompositor,
            layer,
            shm,
            seat: SeatState::new(&globals, &qh),
            viewporter,
            pointer: None,
            pool: None,
            configured: HashMap::new(),
            closed: HashSet::new(),
            origins: HashMap::new(),
            own_rects: HashMap::new(),
            scale: 1.0,
            focus: None,
            left: false,
            right: false,
            outputs_dirty: true,
        };
        // Output info, then xdg-output logical geometry, then seat caps.
        for _ in 0..3 {
            queue.roundtrip(&mut st).map_err(|e| format!("wayland roundtrip: {e}"))?;
        }
        st.pool = SlotPool::new(256 * 256 * 4, &st.shm).ok();
        Ok(Wl { conn, queue, qh, st })
    }

    /// Non-blocking: flush, read what is there, dispatch.
    pub fn dispatch(&mut self) {
        let _ = self.conn.flush();
        // Read only when the socket has data: the Vulkan WSI of the render
        // threads reads the same display, and libwayland's `read_events`
        // waits for every prepared reader, so reading from an idle socket
        // would park the UI thread until the next event (a Fifo swap chain
        // whose surface is occluded may wait for seconds).
        if let Some(g) = self.conn.prepare_read()
            && readable(g.connection_fd())
        {
            let _ = g.read();
        }
        if let Err(e) = self.queue.dispatch_pending(&mut self.st) {
            log::warn!("wayland dispatch: {e}");
        }
        let _ = self.conn.flush();
    }

    pub fn roundtrip(&mut self) {
        if let Err(e) = self.queue.roundtrip(&mut self.st) {
            log::warn!("wayland roundtrip: {e}");
        }
    }

    /// Waits (bounded) for the first configure of a layer surface.
    pub fn wait_configure(&mut self, surface: &wl_surface::WlSurface) -> Option<(u32, u32)> {
        let id = surface.id();
        let end = Instant::now() + Duration::from_secs(2);
        while Instant::now() < end {
            if let Some(s) = self.st.configured.get(&id) {
                return Some(*s);
            }
            if self.st.closed.contains(&id) {
                return None;
            }
            self.roundtrip();
        }
        None
    }

    /// Outputs with their logical geometry, in registry order.
    pub fn outputs(&self) -> Vec<(wl_output::WlOutput, logic::Output)> {
        self.st
            .outputs
            .outputs()
            .filter_map(|o| {
                let info = self.st.outputs.info(&o)?;
                let mode = info.modes.iter().find(|m| m.current).map(|m| m.dimensions);
                let rotated = matches!(
                    info.transform,
                    wl_output::Transform::_90
                        | wl_output::Transform::_270
                        | wl_output::Transform::Flipped90
                        | wl_output::Transform::Flipped270
                );
                let scale = logic::output_scale(info.scale_factor, mode, info.logical_size, rotated);
                let (x, y) = info.logical_position.unwrap_or(info.location);
                let (w, h) = info.logical_size.or_else(|| {
                    mode.map(|(w, h)| {
                        let (w, h) = if rotated { (h, w) } else { (w, h) };
                        let k = info.scale_factor.max(1);
                        (w / k, h / k)
                    })
                })?;
                let name = info.name.clone().unwrap_or_else(|| format!("WL-{}", info.id));
                let description = info.description.clone().unwrap_or_else(|| format!("{} {}", info.make, info.model));
                Some((o, logic::Output { name, description: description.trim().to_string(), x, y, w, h, scale }))
            })
            .collect()
    }

    /// An empty input region (click-through).
    pub fn empty_region(&self) -> Option<Region> {
        Region::new(&self.st.compositor).ok()
    }

    /// A region of `rects` (surface-local logical).
    pub fn region(&self, rects: &[(i32, i32, i32, i32)]) -> Option<Region> {
        let r = Region::new(&self.st.compositor).ok()?;
        for &(x, y, w, h) in rects {
            r.add(x, y, w, h);
        }
        Some(r)
    }

    /// A viewport for `surface` when `wp_viewporter` exists.
    pub fn viewport(&self, surface: &wl_surface::WlSurface) -> Option<wp_viewport::WpViewport> {
        self.st.viewporter.as_ref().map(|v| v.get_viewport(surface, &self.qh, ()))
    }

    /// The `wl_display*` for wgpu.
    pub fn display_ptr(&self) -> *mut std::ffi::c_void {
        self.conn.backend().display_ptr().cast()
    }
}

/// The socket has data to read (no waiting).
fn readable(fd: std::os::fd::BorrowedFd<'_>) -> bool {
    use std::os::fd::AsRawFd;
    let mut p = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd, zero timeout.
    let n = unsafe { libc::poll(&mut p, 1, 0) };
    n > 0 && p.revents & libc::POLLIN != 0
}

/// The compositor offers `zwlr_layer_shell_v1` at version 3 or later.
pub fn probe_layer_shell() -> bool {
    struct Probe;
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Probe {
        fn event(
            _: &mut Self,
            _: &wl_registry::WlRegistry,
            _: wl_registry::Event,
            _: &GlobalListContents,
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    let Ok(conn) = Connection::connect_to_env() else { return false };
    let Ok((globals, _queue)) = registry_queue_init::<Probe>(&conn) else { return false };
    globals.contents().with_list(|list| list.iter().any(|g| g.interface == "zwlr_layer_shell_v1" && g.version >= 3))
}

// ---- handlers ------------------------------------------------------------

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_dirty = true;
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_dirty = true;
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.outputs_dirty = true;
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.closed.insert(layer.wl_surface().id());
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        c: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.configured.insert(layer.wl_surface().id(), c.new_size);
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
            self.focus = None;
            set_cursor(None);
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.focus = Some(e.surface.id());
                    set_cursor(self.pointer_at(&e.surface, e.position));
                }
                PointerEventKind::Leave { .. } => {
                    if self.focus.as_ref() == Some(&e.surface.id()) {
                        self.focus = None;
                        self.left = false;
                        self.right = false;
                        set_cursor(None);
                    }
                }
                PointerEventKind::Press { button, .. } | PointerEventKind::Release { button, .. } => {
                    let down = matches!(e.kind, PointerEventKind::Press { .. });
                    match button {
                        BTN_LEFT => self.left = down,
                        BTN_RIGHT => self.right = down,
                        _ => {}
                    }
                }
                PointerEventKind::Axis { .. } => {}
            }
        }
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for State {
    fn event(
        _: &mut Self,
        _: &wp_viewporter::WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewport::WpViewport, ()> for State {
    fn event(
        _: &mut Self,
        _: &wp_viewport::WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_compositor!(State);
delegate_subcompositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_layer!(State);
delegate_registry!(State);
