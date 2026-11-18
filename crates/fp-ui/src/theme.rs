//! Theme tokens (ui 2.4, core-app 2.9). Ink colours are black or white with
//! the spec's alpha table; Home is always dark, onboarding light.

use std::sync::Arc;

use eframe::egui::{Color32, Context, FontData, FontDefinitions, FontFamily, FontId, FontTweak};
use fp_paint::brand;

/// Alpha byte for an opacity: `round-half-to-even(255 * x)`, clamped.
pub fn alpha_byte(opacity: f64) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8
}

/// `0xRRGGBB` plus an opacity as an egui colour (straight alpha).
pub fn rgb_a(rgb: u32, opacity: f64) -> Color32 {
    Color32::from_rgba_unmultiplied((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha_byte(opacity))
}

/// Opaque `0xRRGGBB`.
pub fn rgb(rgb: u32) -> Color32 {
    rgb_a(rgb, 1.0)
}

pub const ACCENT: u32 = brand::ACCENT;
pub const GREEN: u32 = 0x4CCC73;
pub const ERROR_RED: u32 = 0xFF7366;
pub const WARNING: u32 = 0xFF9400;
pub const GRAY_GLYPH: u32 = 0x8E8E93;
pub const NAVY: u32 = 0x172B54;
pub const BADGE_RED: u32 = 0xCC2133;

/// All colour tokens of one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub dark: bool,
}

impl Tokens {
    pub fn new(dark: bool) -> Self {
        Self { dark }
    }

    /// Ink (white on dark, black on light) at `opacity`.
    pub fn ink_a(&self, opacity: f64) -> Color32 {
        rgb_a(if self.dark { 0xFFFFFF } else { 0x000000 }, opacity)
    }

    pub fn ink(&self) -> Color32 {
        self.ink_a(0.85)
    }
    pub fn ink75(&self) -> Color32 {
        self.ink_a(0.75)
    }
    pub fn secondary(&self) -> Color32 {
        self.ink_a(if self.dark { 0.55 } else { 0.50 })
    }
    pub fn tertiary(&self) -> Color32 {
        self.ink_a(if self.dark { 0.25 } else { 0.26 })
    }
    pub fn divider(&self) -> Color32 {
        self.ink_a(0.06)
    }
    pub fn ink04(&self) -> Color32 {
        self.ink_a(0.04)
    }
    pub fn ink06(&self) -> Color32 {
        self.ink_a(0.06)
    }
    pub fn ink07(&self) -> Color32 {
        self.ink_a(0.07)
    }
    pub fn ink10(&self) -> Color32 {
        self.ink_a(0.10)
    }
    pub fn ink16(&self) -> Color32 {
        self.ink_a(0.16)
    }
    pub fn ink20(&self) -> Color32 {
        self.ink_a(0.20)
    }
    pub fn ink25(&self) -> Color32 {
        self.ink_a(0.25)
    }
    pub fn link(&self) -> Color32 {
        rgb(if self.dark { 0x409CFF } else { 0x0069D9 })
    }
    pub fn menu(&self) -> Color32 {
        rgb(if self.dark { 0x1C263B } else { 0xF7F7FA })
    }
    pub fn window(&self) -> Color32 {
        rgb(if self.dark { 0x0D1733 } else { 0xDBEDFF })
    }
    /// Vertical sky gradient, top then bottom.
    pub fn sky(&self) -> (Color32, Color32) {
        if self.dark { (rgb(0x0D1733), rgb(0x123066)) } else { (rgb(0xF2FAFF), rgb(0xC2DEFC)) }
    }
    pub fn accent(&self) -> Color32 {
        rgb(ACCENT)
    }
}

/// Snow-day gradient of onboarding and the quote card, top then bottom.
pub fn snow_day() -> (Color32, Color32) {
    (rgb(brand::SNOW_DAY.0), rgb(brand::SNOW_DAY.1))
}

/// Name of the bold font family (ui 2.4: headings 32/36 Bold, effect titles 34 Bold).
pub const BOLD_FAMILY: &str = "bold";

/// Font id of a Bold text of `size`.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(BOLD_FAMILY.into()))
}

/// Bold faces tried in order; the first readable one is registered.
const BOLD_FACES: &[&str] = &[
    "C:/Windows/Fonts/segoeuib.ttf",
    "C:/Windows/Fonts/arialbd.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
];

