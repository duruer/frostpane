//! Smoke run: opens one window per z band, moves them, polls hit-testing and
//! exits by itself: `cargo run -p fp-layered --example layered_smoke -- --seconds 3`.

use fp_core::desktop::{Layout, MonitorInfo};
use fp_core::geom::{Frame, PixelRect};
use fp_core::scene::{HitMode, Owner, Sprite, SpriteKey, ZBand};
use fp_layered::LayeredHost;
use fp_paint::tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use std::time::{Duration, Instant};

fn ball(scale: f32, r: f32, c: Color) -> Option<Pixmap> {
    let size = (2.0 * r * scale).ceil() as u32;
    let mut pm = Pixmap::new(size, size)?;
    let mut paint = Paint::default();
    paint.set_color(c);
    paint.anti_alias = true;
    let h = size as f32 / 2.0;
    let path = PathBuilder::from_circle(h, h, h - 1.0)?;
    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    Some(pm)
}

#[cfg(windows)]
fn cursor() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    // SAFETY: valid out pointer.
    unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
}

#[cfg(not(windows))]
fn cursor() -> Option<(i32, i32)> {
    None
}

fn main() {
    let secs: u64 = std::env::args().skip_while(|a| a != "--seconds").nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let (w, h) = (1920, 1080);
    let layout = Layout::new(vec![MonitorInfo {
        id: "smoke".into(),
        name: "smoke".into(),
        pixels: PixelRect::new(0, 0, w, h),
        work: PixelRect::new(0, 0, w, h - 48),
        scale: 1.0,
        primary: true,
    }]);
    let mut host = LayeredHost::new();
    let start = Instant::now();
    let end = start + Duration::from_secs(secs);
    let mut hits = 0u32;
    let mut frames = 0u32;
    while Instant::now() < end {
        let t = start.elapsed().as_secs_f64();
        let mk = |id: u64, z: ZBand, hit: HitMode, x: f64, y: f64| Sprite {
            key: SpriteKey::new(Owner::Ball, id),
            frame: Frame::new(x, y, 40.0, 40.0),
            z,
            hit,
            rev: 1,
            opacity: 1.0,
        };
        let sprites = [
            mk(1, ZBand::Front, HitMode::Alpha, 400.0 + 200.0 * t.sin(), 500.0),
            mk(2, ZBand::UnderDock, HitMode::Never, 800.0, 60.0),
            mk(3, ZBand::Desktop, HitMode::Always, 1000.0, 300.0),
        ];
        host.sync(&sprites, &layout, &mut |k, s| {
            let c = match k.id() {
                1 => Color::from_rgba8(240, 245, 255, 255),
                2 => Color::from_rgba8(200, 120, 120, 255),
                _ => Color::from_rgba8(120, 200, 140, 255),
            };
            ball(s, 20.0, c)
        });
        if host.hit(cursor()).is_some() {
            hits += 1;
        }
        host.pump_messages();
        frames += 1;
        std::thread::sleep(Duration::from_millis(16));
    }
    println!("windows {} frames {frames} hit frames {hits}", host.len());
    host.clear();
    println!("windows after clear {}", host.len());
}
