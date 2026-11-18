//! Live checks against a real X server with an EWMH WM and a compositor
//! (run by `smoke/live.sh`; skipped unless `FP_X11_LIVE=1`). The test makes
//! its own client and dock windows, so it needs nothing but the server.
#![cfg(target_os = "linux")]

use fp_core::geom::PixelRect;
use fp_platform::sprites::Place;
use fp_tracker::HookEvent;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK};
use x11rb::protocol::xproto::{
    AtomEnum, ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, EventMask, MapState, PropMode, Window,
    WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

fn atom(c: &XCBConnection, n: &str) -> u32 {
    c.intern_atom(false, n.as_bytes()).unwrap().reply().unwrap().atom
}

/// A managed client (or dock) window of this test connection.
fn client(c: &XCBConnection, root: Window, x: i16, y: i16, w: u16, h: u16, dock: bool) -> Window {
    let id = c.generate_id().unwrap();
    let aux = CreateWindowAux::new().background_pixel(0x00ff_ffff).event_mask(EventMask::STRUCTURE_NOTIFY);
    c.create_window(0, id, root, x, y, w, h, 0, WindowClass::INPUT_OUTPUT, 0, &aux).unwrap();
    c.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, b"fp-live").unwrap();
    if dock {
        let t = atom(c, "_NET_WM_WINDOW_TYPE");
        let d = atom(c, "_NET_WM_WINDOW_TYPE_DOCK");
        c.change_property32(PropMode::REPLACE, id, t, AtomEnum::ATOM, &[d]).unwrap();
        // 40 px bottom strut over x 0..=999.
        let s = atom(c, "_NET_WM_STRUT_PARTIAL");
        c.change_property32(PropMode::REPLACE, id, s, AtomEnum::CARDINAL, &[0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, 999])
            .unwrap();
    }
    c.map_window(id).unwrap();
    c.flush().unwrap();
    id
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

fn drain_until(rx: &Receiver<HookEvent>, want: &HookEvent) -> Vec<HookEvent> {
    let end = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    while Instant::now() < end {
        if let Ok(e) = rx.recv_timeout(Duration::from_millis(100)) {
            seen.push(e);
            if &e == want {
                return seen;
            }
        }
    }
    panic!("no {want:?}; saw {seen:?}");
}

fn ball() -> Pixmap {
    let mut pm = Pixmap::new(64, 64).unwrap();
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(240, 245, 255, 255));
    let path = PathBuilder::from_circle(32.0, 32.0, 30.0).unwrap();
    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    pm
}

/// Lets the server handle what the backend connection sent (another client).
fn settle() {
    std::thread::sleep(Duration::from_millis(100));
}

fn stack(c: &XCBConnection, root: Window) -> Vec<Window> {
    c.query_tree(root).unwrap().reply().unwrap().children
}

