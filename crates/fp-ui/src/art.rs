//! Textures and the small procedural pictures the windows show (own art:
//! effect tiles, language badges). Painting is done with `fp-paint`; this
//! module only converts the pixmaps into egui textures and caches them.

use eframe::egui::{self, ColorImage, Context, TextureHandle, TextureOptions};
use fp_core::effect::EffectKind;
use fp_paint::tiny_skia::Pixmap;
use fp_paint::{IconName, Sketch, Trace, hex, icon_rgba, rgba};
use std::collections::{HashMap, HashSet};

/// Pre-rendered app icon PNGs (packaging/icons/frostpane.svg), by side in px.
const APP_ICONS: [(u32, &[u8]); 4] = [
    (64, include_bytes!("../../../packaging/icons/png/frostpane-64.png")),
    (128, include_bytes!("../../../packaging/icons/png/frostpane-128.png")),
    (256, include_bytes!("../../../packaging/icons/png/frostpane-256.png")),
    (512, include_bytes!("../../../packaging/icons/png/frostpane-512.png")),
];

/// The pre-rendered app icon that suits `px` device pixels: the smallest one
/// that is at least that big, else the biggest. Returns (side, RGBA image).
pub fn app_icon_image(px: u32) -> Option<(u32, ColorImage)> {
    let (side, bytes) = APP_ICONS.iter().copied().find(|(s, _)| *s >= px).unwrap_or(APP_ICONS[APP_ICONS.len() - 1]);
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.to_rgba8();
    rgba_image(img.width(), img.height(), img.as_raw()).map(|i| (side, i))
}

/// The window/taskbar icon (256 px).
pub fn app_icon_data() -> Option<egui::IconData> {
    let img = image::load_from_memory_with_format(APP_ICONS[2].1, image::ImageFormat::Png).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// A `tiny-skia` pixmap (premultiplied) as an egui image.
pub fn pixmap_image(p: &Pixmap) -> ColorImage {
    ColorImage::from_rgba_premultiplied([p.width() as usize, p.height() as usize], p.data())
}

/// Straight-alpha RGBA bytes as an egui image. `None` when the size is empty
/// or the byte count is not `w * h * 4` (bad host decoder output would
/// otherwise panic inside egui).
pub fn rgba_image(w: u32, h: u32, bytes: &[u8]) -> Option<ColorImage> {
    let need = (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
    if need == 0 || bytes.len() != need {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], bytes))
}

/// Texture cache keyed by name. Textures live as long as the cache.
#[derive(Default)]
pub struct Textures {
    map: HashMap<String, TextureHandle>,
    /// Keys whose `make` failed; not retried every frame (a broken wallpaper
    /// file would otherwise be read and decoded at frame rate).
    failed: HashSet<String>,
}

impl Textures {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.failed.clear();
    }

    /// Let failed keys be tried again (after a rescan or a new decoder).
    pub fn forget_failures(&mut self) {
        self.failed.clear();
    }

    /// Cached texture for `key`, made by `make` the first time.
    pub fn get(
        &mut self,
        ctx: &Context,
        key: &str,
        make: impl FnOnce() -> Option<ColorImage>,
    ) -> Option<TextureHandle> {
        if let Some(t) = self.map.get(key) {
            return Some(t.clone());
        }
        if self.failed.contains(key) {
            return None;
        }
        let Some(img) = make() else {
            self.failed.insert(key.to_string());
            return None;
        };
        let t = ctx.load_texture(key, img, TextureOptions::LINEAR);
        self.map.insert(key.to_string(), t.clone());
        Some(t)
    }

    /// A menu/UI icon at `px` pixels, tinted `color` (0xRRGGBB).
    pub fn icon(&mut self, ctx: &Context, name: IconName, color: u32, px: u32) -> Option<TextureHandle> {
        let key = format!("icon:{name:?}:{color:06x}:{px}");
        self.get(ctx, &key, || rgba_image(px, px, &icon_rgba(name, color, px)))
    }

    /// The app icon for a square `side` points wide, at the context's density.
    pub fn app_icon(&mut self, ctx: &Context, side: f32) -> Option<TextureHandle> {
        let px = (side * ctx.pixels_per_point()).ceil() as u32;
        let chosen = APP_ICONS.iter().map(|(s, _)| *s).find(|s| *s >= px).unwrap_or(512);
        self.get(ctx, &format!("app_icon:{chosen}"), || app_icon_image(px).map(|(_, i)| i))
    }

    /// A pixmap made by `make` at the context's pixel density.
    pub fn pixmap(
        &mut self,
        ctx: &Context,
        key: &str,
        make: impl FnOnce(f32) -> Option<Pixmap>,
    ) -> Option<TextureHandle> {
        let scale = ctx.pixels_per_point();
        let full = format!("{key}@{scale:.2}");
        self.get(ctx, &full, || make(scale).map(|p| pixmap_image(&p)))
    }
}

