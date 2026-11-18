//! Session-bus behaviour against a private bus. Runs for real only under
//! `dbus-run-session -- cargo test -p fp-linux --test dbus`; without a
//! session bus address the test returns at once.
#![cfg(target_os = "linux")]

use fp_linux::Services;
use fp_platform::{Shell, SystemEvent, SystemEvents};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::zvariant::Value;

type Seen = Rc<RefCell<Vec<(SystemEvent, Option<bool>)>>>;

#[test]
fn session_bus_signals_reach_the_callback() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        eprintln!("no session bus: skipped");
        return;
    }
    let mock = Connection::session().unwrap();

    assert!(!fp_linux::kwin_on_bus());
    mock.request_name("org.kde.KWin").unwrap();
    assert!(fp_linux::kwin_on_bus());

    let s = Services::new();
    let got: Seen = Rc::default();
    let sink = got.clone();
    let _guard = s.watch(Box::new(move |e, asleep| sink.borrow_mut().push((e, asleep)))).unwrap();
    // Waits for the watcher threads' first reads, so the match rules exist.
    assert!(!s.taskbar_light());

    mock.emit_signal(
        None::<&str>,
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
        "SettingChanged",
        &("org.freedesktop.appearance", "color-scheme", Value::from(2u32)),
    )
    .unwrap();
    mock.emit_signal(
        None::<&str>,
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
        "ActiveChanged",
        &(true,),
    )
    .unwrap();

    let end = Instant::now() + Duration::from_secs(5);
    while got.borrow().len() < 2 && Instant::now() < end {
        s.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    let got = got.borrow();
    assert!(got.contains(&(SystemEvent::ThemeChanged, None)), "{got:?}");
    assert!(got.contains(&(SystemEvent::DisplayOff, Some(true))), "{got:?}");
    assert!(s.taskbar_light());
}
