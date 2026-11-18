//! Smoke run: prints what the platform layer sees and exercises the driver,
//! hooks and system watcher. Exits by itself: `cargo run -p fp-win --example win_smoke -- --seconds 3`.

#[cfg(windows)]
fn main() {
    use fp_win::windows_impl::*;
    use fp_win::{Platform, Shell};
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    let secs: u64 = std::env::args().skip_while(|a| a != "--seconds").nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    set_dpi_awareness();
    let p = Win;
    for m in p.monitors() {
        println!("monitor {} '{}' {:?} scale {} primary {}", m.id, m.name, m.pixels, m.scale, m.primary);
    }
    println!("lang {} tz {} corner {}", p.os_language(), p.time_zone_id(), p.window_corner());
    println!("power {:?} reduced_motion {} cursor {:?}", p.power(), p.reduced_motion(), p.cursor());
    println!("shelves {:?} windows {}", p.taskbar_shelves(), p.windows().len());
    println!("taskbar light {} login on {} latitude {:?}", p.taskbar_light(), p.login_is_on(), p.region_latitude());
    println!("wallpaper screens {:?}", p.wallpaper_screens());

    let ticks = Rc::new(Cell::new(0u32));
    let t2 = ticks.clone();
    let _driver = Driver::new(10, move || t2.set(t2.get() + 1)).expect("driver window");
    let _watch = SystemWatcher::new(|ev, asleep| println!("system {ev:?} asleep-changed {asleep:?}"));
    let (tx, rx) = std::sync::mpsc::channel();
    let hooks = HookSet::install(tx);
    println!("hooks installed {}", hooks.count());

    let end = Instant::now() + Duration::from_secs(secs);
    let mut events = 0;
    while Instant::now() < end && pump_messages(20) {
        while rx.try_recv().is_ok() {
            events += 1;
        }
    }
    println!("driver ticks {} hook events {}", ticks.get(), events);
}

#[cfg(not(windows))]
fn main() {
    println!("fp-win smoke runs on Windows only");
}
