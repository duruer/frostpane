//! Renders every effect offscreen (headless device, no windows) after a few
//! simulated seconds and writes PNGs over a dark backdrop, to eyeball the
//! shaders: `cargo run -p fp-overlay --example render_dump -- <out dir> [seconds]`.

use fp_core::falling::Falling;
use fp_core::gpu::{DrawList, Layer};
use fp_core::surface::{Surface, SurfaceId};
use fp_core::{Frame, Rect4, Vec2};
use fp_overlay::Gpu;
use fp_petals::{PetalField, PetalKind};
use fp_rain::RainField;
use fp_sky::{FireflyField, StarfallField};
use fp_snow::{SnowConfig, SnowField};

const W: u32 = 960;
const H: u32 = 540;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(args.get(1).expect("usage: render_dump <out dir> [seconds]"));
    let secs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(14.0);
    std::fs::create_dir_all(&dir).expect("out dir");
    let gpu = Gpu::headless().expect("gpu");
    let (w, h) = (W as f64, H as f64);

    let mut window = Surface::new(SurfaceId::Window(1), Frame::new(w * 0.3, h * 0.35, w * 0.3, h * 0.3));
    window.blockers.push(Rect4::new(w * 0.5, h * 0.2, w * 0.7, h * 0.45));
    let mut dock = Surface::new(SurfaceId::Dock, Frame::new(0.0, 0.0, w, 40.0));
    dock.owner = "dock".into();

    let effects: Vec<(&str, Box<dyn Falling>)> = vec![
        (
            "snow",
            Box::new(SnowField::new(
                SnowConfig { width: w, height: h, reduce_motion: false, origin: Vec2::ZERO, seed: 7 },
                None,
            )),
        ),
        ("rain", Box::new(RainField::new(w, h, false, 11))),
        ("sakura", Box::new(PetalField::new(PetalKind::Sakura, w, h, false, 13))),
        ("leaves", Box::new(PetalField::new(PetalKind::Leaves, w, h, false, 17))),
        ("fireflies", Box::new(FireflyField::new(w, h, false, 19))),
        ("starfall", Box::new(StarfallField::new(w, h, false, 23))),
    ];
    for (name, mut fall) in effects {
        fall.show_ground(true);
        fall.set_lightning(true);
        fall.update(&[window.clone(), dock.clone()], 0.1, false);
        let steps = (secs * 60.0) as usize;
        for i in 0..steps {
            fall.step(1.0 / 60.0, Some(Vec2::new(100.0, 100.0)));
            if i % 12 == 0 {
                fall.update(&[window.clone(), dock.clone()], 0.2, false);
            }
        }
        let mut list = DrawList::new(W as f32, H as f32, secs as f32);
        fall.draw(Layer::Main, &mut list);
        let img = gpu.render_image(W, H, &list);
        let mut layers = vec![("main", img)];
        if fall.draws_above() {
            let mut above = DrawList::new(W as f32, H as f32, secs as f32);
            fall.draw(Layer::Above, &mut above);
            layers.push(("above", gpu.render_image(W, H, &above)));
        }
        for (tag, img) in layers {
            // Premultiplied BGRA over a dark gradient, to RGBA.
            let mut px = Vec::with_capacity(img.bgra.len());
            for (i, p) in img.bgra.as_chunks::<4>().0.iter().enumerate() {
                let y = (i as u32 / W) as f32 / H as f32;
                let bg = [0.10 + 0.10 * y, 0.14 + 0.12 * y, 0.28 + 0.14 * y];
                let a = f32::from(p[3]) / 255.0;
                let ch = |src: u8, bg: f32| ((f32::from(src) / 255.0 + bg * (1.0 - a)).clamp(0.0, 1.0) * 255.0) as u8;
                px.extend_from_slice(&[ch(p[2], bg[0]), ch(p[1], bg[1]), ch(p[0], bg[2]), 255]);
            }
            let mut pm = tiny_skia::Pixmap::new(W, H).expect("pixmap");
            pm.data_mut().copy_from_slice(&px);
            let path = dir.join(format!("{name}-{tag}.png"));
            pm.save_png(&path).expect("save png");
            println!("{} ({} passes, {} px covered)", path.display(), list.passes.len(), img.covered(0));
        }
    }
}
