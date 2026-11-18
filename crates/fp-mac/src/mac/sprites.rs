//! Sprite windows of `fp-layered` (porting design 3.1 "Sprites"): one
//! borderless, non-activating `NSPanel` per sprite whose hosted `CALayer`
//! shows the premultiplied RGBA pixmap as a `CGImage`. Bands map to window
//! levels: `Topmost` = [`FRONT_LEVEL`], `NotTopmost` = desktop sprites right
//! above the desktop icons, `After(DOCK)` = right below the Dock. The per-pixel
//! hit test stays the shared alpha-mask cursor poll: `set_click_through`
//! toggles `ignoresMouseEvents`.

use super::{screens, window, winlist};
use crate::logic::{self, DESKTOP_SPRITE_LEVEL, DOCK_ID, FRONT_LEVEL, UNDER_DOCK_LEVEL};
use fp_core::geom::PixelRect;
use fp_platform::sprites::{Place, SpriteBackend, WinId, stack_ok};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSPanel, NSWindowOrderingMode};
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};
use objc2_quartz_core::{CALayer, CATransaction};
use std::collections::HashMap;
use tiny_skia::Pixmap;

struct Sprite {
    window: Retained<NSPanel>,
    layer: Retained<CALayer>,
    image: Option<CFRetained<CGImage>>,
    size: (u32, u32),
    shown: bool,
}

/// AppKit implementation of [`SpriteBackend`]. Ids are window numbers.
pub struct MacSprites {
    mtm: MainThreadMarker,
    wins: HashMap<isize, Sprite>,
    srgb: Option<CFRetained<CGColorSpace>>,
}

impl MacSprites {
    pub fn new(mtm: MainThreadMarker) -> Self {
        // SAFETY: reading an immutable framework constant.
        let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
        Self { mtm, wins: HashMap::new(), srgb }
    }

    /// Premultiplied RGBA8, the pixmap's own layout: no conversion.
    fn image(&self, pm: &Pixmap) -> Option<CFRetained<CGImage>> {
        let (w, h) = (pm.width() as usize, pm.height() as usize);
        let data = CFData::from_bytes(pm.data());
        let provider = CGDataProvider::with_cf_data(Some(&data))?;
        // Default byte order (0): R, G, B, A in memory.
        let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0);
        // SAFETY: 8 bits x 4 components, rows of w*4 bytes, matching `data`;
        // no decode array.
        unsafe {
            CGImage::new(
                w,
                h,
                8,
                32,
                w * 4,
                self.srgb.as_deref(),
                info,
                Some(&provider),
                std::ptr::null(),
                false,
                CGColorRenderingIntent::RenderingIntentDefault,
            )
        }
    }

    fn level_of(&self, id: WinId) -> Option<i64> {
        self.wins.get(&id.0).map(|s| s.window.level() as i64)
    }

    fn set_level(s: &Sprite, level: i64) {
        if s.window.level() as i64 != level {
            s.window.setLevel(level as isize);
            if s.shown {
                s.window.orderFrontRegardless();
            }
        }
    }

    /// Own on-screen window numbers at `level`, back to front.
    fn run_at(&self, level: i64) -> Vec<WinId> {
        let own = winlist::own_pid();
        let mut run: Vec<WinId> = winlist::all_on_screen()
            .into_iter()
            .filter(|w| w.pid == own && w.layer == level)
            .map(|w| WinId(w.id as isize))
            .collect();
        run.reverse();
        run
    }
}