/// CJK bold faces (Japanese, Korean, Chinese), each registered when present
/// so Bold headings do not mix bold Latin with regular-weight kana/hangul.
const CJK_BOLD_FACES: &[(&str, &str)] = &[
    ("bold-ja", "C:/Windows/Fonts/YuGothB.ttc"),
    ("bold-ko", "C:/Windows/Fonts/malgunbd.ttf"),
    ("bold-zh", "C:/Windows/Fonts/msyhbd.ttc"),
    ("bold-cjk", "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc"),
    ("bold-cjk-mac", "/System/Library/Fonts/Hiragino Sans GB.ttc"),
];

fn be16(b: &[u8], at: usize) -> Option<i32> {
    Some(i16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?) as i32)
}

fn be32(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Baseline centre of face 0 in em units: `(ascent + descent - gap) / 2`
/// (descent negative). A mixed-font row puts each face's baseline at
/// `ascent + (row_h - face_row_h) / 2`, so faces whose value differs sit
/// at different heights. Mirrors the font's hhea/typo choice.
fn baseline_centre(b: &[u8]) -> Option<f32> {
    let base = if b.get(0..4)? == b"ttcf" { be32(b, 12)? } else { 0 };
    let n = be16(b, base + 4)? as usize;
    let table = |tag: &[u8; 4]| {
        (0..n).find_map(|i| {
            let e = base + 12 + i * 16;
            (b.get(e..e + 4)? == tag).then(|| be32(b, e + 8)).flatten()
        })
    };
    let head = table(b"head")?;
    let hhea = table(b"hhea")?;
    let upm = be16(b, head + 18)? as f32;
    let (mut a, mut d, mut g) = (be16(b, hhea + 4)?, be16(b, hhea + 6)?, be16(b, hhea + 8)?);
    if let Some(os2) = table(b"OS/2")
        && let (Some(sel), Some(ta), Some(td), Some(tg)) =
            (be16(b, os2 + 62), be16(b, os2 + 68), be16(b, os2 + 70), be16(b, os2 + 72))
        && (sel & 128 != 0 || (a == 0 && d == 0))
    {
        (a, d, g) = (ta, td, tg);
    }
    (upm > 0.0).then(|| (a + d - g) as f32 / 2.0 / upm)
}

/// Shared bytes of a font file (owned or memory-mapped).
pub type FontBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// A font file, memory-mapped: the pages are shared with the system's font
/// cache and only the parts egui reads get paged in, instead of private
/// copies (the three CJK bold faces alone are about 40 MB). Falls back to
/// reading the file.
pub fn font_file(path: &str) -> Option<FontBytes> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: system font files are not truncated or rewritten while the
    // process runs; the map stays valid for the life of the font set.
    match unsafe { memmap2::Mmap::map(&file) } {
        Ok(map) => Some(Arc::new(map)),
        Err(_) => std::fs::read(path).ok().map(|b| Arc::new(b) as FontBytes),
    }
}

/// Font set with a `bold` family: the bold face first, then the CJK bold
/// faces, then the regular proportional chain (symbol fallback). Without a
/// bold face the family is just the regular chain, so text never goes missing.
pub fn font_definitions(bold_face: Option<FontBytes>, cjk_bold: Vec<(String, FontBytes)>) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut chain = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let primary = bold_face.as_deref().and_then(|b| baseline_centre(b.as_ref()));
    for (at, (name, bytes)) in cjk_bold.into_iter().enumerate() {
        let centre = baseline_centre((*bytes).as_ref());
        let mut data = FontData::from_blob(bytes, 0);
        // Line the CJK baseline up with the Latin one (mixed-font rows).
        if let (Some(p), Some(c)) = (primary, centre) {
            data = data.tweak(FontTweak { y_offset_factor: p - c, ..FontTweak::default() });
        }
        fonts.font_data.insert(name.clone(), Arc::new(data));
        chain.insert(at, name);
    }
    if let Some(bytes) = bold_face {
        fonts.font_data.insert("bold-face".into(), Arc::new(FontData::from_blob(bytes, 0)));
        chain.insert(0, "bold-face".into());
    }
    fonts.families.insert(FontFamily::Name(BOLD_FAMILY.into()), chain);
    fonts
}

/// Register the bold family on `ctx`.
pub fn install_fonts(ctx: &Context) {
    let face = BOLD_FACES.iter().find_map(|p| font_file(p));
    let cjk = CJK_BOLD_FACES.iter().filter_map(|(n, p)| font_file(p).map(|b| ((*n).to_string(), b))).collect();
    ctx.set_fonts(font_definitions(face, cjk));
}