/// One texture updated in place (animated art such as the ground critters).
#[derive(Default)]
pub struct Animated {
    handle: Option<TextureHandle>,
    last: Option<i64>,
}

impl Animated {
    /// The texture, repainted by `make` only when `frame` changed.
    pub fn update(
        &mut self,
        ctx: &Context,
        name: &str,
        frame: i64,
        make: impl FnOnce() -> Option<ColorImage>,
    ) -> Option<TextureHandle> {
        if self.last != Some(frame)
            && let Some(img) = make()
        {
            match &mut self.handle {
                Some(h) => h.set(img, TextureOptions::LINEAR),
                None => self.handle = Some(ctx.load_texture(name, img, TextureOptions::LINEAR)),
            }
            self.last = Some(frame);
        }
        self.handle.clone()
    }
}

/// Effect tile picture colours: sky top and bottom.
pub fn tile_sky(e: EffectKind) -> (u32, u32) {
    match e {
        EffectKind::Snow => (0x7FA8E8, 0xDCEBFF),
        EffectKind::Rain => (0x34425F, 0x7B8FB0),
        EffectKind::Sakura => (0xFFD6E5, 0xF5A3C0),
        EffectKind::Leaves => (0xF2B66B, 0xB5542A),
        EffectKind::Fireflies => (0x0B1B2E, 0x1D3B33),
        EffectKind::Starfall => (0x090B2B, 0x2A1B5E),
    }
}

fn rect_path(w: f32, h: f32) -> Option<fp_paint::tiny_skia::Path> {
    let mut t = Trace::new();
    t.move_to(0.0, 0.0).line(w, 0.0).line(w, h).line(0.0, h).close();
    t.finish()
}

/// Deterministic pseudo-random in 0..1 (same hash idea as the snow dots).
fn noise(i: usize, k: u32) -> f32 {
    let v = (((13 * i) as f64) + f64::from(k)) * 12.9898;
    let s = v.sin() * 43758.5453;
    (s - s.floor()) as f32
}

/// Own picture for an effect tile (used when no wallpaper file exists):
/// a sky gradient with a few marks of the effect. `w` x `h` points.
pub fn tile_art(e: EffectKind, w: f32, h: f32, scale: f32) -> Option<Pixmap> {
    let (pw, ph) = ((w * scale).ceil().max(1.0) as u32, (h * scale).ceil().max(1.0) as u32);
    let mut pm = Pixmap::new(pw, ph)?;
    {
        let mut s = Sketch::new(&mut pm, scale);
        let (top, bottom) = tile_sky(e);
        if let Some(p) = rect_path(w, h) {
            s.vertical(&p, hex(top), hex(bottom));
        }
        for i in 0..28 {
            let x = noise(i, 1) * w;
            let y = noise(i, 2) * h;
            let r = 0.6 + noise(i, 3) * 1.8;
            match e {
                EffectKind::Snow => s.circle(x, y, r + 0.6, rgba(1.0, 1.0, 1.0, 0.9)),
                EffectKind::Rain => {
                    s.line(x, y, x - 3.0, y - 9.0, rgba(0.82, 0.9, 1.0, 0.6), 1.0);
                }
                EffectKind::Sakura => s.ellipse(x, y, 5.0 + r, 3.0 + r * 0.5, rgba(1.0, 0.96, 0.98, 0.85)),
                EffectKind::Leaves => s.ellipse(x, y, 6.0 + r, 3.5 + r * 0.5, rgba(0.95, 0.55, 0.15, 0.9)),
                EffectKind::Fireflies => s.halo(x, y, 4.0 + r * 2.0, rgba(0.96, 0.86, 0.3, 0.9)),
                EffectKind::Starfall => {
                    if i % 5 == 0 {
                        s.line(x, y, x - 14.0, y + 9.0, rgba(1.0, 1.0, 1.0, 0.8), 1.2);
                    }
                    s.circle(x, y, r * 0.6, rgba(1.0, 1.0, 1.0, 0.9));
                }
            }
        }
    }
    Some(pm)
}