#[test]
fn live_x11() {
    if std::env::var("FP_X11_LIVE").as_deref() != Ok("1") {
        eprintln!("live_x11 skipped (set FP_X11_LIVE=1 inside a test X session)");
        return;
    }
    let (c, screen) = XCBConnection::connect(None).unwrap();
    let root = c.setup().roots[screen].root;
    let parts = fp_x11::backend(false);
    let os = parts.services.clone();
    let mut sprites = parts.sprites;
    assert!(!parts.limits.no_overlay, "a compositor is expected");

    // ---- monitors, dock strut -> work area + shelf -------------------------
    let mons = os.monitors();
    assert!(!mons.is_empty());
    println!("monitors: {mons:?}");
    let dock = client(&c, root, 0, 1040, 1000, 40, true);
    wait_for("dock shelf", || !os.taskbar_shelves().is_empty());
    let shelves = os.taskbar_shelves();
    println!("shelves: {shelves:?}");
    assert!(shelves.contains(&PixelRect::new(0, 1040, 1000, 1080)));
    assert_eq!(os.monitors()[0].work.bottom, 1040);
    assert_eq!(os.shelves_reader()(), shelves);

    // ---- window source -----------------------------------------------------
    let (tx, rx) = channel();
    let _hooks = os.hooks(tx).expect("hooks");
    let win = client(&c, root, 300, 300, 400, 300, false);
    wait_for("client listed", || os.windows().iter().any(|w| w.hwnd == i64::from(win)));
    let listed = os.windows();
    assert!(listed.iter().all(|w| w.hwnd != i64::from(dock)), "docks are not listed");
    let me = listed.iter().find(|w| w.hwnd == i64::from(win)).unwrap();
    println!("client: {me:?}");
    let l = fp_tracker::filter(me).expect("client passes the filter");
    assert!(l.bounds.width() >= 400);
    assert!(l.bounds.top < 300, "frame extents include the title bar: {:?}", l.bounds);

    // ---- hooks: drag start/end, minimize -----------------------------------
    std::thread::sleep(Duration::from_millis(300));
    while rx.try_recv().is_ok() {}
    for i in 0..5 {
        c.configure_window(win, &ConfigureWindowAux::new().x(320 + i * 10).y(310)).unwrap();
        c.flush().unwrap();
        std::thread::sleep(Duration::from_millis(40));
    }
    let seen = drain_until(&rx, &HookEvent::MoveSizeEnd);
    assert!(seen.contains(&HookEvent::MoveSizeStart), "{seen:?}");

    // ---- sprites -----------------------------------------------------------
    let id = sprites.create().expect("sprite");
    let sw = id.0 as Window;
    assert!(sprites.set_bitmap(id, &ball()));
    sprites.place(id, Place::Topmost);
    sprites.present(id, 340, 360, 255);
    settle();
    let g = c.get_geometry(sw).unwrap().reply().unwrap();
    assert_eq!((g.x, g.y, g.width, g.height), (340, 360, 64, 64));
    let a = c.get_window_attributes(sw).unwrap().reply().unwrap();
    assert_eq!(a.map_state, MapState::VIEWABLE);
    assert!(a.override_redirect);
    let input = |c: &XCBConnection| c.shape_get_rectangles(sw, SK::INPUT).unwrap().reply().unwrap().rectangles.len();
    assert_eq!(input(&c), 0, "created click-through");
    sprites.set_click_through(id, false);
    settle();
    assert!(input(&c) > 1, "input follows the alpha");
    sprites.set_click_through(id, true);
    settle();
    assert_eq!(input(&c), 0);
    assert_eq!(*stack(&c, root).last().unwrap(), sw, "topmost sprite on top");
    // Fade re-uploads; must not fail.
    sprites.present(id, 340, 360, 100);

    // Pointer over the sprite: our sprite is on top, nothing covers it.
    c.warp_pointer(0u32, root, 0, 0, 0, 0, 372, 392).unwrap();
    c.sync().unwrap();
    c.flush().unwrap();
    assert_eq!(os.cursor(), Some((372, 392)));
    assert!(!sprites.covered(id, (372, 392)));
    // Sent to the desktop band, under the client window: covered.
    sprites.place(id, Place::NotTopmost);
    sprites.stack_desktop(&[id]);
    settle();
    c.flush().unwrap();
    let s = stack(&c, root);
    let frame_pos = s.iter().position(|w| {
        c.query_tree(*w).ok().and_then(|r| r.reply().ok()).is_some_and(|r| r.children.contains(&win)) || *w == win
    });
    let sprite_pos = s.iter().position(|w| *w == sw);
    println!("stack: sprite {sprite_pos:?} client frame {frame_pos:?}");
    assert!(sprite_pos < frame_pos);
    assert!(sprites.covered(id, (372, 392)));
    assert!(sprites.taskbar_above(id), "the dock lies above a desktop-band sprite");
    assert!(sprites.lowest_taskbar().is_some());
    // Back to the front.
    sprites.place(id, Place::Topmost);
    sprites.raise_front(&[id]);
    settle();
    assert_eq!(*stack(&c, root).last().unwrap(), sw);
    assert!(!sprites.taskbar_above(id));
    sprites.hide(id);
    settle();
    let a = c.get_window_attributes(sw).unwrap().reply().unwrap();
    assert_ne!(a.map_state, MapState::VIEWABLE);
    sprites.pump();
    sprites.destroy(id);
    settle();
    assert!(c.get_window_attributes(sw).unwrap().reply().is_err());

    // ---- buttons, over_own, fullscreen -------------------------------------
    assert!(!os.left_button_down() && !os.right_button_down());
    assert!(!os.over_own((372, 392)));
    let fs = sprites.fullscreen();
    println!("active window rect: {fs:?}");

    // ---- minimize via the WM (ICCCM WM_CHANGE_STATE) -> MinimizeStart ------
    let change = atom(&c, "WM_CHANGE_STATE");
    let ev = x11rb::protocol::xproto::ClientMessageEvent::new(32, win, change, [3u32, 0, 0, 0, 0]);
    c.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev).unwrap();
    c.flush().unwrap();
    drain_until(&rx, &HookEvent::MinimizeStart(i64::from(win)));
    wait_for("minimized client is iconic", || os.windows().iter().any(|w| w.hwnd == i64::from(win) && w.iconic));

    // ---- destroy -> Destroy ------------------------------------------------
    c.destroy_window(win).unwrap();
    c.flush().unwrap();
    drain_until(&rx, &HookEvent::Destroy(i64::from(win)));
    c.destroy_window(dock).unwrap();
    c.flush().unwrap();
}
