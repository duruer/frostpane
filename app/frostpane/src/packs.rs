//! Generated wallpaper packs (ui 3.2: the rewrite ships its own art).
//!
//! A pack is `<id>-<slot>.jpg` for the 8 day slots. At startup every pack
//! whose midday still is missing gets drawn procedurally (sky tinted per
//! slot, a ground silhouette and the effect's marks) and written as JPEG.
//! Existing files are never touched, so user or later shipped art wins.

use fp_core::effect::EffectKind;
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{Sketch, Trace, hex, rgba};
use fp_sources::daynight::{self, Slot};
use fp_ui::wallpapers::ORDER;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

/// Size of the generated stills.
pub const WIDTH: u32 = 1920;
pub const HEIGHT: u32 = 1200;

/// `(brightness, warm mix, warm colour)` per slot.
fn slot_tone(slot: Slot) -> (f32, f32, u32) {
    match slot {
        Slot::Dawn => (0.62, 0.35, 0xE79BB8),
        Slot::Sunrise => (0.85, 0.40, 0xFFB070),
        Slot::Morning => (1.0, 0.08, 0xFFE3B0),
        Slot::Midday => (1.0, 0.0, 0xFFFFFF),
        Slot::Golden => (0.95, 0.30, 0xFFC060),
        Slot::Sunset => (0.78, 0.45, 0xF2704A),
        Slot::Dusk => (0.5, 0.35, 0x8A5CB0),
        Slot::Night => (0.34, 0.20, 0x203060),
    }
}

