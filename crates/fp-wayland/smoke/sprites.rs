//! Sprite surface smoke: one 64x64 red disc as a layer surface, shown at
//! (300, 300), moved to (700, 200), faded to half alpha, then hidden. After
//! each step it prints the step name and waits for a line on stdin, so
//! `smoke/smoke-wayland.sh` can take a screenshot in between.

#[cfg(target_os = "linux")]
fn main() {
    use fp_platform::select::WindowSource;
    use fp_platform::sprites::Place;
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    let mut parts = fp_wayland::backend(WindowSource::None);
    let sp = &mut parts.sprites;
    let mut pm = tiny_skia::Pixmap::new(64, 64).expect("pixmap");
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 0, 0, 255);
    let disc = tiny_skia::PathBuilder::from_circle(32.0, 32.0, 30.0).expect("circle");
    pm.fill_path(&disc, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    let id = sp.create().expect("sprite");
    sp.place(id, Place::Topmost);
    assert!(sp.set_bitmap(id, &pm));
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut step = |name: &str, sp: &mut Box<dyn fp_platform::sprites::SpriteBackend>| {
        let end = Instant::now() + Duration::from_millis(500);
        while Instant::now() < end {
            sp.pump();
            std::thread::sleep(Duration::from_millis(16));
        }
        println!("{name}");
        let _ = lines.next();
    };
    sp.present(id, 300, 300, 255);
    step("shown", sp);
    sp.present(id, 700, 200, 255);
    step("moved", sp);
    sp.present(id, 700, 200, 128);
    step("faded", sp);
    sp.set_click_through(id, false);
    sp.hide(id);
    step("hidden", sp);
    sp.destroy(id);
    sp.pump();
}

#[cfg(not(target_os = "linux"))]
fn main() {}
