//! Smoke test: every effect on every monitor, `--seconds N` each (default 3).
//!
//! Also exercises share, wind, the behind-windows hand-over (snow), screen
//! updates with a fake window ledge and a final rebuild. Pumps the UI message
//! loop like the real app. Exits by itself with status 1 if nothing rendered.
//!
//! `cargo run -p fp-overlay --example overlay_smoke -- --seconds 2`
//! `--effects snow,rain` limits the list.

#[cfg(windows)]
fn main() {
    use fp_core::Frame;
    use fp_core::falling::Falling;
    use fp_core::surface::{ScreenSnap, Surface, SurfaceId};
    use fp_core::wind::Wind;
    use fp_overlay::{EffectFactory, OverlayHost, PanelInfo};
    use fp_petals::{PetalField, PetalKind};
    use fp_rain::RainField;
    use fp_sky::{FireflyField, StarfallField};
    use fp_snow::{SnowConfig, SnowField};
    use fp_win::Platform;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};

    let args: Vec<String> = std::env::args().collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    let secs: f64 = value("--seconds").and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let wanted: Option<Vec<String>> = value("--effects").map(|s| s.split(',').map(str::to_owned).collect());

    fp_win::windows_impl::set_dpi_awareness();
    let mons = fp_win::windows_impl::Win.monitors();
    let mut host = OverlayHost::new(fp_win::overlay::WinOverlay::new(), &mons).expect("overlay");
    println!("panels {}", host.panels());

    let pump = |dur: Duration, host: &OverlayHost| {
        let end = Instant::now() + dur;
        let mut last_poll = Instant::now();
        while Instant::now() < end {
            // SAFETY: standard message loop on the creating thread.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if last_poll.elapsed() >= Duration::from_millis(200) {
                let dt = last_poll.elapsed().as_secs_f64().min(0.25);
                last_poll = Instant::now();
                let snaps: Vec<ScreenSnap> = host
                    .infos()
                    .iter()
                    .map(|p| {
                        let mut ledge = Surface::new(
                            SurfaceId::Window(1),
                            Frame::new(p.width * 0.3, p.height * 0.4, p.width * 0.25, p.height * 0.3),
                        );
                        ledge.owner = "smoke".into();
                        ScreenSnap { frame: p.frame, shown: true, ground: true, surfaces: vec![ledge] }
                    })
                    .collect();
                host.update(&snaps, dt, false);
            }
            std::thread::sleep(Duration::from_millis(8));
        }
    };

    type Make = fn(&PanelInfo) -> Box<dyn Falling>;
    let effects: [(&str, Make); 6] = [
        ("snow", |p| {
            Box::new(SnowField::new(
                SnowConfig { width: p.width, height: p.height, reduce_motion: false, origin: p.frame.min(), seed: 7 },
                None,
            ))
        }),
        ("rain", |p| Box::new(RainField::new(p.width, p.height, false, 11))),
        ("sakura", |p| Box::new(PetalField::new(PetalKind::Sakura, p.width, p.height, false, 13))),
        ("leaves", |p| Box::new(PetalField::new(PetalKind::Leaves, p.width, p.height, false, 17))),
        ("fireflies", |p| Box::new(FireflyField::new(p.width, p.height, false, 19))),
        ("starfall", |p| Box::new(StarfallField::new(p.width, p.height, false, 23))),
    ];

    let mut ok = true;
    for (name, make) in effects {
        if wanted.as_ref().is_some_and(|w| !w.iter().any(|n| n == name)) {
            continue;
        }
        let factory: EffectFactory = Arc::new(make);
        host.set_effect(Some(factory));
        host.set_wind(Wind::GentleRight);
        let before = host.frames();
        if name == "snow" {
            // Behind-windows hand-over, then back to the front.
            pump(Duration::from_secs_f64(secs / 3.0), &host);
            host.set_behind_windows(true);
            pump(Duration::from_secs_f64(secs / 3.0), &host);
            host.set_behind_windows(false);
            pump(Duration::from_secs_f64(secs / 3.0), &host);
        } else {
            pump(Duration::from_secs_f64(secs), &host);
        }
        let frames = host.frames() - before;
        println!("{name}: frames {frames}");
        ok &= frames > 0;
    }

    // Density and rebuild.
    host.set_share(0.5);
    host.rebuild(&mons).expect("rebuild");
    pump(Duration::from_secs_f64(secs / 2.0), &host);
    println!("rebuilt: panels {} frames {} rescaled {}", host.panels(), host.frames(), host.rescaled());
    ok &= host.frames() > 0;
    drop(host);
    if !ok {
        eprintln!("smoke: some effect rendered no frames");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("overlay_smoke needs Windows");
}