/// Round language badge (own neutral icon): the language code on a disc.
/// Returns the disc colour for `lang` (0xRRGGBB), distinct per language.
pub fn lang_color(lang: fp_core::effect::Lang) -> u32 {
    use fp_core::effect::Lang;
    match lang {
        Lang::En => 0x3B6FD6,
        Lang::Ja => 0xD64B5C,
        Lang::Es => 0xE08A2E,
        Lang::De => 0xC9A227,
        Lang::Fr => 0x4C8FD6,
        Lang::Ko => 0x2FA37A,
        Lang::Ru => 0x8A5CD6,
        Lang::Tr => 0x6A9A2E,
        Lang::Kk => 0x1E9BB8,
    }
}

/// Paint a vertical gradient into `rect` as a mesh (square corners).
pub fn gradient(painter: &egui::Painter, rect: egui::Rect, top: egui::Color32, bottom: egui::Color32) {
    let mut mesh = egui::Mesh::default();
    let uv = egui::pos2(0.0, 0.0);
    let white_uv = |p: egui::Pos2, c: egui::Color32| egui::epaint::Vertex { pos: p, uv, color: c };
    mesh.vertices.push(white_uv(rect.left_top(), top));
    mesh.vertices.push(white_uv(rect.right_top(), top));
    mesh.vertices.push(white_uv(rect.right_bottom(), bottom));
    mesh.vertices.push(white_uv(rect.left_bottom(), bottom));
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_art_paints_every_effect() {
        for e in EffectKind::ALL {
            let pm = tile_art(e, 160.0, 100.0, 1.0).unwrap();
            assert_eq!((pm.width(), pm.height()), (160, 100));
            // The sky gradient fills the whole picture.
            assert!(pm.pixels().iter().all(|p| p.alpha() == 255));
            let img = pixmap_image(&pm);
            assert_eq!(img.size, [160, 100]);
        }
        assert!(tile_art(EffectKind::Snow, 0.0, 0.0, 1.0).is_some());
    }

    #[test]
    fn app_icon_sizes_and_window_icon() {
        // Smallest embedded size that is big enough, else the biggest.
        for (px, want) in [(1, 64), (64, 64), (65, 128), (200, 256), (300, 512), (5000, 512)] {
            let (side, img) = app_icon_image(px).unwrap();
            assert_eq!(side, want);
            assert_eq!(img.size, [want as usize; 2]);
        }
        let icon = app_icon_data().unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        // The tile corner is transparent, the middle is not.
        assert_eq!(icon.rgba[3], 0);
        assert_eq!(icon.rgba[(128 * 256 + 128) * 4 + 3], 255);
    }

    #[test]
    fn rgba_images_and_colours() {
        let img = rgba_image(1, 1, &[10, 20, 30, 255]).unwrap();
        assert_eq!(img.pixels.len(), 1);
        // Wrong byte counts and empty sizes are refused, not a panic.
        assert!(rgba_image(2, 2, &[0; 15]).is_none());
        assert!(rgba_image(0, 3, &[]).is_none());
        assert!(rgba_image(u32::MAX, u32::MAX, &[]).is_none());
        for l in fp_core::effect::Lang::ALL {
            let _ = lang_color(l);
        }
        let colours: std::collections::HashSet<_> = fp_core::effect::Lang::ALL.iter().map(|l| lang_color(*l)).collect();
        assert_eq!(colours.len(), 9);
        let (a, b) = tile_sky(EffectKind::Snow);
        assert_ne!(a, b);
        assert!(noise(3, 1) >= 0.0 && noise(3, 1) < 1.0);
    }

    #[test]
    fn textures_cache_by_key() {
        let ctx = Context::default();
        let mut t = Textures::new();
        assert!(t.is_empty());
        let a = t.get(&ctx, "k", || rgba_image(1, 1, &[0, 0, 0, 255]));
        assert!(a.is_some());
        // Second call must not run `make`.
        let b = t.get(&ctx, "k", || panic!("cached"));
        assert!(b.is_some());
        assert_eq!(t.len(), 1);
        assert!(t.get(&ctx, "none", || None).is_none());
        // A failed key is remembered and `make` is not run again.
        assert!(t.get(&ctx, "none", || panic!("retried")).is_none());
        t.forget_failures();
        assert!(t.get(&ctx, "none", || rgba_image(1, 1, &[1, 2, 3, 4])).is_some());
        let icon = t.icon(&ctx, IconName::Snow, 0xFFFFFF, 24);
        assert!(icon.is_some());
        let pm = t.pixmap(&ctx, "p", |s| tile_art(EffectKind::Rain, 10.0, 10.0, s));
        assert!(pm.is_some());
        t.clear();
        assert!(t.is_empty());
    }
}
