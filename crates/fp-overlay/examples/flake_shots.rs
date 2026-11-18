//! Falling-flake look check (headless, no windows): renders a still of the
//! falling snow, a zoomed crop, a specimen sheet of single flakes over time
//! (rows = time, to spot tumble flicker) and times the flake pass.
//! `cargo run --release -p fp-overlay --example flake_shots -- <out dir> <tag>`

use fp_core::Vec2;
use fp_core::falling::Falling;
use fp_core::gpu::{DrawList, Layer, PassKind, SnowFlake};
use fp_overlay::gpu::Slots;
use fp_overlay::{Gpu, Image};
use fp_snow::{SnowConfig, SnowField};
use std::path::Path;
use std::time::Instant;

const W: u32 = 1200;
const H: u32 = 700;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(args.get(1).expect("usage: flake_shots <out dir> <tag>"));
    let tag = args.get(2).map_or("shot", String::as_str);
    std::fs::create_dir_all(&dir).expect("out dir");
    let gpu = Gpu::headless().expect("gpu");

    // A settled field after a few seconds, flakes only.
    let (field, list) = field_list(W, H, 11, 40.0);
    let flakes = flakes_of(&list);
    let img = gpu.render_image(W, H, &list);
    let rgba = composite(&img);
    let mass: f64 = img.bgra.as_chunks::<4>().0.iter().map(|p| f64::from(p[3]) / 255.0).sum();
    println!("field: {} flakes, alpha mass {mass:.1} px", list.passes[0].instances);
    save(&dir.join(format!("{tag}-field.png")), W, H, &rgba);
    let (cx, cy, cw, ch) = (60, 120, 400, 233);
    let crop = zoom(&rgba, W, cx, cy, cw, ch, 3);
    save(&dir.join(format!("{tag}-field-zoom3.png")), cw * 3, ch * 3, &crop);
    // Consecutive frames of the same crop (1/60 s apart) to compare by eye.
    for k in 1..4 {
        let mut l = DrawList::new(W as f32, H as f32, 0.0);
        let mut u = list.passes[0].uniforms.clone();
        u[0] += k as f32 / 60.0;
        l.instanced(PassKind::SnowFlakes, u, &flakes);
        let rgba = composite(&gpu.render_image(W, H, &l));
        let crop = zoom(&rgba, W, cx, cy, cw, ch, 3);
        save(&dir.join(format!("{tag}-field-zoom3-f{k}.png")), cw * 3, ch * 3, &crop);
    }
    drop(field);

    // Specimen sheets: per shape kind (speck, clump, crystal, bokeh; the
    // kind is the integer part of `shape`), flakes spread over that kind's
    // size range, held still (no fall, sway or wind) and drawn at 8 moments
    // 0.2 s apart (rows), at display scale 1 and 2. Before the shape kinds
    // existed every flake reads as kind 0, so the sheet is size-sampled.
    let mut sorted = flakes.clone();
    sorted.sort_by(|a, b| a.size.total_cmp(&b.size));
    let mut picks = Vec::new();
    for (kind, n) in [(0.0f32, 4usize), (1.0, 5), (2.0, 5), (3.0, 2)] {
        let of: Vec<SnowFlake> = sorted.iter().filter(|f| kind_of(f) == kind).copied().collect();
        let n = if kind == 0.0 && of.len() == sorted.len() { 16 } else { n };
        for c in 0..n.min(of.len()) {
            picks.push(of[(c * (of.len() - 1)) / (n - 1).max(1)]);
        }
    }
    let (cols, rows, cell) = (picks.len(), 8usize, 20.0f32);
    let (sw, sh) = (cols as f32 * cell, rows as f32 * cell);
    let mut sheet = Vec::new();
    for (c, &f) in picks.iter().enumerate() {
        for r in 0..rows {
            sheet.push(SnowFlake {
                x: (c as f32 + 0.5) * cell,
                top: sh - (r as f32 + 0.5) * cell,
                born: -(r as f32) * 0.2,
                fall: 0.0,
                drift: 0.0,
                sway: 0.0,
                gone: 1e9,
                ..f
            });
        }
    }
    for scale in [1u32, 2] {
        let mut l = DrawList::new(sw, sh, 0.0);
        l.scene[3] = scale as f32;
        l.instanced(PassKind::SnowFlakes, vec![0.0, 0.0, 0.0, sw + 60.0, 30.0], &sheet);
        let (iw, ih) = (sw as u32 * scale, sh as u32 * scale);
        let rgba = composite(&gpu.render_image(iw, ih, &l));
        let k = 8 / scale;
        save(&dir.join(format!("{tag}-sheet-x{scale}-zoom{k}.png")), iw * k, ih * k, &zoom(&rgba, iw, 0, 0, iw, ih, k));
    }

    // Flicker: each specimen over 120 consecutive 60 fps frames (one cell
    // per frame); the largest frame-to-frame change of its alpha mass.
    let frames = 120usize;
    let mut cells = Vec::new();
    for (c, &f) in picks.iter().enumerate() {
        for k in 0..frames {
            cells.push(SnowFlake {
                x: (c as f32 + 0.5) * cell,
                top: frames as f32 * cell - (k as f32 + 0.5) * cell,
                born: -(k as f32) / 60.0,
                fall: 0.0,
                drift: 0.0,
                sway: 0.0,
                gone: 1e9,
                ..f
            });
        }
    }
    let (fw, fh) = (sw, frames as f32 * cell);
    let mut l = DrawList::new(fw, fh, 0.0);
    l.instanced(PassKind::SnowFlakes, vec![0.0, 0.0, 0.0, fw + 60.0, 30.0], &cells);
    let img = gpu.render_image(fw as u32, fh as u32, &l);
    let c = cell as u32;
    let mut worst = 0.0f64;
    for col in 0..picks.len() as u32 {
        let mass = |k: u32| -> f64 {
            let mut m = 0.0;
            for y in k * c..(k + 1) * c {
                for x in col * c..(col + 1) * c {
                    m += f64::from(img.pixel(x, y)[3]);
                }
            }
            m
        };
        let ms: Vec<f64> = (0..frames as u32).map(mass).collect();
        let peak = ms.iter().copied().fold(0.0, f64::max).max(1.0);
        for w in ms.windows(2) {
            worst = worst.max((w[1] - w[0]).abs() / peak);
        }
    }
    println!("flicker: worst frame-to-frame alpha-mass change {:.2}% of a flake", worst * 100.0);

    // Timing: 6000 and 60000 flakes over a 2560x1440 target (300 frames
    // encoded and submitted back to back, then waited on, against an empty
    // list), and the CPU draw() cost.
    let (pw, ph) = (2560u32, 1440u32);
    let (big, plist) = field_list(pw, ph, 5, 4.0);
    let base = flakes_of(&plist);
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("timing"),
        size: wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: fp_overlay::gpu::FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let mut slots = Slots::new(&gpu);
    let mut time = |list: &DrawList| {
        let n = 300;
        let mut run = |n: usize| {
            for _ in 0..n {
                gpu.render(&mut slots, &view, Some(list));
            }
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        };
        run(20);
        let t = Instant::now();
        run(n);
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    };
    let empty = DrawList::new(pw as f32, ph as f32, 0.0);
    let e = time(&empty);
    for count in [6000usize, 60000] {
        let mut many: Vec<SnowFlake> = Vec::with_capacity(count);
        while many.len() < count {
            many.extend(base.iter().take(count - many.len()));
        }
        let mut l = DrawList::new(pw as f32, ph as f32, 0.0);
        l.instanced(PassKind::SnowFlakes, plist.passes[0].uniforms.clone(), &many);
        let f = time(&l);
        println!("frame 2560x1440: empty {e:.3} ms, {count} flakes {f:.3} ms (+{:.3} ms)", f - e);
    }
    let t = Instant::now();
    let m = 600;
    for _ in 0..m {
        let mut out = DrawList::new(pw as f32, ph as f32, 0.0);
        big.draw(Layer::Main, &mut out);
    }
    let flakes_n = plist.passes.iter().find(|p| p.kind == PassKind::SnowFlakes).map_or(0, |p| p.instances);
    println!(
        "cpu draw() {flakes_n} flakes: {:.1} us/frame, flake stride {} B",
        t.elapsed().as_secs_f64() * 1e6 / m as f64,
        size_of::<SnowFlake>()
    );
}