/// Line height of a text of `size`: `ceil(size * 1.22)`.
pub fn line_height(size: f32) -> f32 {
    (size * 1.22).ceil()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_rounds_half_to_even() {
        assert_eq!(alpha_byte(0.7), 178);
        assert_eq!(alpha_byte(0.3), 76);
        assert_eq!(alpha_byte(0.5), 128);
        assert_eq!(alpha_byte(2.0), 255);
        assert_eq!(alpha_byte(-1.0), 0);
    }

    #[test]
    fn ink_tokens_follow_the_table() {
        let dark = Tokens::new(true);
        let light = Tokens::new(false);
        assert_eq!(dark.ink().a(), 217);
        assert_eq!(light.ink().a(), 217);
        assert_eq!(dark.secondary().a(), alpha_byte(0.55));
        assert_eq!(light.secondary().a(), alpha_byte(0.50));
        assert_eq!(dark.tertiary().a(), alpha_byte(0.25));
        assert_eq!(light.tertiary().a(), alpha_byte(0.26));
        assert_eq!(dark.divider().a(), alpha_byte(0.06));
        for (t, a) in [
            (dark.ink04(), 0.04),
            (dark.ink06(), 0.06),
            (dark.ink07(), 0.07),
            (dark.ink10(), 0.10),
            (dark.ink16(), 0.16),
            (dark.ink20(), 0.20),
            (dark.ink25(), 0.25),
        ] {
            assert_eq!(t.a(), alpha_byte(a));
        }
        assert_eq!(dark.ink75().a(), alpha_byte(0.75));
        // Ink is white on dark, black on light.
        assert_eq!(dark.ink_a(1.0), Color32::WHITE);
        assert_eq!(light.ink_a(1.0), Color32::BLACK);
    }

    #[test]
    fn bold_family_is_registered() {
        let bytes = || -> FontBytes { Arc::new(vec![0u8]) };
        let with = font_definitions(Some(bytes()), vec![("bold-ja".into(), bytes())]);
        let chain = &with.families[&FontFamily::Name(BOLD_FAMILY.into())];
        assert_eq!(chain[0], "bold-face");
        assert_eq!(chain[1], "bold-ja");
        assert!(chain.len() > 1);
        let without = font_definitions(None, vec![]);
        assert!(!without.families[&FontFamily::Name(BOLD_FAMILY.into())].is_empty());
        assert_eq!(bold(32.0).family, FontFamily::Name(BOLD_FAMILY.into()));
    }

    #[test]
    fn font_files_map_and_match_the_bytes() {
        assert!(font_file("C:/no/such/font.ttf").is_none());
        let Ok(want) = std::fs::read("C:/Windows/Fonts/segoeuib.ttf") else { return };
        let mapped = font_file("C:/Windows/Fonts/segoeuib.ttf").expect("maps");
        assert_eq!((*mapped).as_ref(), &want[..]);
    }

    #[test]
    fn cjk_baseline_is_aligned_to_latin() {
        assert_eq!(baseline_centre(&[0; 8]), None);
        let (Ok(latin), Ok(cjk)) =
            (std::fs::read("C:/Windows/Fonts/segoeuib.ttf"), std::fs::read("C:/Windows/Fonts/YuGothB.ttc"))
        else {
            return;
        };
        let defs = font_definitions(Some(Arc::new(latin.clone())), vec![("bold-ja".into(), Arc::new(cjk.clone()))]);
        let want = baseline_centre(&latin).unwrap() - baseline_centre(&cjk).unwrap();
        assert!(want.abs() > 0.01, "fonts differ, so a tweak is needed");
        assert_eq!(defs.font_data["bold-ja"].tweak.y_offset_factor, want);
    }

    #[test]
    fn colours_and_gradients() {
        let d = Tokens::new(true);
        let l = Tokens::new(false);
        assert_eq!(d.link(), rgb(0x409CFF));
        assert_eq!(l.link(), rgb(0x0069D9));
        assert_eq!(d.menu(), rgb(0x1C263B));
        assert_eq!(l.menu(), rgb(0xF7F7FA));
        assert_eq!(d.window(), rgb(0x0D1733));
        assert_eq!(l.window(), rgb(0xDBEDFF));
        assert_eq!(d.sky(), (rgb(0x0D1733), rgb(0x123066)));
        assert_eq!(l.sky(), (rgb(0xF2FAFF), rgb(0xC2DEFC)));
        assert_eq!(d.accent(), rgb(0x1F70E0));
        assert_eq!(snow_day(), (rgb(0xDBEDFF), rgb(0x8CBDFA)));
        assert_eq!(line_height(11.0), 14.0);
        assert_eq!(line_height(13.0), 16.0);
    }
}