fn transaction(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

impl SpriteBackend for MacSprites {
    fn create(&mut self) -> Option<WinId> {
        let layer = CALayer::new();
        let window = window::panel(self.mtm, logic::CocoaRect::new(0.0, 0.0, 1.0, 1.0), &layer, logic::NORMAL_LEVEL);
        let n = window.windowNumber();
        if n <= 0 {
            window.close();
            return None;
        }
        self.wins.insert(n, Sprite { window, layer, image: None, size: (0, 0), shown: false });
        Some(WinId(n))
    }

    fn destroy(&mut self, id: WinId) {
        if let Some(s) = self.wins.remove(&id.0) {
            s.window.orderOut(None);
            s.window.close();
        }
    }

    fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool {
        let image = self.image(pm);
        let Some(s) = self.wins.get_mut(&id.0) else { return false };
        let scale = screens::space().scale;
        transaction(|| {
            // SAFETY: `contents` takes a CGImage; nil clears it.
            unsafe { s.layer.setContents(image.as_deref().map(window::any)) };
            s.layer.setContentsScale(scale);
        });
        s.size = (pm.width(), pm.height());
        let ok = image.is_some();
        s.image = image;
        ok
    }

    fn present(&mut self, id: WinId, x: i32, y: i32, alpha: u8) {
        let Some(s) = self.wins.get_mut(&id.0) else { return };
        if s.image.is_none() {
            return;
        }
        let frame = logic::sprite_frame(x, y, s.size.0, s.size.1, screens::space());
        transaction(|| {
            s.window.setFrame_display(window::ns_rect(frame), false);
            window::fit_layer(&s.window, &s.layer);
            s.window.setAlphaValue(f64::from(alpha) / 255.0);
        });
        if !s.shown {
            s.window.orderFrontRegardless();
            s.shown = true;
        }
    }

    fn hide(&mut self, id: WinId) {
        if let Some(s) = self.wins.get_mut(&id.0) {
            s.window.orderOut(None);
            s.shown = false;
        }
    }

    fn set_click_through(&mut self, id: WinId, on: bool) {
        if let Some(s) = self.wins.get(&id.0)
            && s.window.ignoresMouseEvents() != on
        {
            s.window.setIgnoresMouseEvents(on);
        }
    }

    /// Win32 `insertAfter` semantics: `After(w)` puts the window right below `w`.
    fn place(&mut self, id: WinId, p: Place) {
        let Some(s) = self.wins.get(&id.0) else { return };
        match p {
            Place::Topmost => Self::set_level(s, FRONT_LEVEL),
            Place::NotTopmost => Self::set_level(s, DESKTOP_SPRITE_LEVEL),
            Place::Top => {
                if s.shown {
                    s.window.orderFrontRegardless();
                }
            }
            Place::After(WinId(DOCK_ID)) => Self::set_level(s, UNDER_DOCK_LEVEL),
            Place::After(other) => {
                let Some(level) = self.level_of(other) else { return };
                Self::set_level(s, level);
                if s.shown {
                    s.window.orderWindow_relativeTo(NSWindowOrderingMode::Below, other.0);
                }
            }
        }
    }

    /// The Dock window always exists (also when auto-hidden).
    fn lowest_taskbar(&self) -> Option<WinId> {
        Some(WinId(DOCK_ID))
    }

    fn taskbar_above(&self, id: WinId) -> bool {
        self.level_of(id).is_some_and(logic::dock_above)
    }

    fn covered(&self, id: WinId, cursor: (i32, i32)) -> bool {
        let Ok(n) = u32::try_from(id.0) else { return false };
        let at = screens::space().px_quartz(cursor.0, cursor.1);
        logic::covered(&winlist::above(n), winlist::own_pid(), at)
    }

    fn fullscreen(&self) -> Vec<PixelRect> {
        let (raws, sp) = screens::read(self.mtm);
        let rects: Vec<PixelRect> = raws.iter().map(|s| sp.rect_px(s.frame)).collect();
        logic::fullscreen(&winlist::on_screen(), winlist::own_pid(), &rects, sp)
    }

    fn raise_front(&mut self, ids: &[WinId]) {
        let own = winlist::own_pid();
        let found: Vec<WinId> = winlist::all_on_screen()
            .into_iter()
            .filter(|w| w.pid == own && w.layer == FRONT_LEVEL)
            .map(|w| WinId(w.id as isize))
            .filter(|w| ids.contains(w))
            .collect();
        for id in found.into_iter().rev() {
            if let Some(s) = self.wins.get(&id.0) {
                s.window.orderFrontRegardless();
            }
        }
    }

    fn stack_desktop(&mut self, ids: &[WinId]) {
        if ids.is_empty() {
            return;
        }
        for id in ids {
            if let Some(s) = self.wins.get(&id.0) {
                Self::set_level(s, DESKTOP_SPRITE_LEVEL);
            }
        }
        if stack_ok(&self.run_at(DESKTOP_SPRITE_LEVEL), ids) {
            return;
        }
        // Each one to the front of the level, in list order: back to front.
        for id in ids {
            if let Some(s) = self.wins.get(&id.0).filter(|s| s.shown) {
                s.window.orderFrontRegardless();
            }
        }
    }

    /// AppKit's run loop dispatches everything.
    fn pump(&mut self) {}
}

impl Drop for MacSprites {
    fn drop(&mut self) {
        for (_, s) in self.wins.drain() {
            s.window.orderOut(None);
            s.window.close();
        }
    }
}