/// A snow field stepped for `secs`, and a list holding only its falling flakes.
fn field_list(w: u32, h: u32, seed: u64, secs: f64) -> (SnowField, DrawList) {
    let mut field = SnowField::new(
        SnowConfig { width: w as f64, height: h as f64, reduce_motion: false, origin: Vec2::ZERO, seed },
        None,
    );
    field.show_ground(false);
    for _ in 0..(secs * 60.0) as usize {
        field.step(1.0 / 60.0, None);
    }
    let mut all = DrawList::new(w as f32, h as f32, secs as f32);
    field.draw(Layer::Main, &mut all);
    let mut list = DrawList::new(w as f32, h as f32, secs as f32);
    list.passes = all.passes.into_iter().filter(|p| p.kind == PassKind::SnowFlakes).collect();
    (field, list)
}

/// Shape kind of a flake (0 when the layout has no `shape` field yet, so the
/// example also builds against the older 52-byte layout).
fn kind_of(f: &SnowFlake) -> f32 {
    let b = bytemuck::bytes_of(f);
    b.get(52..56).map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]).floor())
}

fn flakes_of(list: &DrawList) -> Vec<SnowFlake> {
    bytemuck::pod_collect_to_vec(&list.passes[0].buffers[0])
}

/// Premultiplied BGRA over a dark blue gradient, to RGBA.
fn composite(img: &Image) -> Vec<u8> {
    let mut px = Vec::with_capacity(img.bgra.len());
    for (i, p) in img.bgra.as_chunks::<4>().0.iter().enumerate() {
        let y = (i as u32 / img.width) as f32 / img.height as f32;
        let bg = [0.05 + 0.06 * y, 0.08 + 0.08 * y, 0.18 + 0.12 * y];
        let a = f32::from(p[3]) / 255.0;
        let ch = |src: u8, bg: f32| ((f32::from(src) / 255.0 + bg * (1.0 - a)).clamp(0.0, 1.0) * 255.0) as u8;
        px.extend_from_slice(&[ch(p[2], bg[0]), ch(p[1], bg[1]), ch(p[0], bg[2]), 255]);
    }
    px
}

/// Nearest-neighbour `k`x zoom of a crop of an RGBA image `w` wide.
fn zoom(rgba: &[u8], w: u32, x0: u32, y0: u32, cw: u32, ch: u32, k: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((cw * ch * k * k * 4) as usize);
    for y in 0..ch * k {
        for x in 0..cw * k {
            let i = (((y0 + y / k) * w + x0 + x / k) * 4) as usize;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    out
}

fn save(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let mut pm = tiny_skia::Pixmap::new(w, h).expect("pixmap");
    pm.data_mut().copy_from_slice(rgba);
    pm.save_png(path).expect("save png");
    println!("{}", path.display());
}