fn tint(color: u32, slot: Slot) -> u32 {
    let (bright, mix, warm) = slot_tone(slot);
    let ch = |shift: u32| {
        let base = ((color >> shift) & 0xFF) as f32;
        let w = ((warm >> shift) & 0xFF) as f32;
        ((base * (1.0 - mix) + w * mix) * bright).clamp(0.0, 255.0) as u32
    };
    (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// Deterministic pseudo-random in 0..1.
fn noise(i: usize, k: u32) -> f32 {
    let v = (((13 * i) as f64) + f64::from(k)) * 12.9898;
    let s = v.sin() * 43758.5453;
    (s - s.floor()) as f32
}

/// Draw one still of `effect` for `slot`, `w` x `h` pixels.
pub fn render(effect: EffectKind, slot: Slot, w: u32, h: u32) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    let (wf, hf) = (w as f32, h as f32);
    let unit = hf / 1200.0;
    {
        let mut s = Sketch::new(&mut pm, 1.0);
        let (top, bottom) = fp_ui::art::tile_sky(effect);
        let (top, bottom) = (tint(top, slot), tint(bottom, slot));
        let mut t = Trace::new();
        t.move_to(0.0, 0.0).line(wf, 0.0).line(wf, hf).line(0.0, hf).close();
        if let Some(p) = t.finish() {
            s.vertical(&p, hex(top), hex(bottom));
        }
        // Ground silhouette: two rolling hills in a darker shade of the sky.
        for (layer, base, shade) in [(0usize, 0.22f32, 0.55f32), (1, 0.14, 0.35)] {
            let mut t = Trace::new();
            t.move_to(0.0, 0.0);
            for step in 0..=24 {
                let x = wf * step as f32 / 24.0;
                let y = hf * base + hf * 0.05 * (noise(step + 40 * layer, 7) - 0.5) * 2.0;
                t.line(x, y);
            }
            t.line(wf, 0.0).close();
            if let Some(p) = t.finish() {
                let dark = |sh: u32| ((((bottom >> sh) & 0xFF) as f32) * shade) as u32;
                s.fill(&p, hex((dark(16) << 16) | (dark(8) << 8) | dark(0)));
            }
        }
        let night = matches!(slot, Slot::Night | Slot::Dusk);
        for i in 0..520 {
            let x = noise(i, 1) * wf;
            let y = noise(i, 2) * hf;
            let r = (1.0 + noise(i, 3) * 3.0) * unit;
            match effect {
                EffectKind::Snow => s.circle(x, y, r * 1.6, rgba(1.0, 1.0, 1.0, 0.85)),
                EffectKind::Rain => {
                    s.line(x, y, x - 8.0 * unit, y - 30.0 * unit, rgba(0.82, 0.9, 1.0, 0.45), unit * 1.5);
                }
                EffectKind::Sakura => {
                    s.ellipse(x, y, (8.0 + r * 2.0) * unit, (5.0 + r) * unit, rgba(1.0, 0.9, 0.94, 0.85));
                }
                EffectKind::Leaves => {
                    s.ellipse(x, y, (9.0 + r * 2.0) * unit, (5.0 + r) * unit, rgba(0.95, 0.5, 0.15, 0.9));
                }
                EffectKind::Fireflies => {
                    let a = if night { 0.9 } else { 0.5 };
                    s.halo(x, y, (6.0 + r * 4.0) * unit, rgba(0.96, 0.86, 0.3, a));
                }
                EffectKind::Starfall => {
                    if i % 9 == 0 {
                        s.line(x, y, x - 40.0 * unit, y + 26.0 * unit, rgba(1.0, 1.0, 1.0, 0.7), unit * 2.0);
                    }
                    s.circle(x, y, r * 0.8, rgba(1.0, 1.0, 1.0, 0.9));
                }
            }
        }
    }
    Some(pm)
}

/// Encode a pixmap (opaque) as JPEG.
pub fn encode(pm: &Pixmap) -> Option<Vec<u8>> {
    let rgb: Vec<u8> = pm.data().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    let enc = jpeg_encoder::Encoder::new(&mut out, 88);
    enc.encode(&rgb, u16::try_from(pm.width()).ok()?, u16::try_from(pm.height()).ok()?, jpeg_encoder::ColorType::Rgb)
        .ok()?;
    Some(out)
}

/// Write `bytes` to `path` through a temp file so a half file never shows.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("jpg.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Packs whose midday still is missing from `dir`.
pub fn missing(dir: &Path) -> Vec<EffectKind> {
    ORDER.iter().copied().filter(|e| !dir.join(daynight::file_name(e.wallpaper_id(), Slot::Midday)).is_file()).collect()
}

/// Draw every missing pack at `w` x `h`; returns the number of files written.
/// Files that already exist stay as they are.
pub fn ensure_packs_sized(dir: &Path, w: u32, h: u32) -> usize {
    let todo = missing(dir);
    if todo.is_empty() || std::fs::create_dir_all(dir).is_err() {
        return 0;
    }
    let mut written = 0;
    for effect in todo {
        for slot in Slot::ALL {
            let path = dir.join(daynight::file_name(effect.wallpaper_id(), slot));
            if path.exists() {
                continue;
            }
            let ok =
                render(effect, slot, w, h).and_then(|pm| encode(&pm)).is_some_and(|b| write_atomic(&path, &b).is_ok());
            if ok {
                written += 1;
            } else {
                log::warn!("wallpaper still not written: {}", path.display());
            }
        }
    }
    written
}

/// Draw every missing pack at the full size.
pub fn ensure_packs(dir: &Path) -> usize {
    ensure_packs_sized(dir, WIDTH, HEIGHT)
}

/// Run [`ensure_packs`] on a worker thread. `done` runs on that thread after
/// the files are written (wake the UI); the receiver gets the file count.
/// None when nothing is missing, so no thread starts.
pub fn spawn_ensure(dir: &Path, done: impl FnOnce() + Send + 'static) -> Option<Receiver<usize>> {
    if missing(dir).is_empty() {
        return None;
    }
    let (tx, rx) = channel();
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        let n = ensure_packs(&dir);
        let _ = tx.send(n);
        done();
    });
    Some(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_ui::wallpapers::scan;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fp-packs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn empty_dir_gives_six_daylong_papers() {
        let d = tmp("empty");
        assert_eq!(ensure_packs_sized(&d, 64, 40), 48);
        let papers = scan(|f| d.join(f).is_file());
        assert_eq!(papers.len(), 6);
        assert!(papers.iter().all(|p| p.daylong));
        let bytes = std::fs::read(d.join(daynight::file_name(EffectKind::Snow.wallpaper_id(), Slot::Midday))).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
        assert!(!d.join("tmp-never.jpg").exists());
        // second run: nothing missing, nothing written
        assert_eq!(ensure_packs_sized(&d, 64, 40), 0);
        assert_eq!(ensure_packs(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let d = tmp("keep");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(daynight::file_name(EffectKind::Rain.wallpaper_id(), Slot::Dawn)), b"mine").unwrap();
        ensure_packs_sized(&d, 32, 20);
        assert_eq!(
            std::fs::read(d.join(daynight::file_name(EffectKind::Rain.wallpaper_id(), Slot::Dawn))).unwrap(),
            b"mine"
        );
        assert!(d.join(daynight::file_name(EffectKind::Rain.wallpaper_id(), Slot::Midday)).is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn spawn_skips_when_complete() {
        let d = tmp("spawn");
        std::fs::create_dir_all(&d).unwrap();
        for e in ORDER {
            std::fs::write(d.join(daynight::file_name(e.wallpaper_id(), Slot::Midday)), b"x").unwrap();
        }
        assert!(spawn_ensure(&d, || {}).is_none());
        assert!(missing(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tones_differ_per_slot_and_render_is_opaque() {
        assert_ne!(tint(0x7FA8E8, Slot::Night), tint(0x7FA8E8, Slot::Midday));
        for e in ORDER {
            for s in Slot::ALL {
                let pm = render(e, s, 48, 30).unwrap();
                assert!(pm.data().as_chunks::<4>().0.iter().all(|p| p[3] == 255));
            }
        }
        assert!(render(EffectKind::Snow, Slot::Dawn, 0, 0).is_none());
    }
}
