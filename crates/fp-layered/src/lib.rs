//! Host for CPU-drawn object windows. Not part of the coverage gate.
//!
//! Spec: snowballs 3.7 (ball window recipe, placement thresholds, opacity),
//! characters 0.1 (window recipe, `OrderFront`/`OrderBelow`, under-dock set),
//! trees 3.16, decorations 3.4-3.5 and 7 (desktop band, hover hit-test via
//! alpha mask + cover test), desktop-native 3.4 (re-raise own topmost windows
//! above the overlay), 3.10 (WS_EX_TRANSPARENT toggling), core-app 2.7
//! (tray pointer overlay window).
//!
//! Model: every frame the app passes the full [`Sprite`] list. The host diffs
//! by key: creates/destroys windows, repaints when `rev` or the monitor scale
//! changed (asking the world for a pixmap), moves with `UpdateLayeredWindow`
//! (position + bitmap in one call), and toggles click-through from one shared
//! cursor poll using each window's CPU alpha buffer (layered-art spike).
//!
//! Window box: the window takes the size of the painted bitmap. A bitmap whose
//! size is within one pixel of the frame's pixel size is kept (no repaint on
//! rounding jitter); a larger difference repaints.
//!
//! Layering: the pure decisions live in [`logic`]; everything that touches the
//! OS sits behind the [`Backend`] trait (Win32 in `windows_impl`), so the diff,
//! z-order and hit logic is tested with a recording mock.

pub mod logic;
#[cfg(windows)]
pub mod windows_impl;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use fp_core::desktop::Layout;
use fp_core::geom::PixelRect;
use fp_core::scene::{HitMode, Sprite, SpriteKey, ZBand};
use fp_paint::tiny_skia::Pixmap;
use logic::{Candidate, Mask, Settle};

/// Opaque OS window handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WinId(pub isize);

/// Where to put a window in the z-order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Topmost,
    NotTopmost,
    Top,
    After(WinId),
}

/// OS seam of the host. One implementation per platform.
pub trait Backend {
    /// New hidden click-through layered window.
    fn create(&mut self) -> Option<WinId>;
    fn destroy(&mut self, id: WinId);
    /// Replace the window's bitmap (premultiplied RGBA in, size = pixmap size).
    /// False when the bitmap could not be stored (the old one is gone too).
    fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool;
    /// Move to `(x, y)` pixels, apply `alpha` and show if hidden, in one call.
    fn present(&mut self, id: WinId, x: i32, y: i32, alpha: u8);
    fn hide(&mut self, id: WinId);
    /// Toggle `WS_EX_TRANSPARENT`.
    fn set_click_through(&mut self, id: WinId, on: bool);
    /// z-order change without move/resize/activation.
    fn place(&mut self, id: WinId, p: Place);
    /// Lowest-z taskbar window among the topmost ones.
    fn lowest_taskbar(&self) -> Option<WinId>;
    /// A topmost taskbar window sits above `id`.
    fn taskbar_above(&self, id: WinId) -> bool;
    /// A foreign top-level window lies above `id` at the cursor.
    fn covered(&self, id: WinId, cursor: (i32, i32)) -> bool;
    /// Pixel rect of the foreground window when it is a real foreign app; the
    /// caller counts only monitors it covers completely as fullscreen.
    fn fullscreen(&self) -> Vec<PixelRect>;
    /// Re-apply topmost to those of `ids` found in the topmost band, keeping
    /// their relative order, so they end above everything else topmost.
    fn raise_front(&mut self, ids: &[WinId]);
    /// Order `ids` (back to front) right above the desktop host.
    fn stack_desktop(&mut self, ids: &[WinId]);
    /// Dispatch pending messages of this thread.
    fn pump(&mut self);
}

/// Backend for platforms without an implementation: windows are bookkeeping only.
#[derive(Default)]
pub struct NullBackend {
    next: isize,
}

impl Backend for NullBackend {
    fn create(&mut self) -> Option<WinId> {
        self.next += 1;
        Some(WinId(self.next))
    }
    fn destroy(&mut self, _: WinId) {}
    fn set_bitmap(&mut self, _: WinId, _: &Pixmap) -> bool {
        true
    }
    fn present(&mut self, _: WinId, _: i32, _: i32, _: u8) {}
    fn hide(&mut self, _: WinId) {}
    fn set_click_through(&mut self, _: WinId, _: bool) {}
    fn place(&mut self, _: WinId, _: Place) {}
    fn lowest_taskbar(&self) -> Option<WinId> {
        None
    }
    fn taskbar_above(&self, _: WinId) -> bool {
        false
    }
    fn covered(&self, _: WinId, _: (i32, i32)) -> bool {
        false
    }
    fn fullscreen(&self) -> Vec<PixelRect> {
        Vec::new()
    }
    fn raise_front(&mut self, _: &[WinId]) {}
    fn stack_desktop(&mut self, _: &[WinId]) {}
    fn pump(&mut self) {}
}

#[cfg(windows)]
type PlatformBackend = windows_impl::WinBackend;
#[cfg(not(windows))]
type PlatformBackend = NullBackend;

/// How often z-order is re-asserted (desktop-native 3.4 `KeepOnTop`, decorations 3.7 `Stack`).
const MAINTAIN_EVERY: Duration = Duration::from_secs(1);
/// How often every desktop window re-checks the taskbar overlap
/// (decorations 3.5: Settle on every third 0.1 s tick).
const SETTLE_EVERY: Duration = Duration::from_millis(300);
/// Scale change that forces a repaint.
const SCALE_EPS: f32 = 0.001;

struct Entry {
    id: WinId,
    band: ZBand,
    hit: HitMode,
    rev: u64,
    painted: bool,
    painted_scale: f32,
    /// Size of the current bitmap = size of the window.
    bmp: (i32, i32),
    mask: Option<Mask>,
    pos: (i32, i32),
    alpha: u8,
    shown: bool,
    transparent: bool,
    over_taskbar: bool,
    scale: f64,
}

impl Entry {
    fn rect(&self) -> PixelRect {
        PixelRect::new(self.pos.0, self.pos.1, self.pos.0 + self.bmp.0, self.pos.1 + self.bmp.1)
    }
    fn interactive(&self) -> bool {
        self.shown && self.painted && self.hit != HitMode::Never
    }
}

struct Core<B: Backend> {
    be: B,
    entries: HashMap<SpriteKey, Entry>,
    /// Sprite order of the last sync, back to front.
    order: Vec<SpriteKey>,
    hovered: Option<SpriteKey>,
    near: bool,
    last_maintain: Option<Instant>,
    last_settle: Option<Instant>,
    last_stack: Vec<WinId>,
}

impl<B: Backend> Core<B> {
    fn new(be: B) -> Self {
        Self {
            be,
            entries: HashMap::new(),
            order: Vec::new(),
            hovered: None,
            near: false,
            last_maintain: None,
            last_settle: None,
            last_stack: Vec::new(),
        }
    }

    fn band_place(&mut self, id: WinId, band: ZBand) {
        match band {
            ZBand::Front => self.be.place(id, Place::Topmost),
            ZBand::UnderDock => match self.be.lowest_taskbar() {
                Some(tb) => self.be.place(id, Place::After(tb)),
                None => self.be.place(id, Place::Topmost),
            },
            ZBand::Desktop => self.be.place(id, Place::NotTopmost),
        }
    }

    fn sync(
        &mut self,
        sprites: &[Sprite],
        layout: &Layout,
        paint: &mut dyn FnMut(SpriteKey, f32) -> Option<Pixmap>,
        now: Instant,
    ) {
        // Destroy what is gone.
        let live: HashSet<SpriteKey> = sprites.iter().map(|s| s.key).collect();
        let be = &mut self.be;
        self.entries.retain(|k, e| {
            let keep = live.contains(k);
            if !keep {
                be.destroy(e.id);
            }
            keep
        });
        if self.hovered.is_some_and(|k| !self.entries.contains_key(&k)) {
            self.hovered = None;
        }
        self.order.clear();

        let monitors: Vec<(PixelRect, PixelRect)> =
            layout.screens().iter().map(|s| (s.info.pixels, s.info.work)).collect();
        let mut fullscreen: Option<Vec<PixelRect>> = None;

        for s in sprites {
            if self.order.contains(&s.key) {
                continue; // duplicate key: first one wins
            }
            let mut created = false;
            if !self.entries.contains_key(&s.key) {
                let Some(id) = self.be.create() else { continue };
                self.entries.insert(
                    s.key,
                    Entry {
                        id,
                        band: s.z,
                        hit: s.hit,
                        rev: s.rev,
                        painted: false,
                        painted_scale: 0.0,
                        bmp: (0, 0),
                        mask: None,
                        pos: (0, 0),
                        alpha: 0,
                        shown: false,
                        transparent: true,
                        over_taskbar: false,
                        scale: 1.0,
                    },
                );
                self.band_place(id, s.z);
                created = true;
            }
            self.order.push(s.key);
            let scale = logic::sane_scale(layout.at_point(s.frame.center()).map(|sc| sc.scale()).unwrap_or(1.0));
            let rect = layout.frame_to_pixels(s.frame);
            let alpha = logic::opacity_byte(s.opacity);

            let band_changed = {
                let e = self.entries.get_mut(&s.key).expect("entry exists");
                e.scale = scale;
                let changed = !created && e.band != s.z;
                if changed {
                    e.band = s.z;
                    e.over_taskbar = false;
                }
                changed.then_some(e.id)
            };
            if let Some(id) = band_changed {
                self.band_place(id, s.z);
            }

            let e = self.entries.get_mut(&s.key).expect("entry exists");
            let hit_changed = e.hit != s.hit;
            if hit_changed {
                e.hit = s.hit;
                if s.hit != HitMode::Alpha {
                    e.mask = None;
                }
            }
            // Click-through baseline: Always is clickable, the rest start
            // click-through (Alpha is toggled by `hit`).
            let want = s.hit != HitMode::Always;
            if created || (e.transparent != want && (hit_changed || s.hit != HitMode::Alpha)) {
                e.transparent = want;
                self.be.set_click_through(e.id, want);
            }

            if alpha == 0 {
                if e.shown {
                    self.be.hide(e.id);
                    e.shown = false;
                }
                e.alpha = 0;
                continue;
            }

            let size_ok = e.painted && (e.bmp.0 - rect.width()).abs() <= 1 && (e.bmp.1 - rect.height()).abs() <= 1;
            let need_mask = s.hit == HitMode::Alpha && e.mask.is_none();
            let need = !e.painted
                || e.rev != s.rev
                || (e.painted_scale - scale as f32).abs() > SCALE_EPS
                || !size_ok
                || need_mask;
            let mut fresh = false;
            if need && let Some(pm) = paint(s.key, scale as f32) {
                if self.be.set_bitmap(e.id, &pm) {
                    e.bmp = (pm.width() as i32, pm.height() as i32);
                    e.mask = (s.hit == HitMode::Alpha).then(|| Mask::from_pixmap(&pm));
                    e.painted = true;
                    e.rev = s.rev;
                    e.painted_scale = scale as f32;
                    fresh = true;
                } else {
                    // Old bitmap is gone: hide and retry next frame.
                    log::warn!("layered window bitmap failed for {:?}", s.key);
                    e.painted = false;
                    e.mask = None;
                    if e.shown {
                        self.be.hide(e.id);
                        e.shown = false;
                    }
                }
            }
            if !e.painted {
                continue;
            }
            let pos = (rect.left, rect.top);
            if fresh || !e.shown || e.pos != pos || e.alpha != alpha {
                let moved = e.pos != pos || !e.shown || fresh;
                e.pos = pos;
                e.alpha = alpha;
                e.shown = true;
                self.be.present(e.id, pos.0, pos.1, alpha);
                if moved && e.band == ZBand::Desktop {
                    let fs = fullscreen.get_or_insert_with(|| self.be.fullscreen());
                    let id = e.id;
                    let touching = logic::touches_taskbar(e.rect(), e.scale, &monitors, fs);
                    let act = logic::settle(touching, e.over_taskbar, || self.be.taskbar_above(id));
                    apply_settle(&mut self.be, e, act);
                }
            }
        }

        let due = self.last_maintain.is_none_or(|t| now.saturating_duration_since(t) >= MAINTAIN_EVERY);
        if due {
            self.last_maintain = Some(now);
            self.last_settle = Some(now);
            self.maintain(&monitors);
            return;
        }
        if self.last_settle.is_none_or(|t| now.saturating_duration_since(t) >= SETTLE_EVERY) {
            self.last_settle = Some(now);
            self.settle_all(&monitors);
        }
        // Cheap when nothing changed: `restack` compares against the last
        // stacked list, so creation, band changes, lowering and pure order
        // changes (a raised decoration moves to the end) all restack here.
        self.restack();
    }

    /// Settle every visible desktop window (decorations 3.7).
    fn settle_all(&mut self, monitors: &[(PixelRect, PixelRect)]) {
        if !self.entries.values().any(|e| e.band == ZBand::Desktop && e.shown) {
            return;
        }
        let fs = self.be.fullscreen();
        for k in &self.order {
            let Some(e) = self.entries.get_mut(k) else { continue };
            if e.band != ZBand::Desktop || !e.shown {
                continue;
            }
            let id = e.id;
            let touching = logic::touches_taskbar(e.rect(), e.scale, monitors, &fs);
            let act = logic::settle(touching, e.over_taskbar, || self.be.taskbar_above(id));
            apply_settle(&mut self.be, e, act);
        }
    }

    /// Ids of visible windows of one band, in sprite order.
    fn band_ids(&self, band: ZBand, only_flat: bool) -> Vec<WinId> {
        self.order
            .iter()
            .filter_map(|k| self.entries.get(k))
            .filter(|e| e.band == band && e.shown && (!only_flat || !e.over_taskbar))
            .map(|e| e.id)
            .collect()
    }

    /// Periodic z-order upkeep.
    fn maintain(&mut self, monitors: &[(PixelRect, PixelRect)]) {
        let front = self.band_ids(ZBand::Front, false);
        if !front.is_empty() {
            self.be.raise_front(&front);
        }
        for id in self.band_ids(ZBand::UnderDock, false) {
            self.band_place(id, ZBand::UnderDock);
        }
        // Desktop: settle everyone, then a full stack check.
        self.settle_all(monitors);
        self.last_stack.clear();
        self.restack();
    }

    /// Run the desktop stack when the candidate list changed (or was cleared).
    fn restack(&mut self) {
        let ids = self.band_ids(ZBand::Desktop, true);
        if ids.is_empty() {
            self.last_stack.clear();
            return;
        }
        if self.last_stack != ids {
            self.be.stack_desktop(&ids);
            self.last_stack = ids;
        }
    }

    fn hit(&mut self, cursor: Option<(i32, i32)>) -> Option<SpriteKey> {
        let mut keys: Vec<SpriteKey> = Vec::new();
        let mut cands: Vec<Candidate> = Vec::new();
        for k in &self.order {
            if let Some(e) = self.entries.get(k).filter(|e| e.interactive()) {
                keys.push(*k);
                let mask = if e.hit == HitMode::Alpha { e.mask.as_ref() } else { None };
                cands.push(Candidate { rect: e.rect(), mask, scale: e.scale });
            }
        }
        let hovered_idx = self.hovered.and_then(|h| keys.iter().position(|k| *k == h));
        let (hot, near) = match cursor {
            None => (None, false),
            Some(c) => {
                let near = cands.iter().zip(&keys).any(|(cd, k)| {
                    self.entries.get(k).is_some_and(|e| e.hit == HitMode::Alpha)
                        && cd.rect.contains(c.0 as f64, c.1 as f64)
                });
                let mut covered = |i: usize| self.entries.get(&keys[i]).is_some_and(|e| self.be.covered(e.id, c));
                (logic::pick(&cands, c, hovered_idx, &mut covered).map(|i| keys[i]), near)
            }
        };
        self.near = near;
        self.hovered = hot;
        for (k, e) in self.entries.iter_mut() {
            if e.hit != HitMode::Alpha {
                continue;
            }
            let want = Some(*k) != hot;
            if e.transparent != want {
                e.transparent = want;
                self.be.set_click_through(e.id, want);
            }
        }
        hot
    }

    fn clear(&mut self) {
        for (_, e) in self.entries.drain() {
            self.be.destroy(e.id);
        }
        self.order.clear();
        self.hovered = None;
        self.near = false;
        self.last_maintain = None;
        self.last_settle = None;
        self.last_stack.clear();
    }
}

/// Apply a settle decision. A lowered window rejoins the desktop list, which
/// the next `restack` picks up.
fn apply_settle<B: Backend>(be: &mut B, e: &mut Entry, act: Settle) {
    match act {
        Settle::Nothing => {}
        Settle::Raise => {
            e.over_taskbar = true;
            be.place(e.id, Place::Topmost);
        }
        Settle::Lower => {
            e.over_taskbar = false;
            be.place(e.id, Place::NotTopmost);
        }
    }
}

/// Diffing window host.
pub struct LayeredHost {
    core: Core<PlatformBackend>,
}

impl Default for LayeredHost {
    fn default() -> Self {
        Self { core: Core::new(PlatformBackend::default()) }
    }
}

impl LayeredHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply this frame's sprites. `paint(key, scale)` renders when needed.
    /// Sprite order matters for `ZBand::Desktop` (later = nearer the front)
    /// and for hit-testing (later = on top).
    pub fn sync(
        &mut self,
        sprites: &[Sprite],
        layout: &Layout,
        paint: &mut dyn FnMut(SpriteKey, f32) -> Option<Pixmap>,
    ) {
        self.core.sync(sprites, layout, paint, Instant::now());
    }

    /// Sprite under the cursor whose pixel is opaque and not covered
    /// (cursor in pixels). Also toggles WS_EX_TRANSPARENT.
    pub fn hit(&mut self, cursor_px: Option<(i32, i32)>) -> Option<SpriteKey> {
        self.core.hit(cursor_px)
    }

    /// True while the cursor is inside the box of an alpha-hit sprite: the
    /// caller should poll [`LayeredHost::hit`] faster (every frame) then.
    pub fn fast_poll(&self) -> bool {
        self.core.near
    }

    /// Native ids of the shown `ZBand::UnderDock` windows (hosts exclude them
    /// from z-order re-raises).
    pub fn under_dock_ids(&self) -> Vec<isize> {
        self.core.band_ids(ZBand::UnderDock, false).into_iter().map(|i| i.0).collect()
    }

    /// Re-raise every front window above other topmost windows (the weather
    /// overlay re-asserts itself every second; call this right after it).
    pub fn raise_front(&mut self) {
        let ids = self.core.band_ids(ZBand::Front, false);
        if !ids.is_empty() {
            self.core.be.raise_front(&ids);
        }
    }

    /// Dispatch pending window messages of this thread (for callers without
    /// their own message loop).
    pub fn pump_messages(&mut self) {
        self.core.be.pump();
    }

    /// Number of live windows.
    pub fn len(&self) -> usize {
        self.core.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.core.entries.is_empty()
    }

    /// Destroy all windows.
    pub fn clear(&mut self) {
        self.core.clear();
    }
}

impl Drop for LayeredHost {
    fn drop(&mut self) {
        self.core.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::desktop::MonitorInfo;
    use fp_core::geom::Frame;
    use fp_core::scene::Owner;

    #[derive(Default)]
    struct Mock {
        next: isize,
        log: Vec<String>,
        taskbar: Option<WinId>,
        taskbar_above: bool,
        covered: bool,
        fullscreen: Vec<PixelRect>,
        fail_create: bool,
        fail_bitmap: bool,
    }

    impl Backend for Mock {
        fn create(&mut self) -> Option<WinId> {
            if self.fail_create {
                return None;
            }
            self.next += 1;
            self.log.push(format!("create {}", self.next));
            Some(WinId(self.next))
        }
        fn destroy(&mut self, id: WinId) {
            self.log.push(format!("destroy {}", id.0));
        }
        fn set_bitmap(&mut self, id: WinId, pm: &Pixmap) -> bool {
            self.log.push(format!("bitmap {} {}x{}", id.0, pm.width(), pm.height()));
            !self.fail_bitmap
        }
        fn present(&mut self, id: WinId, x: i32, y: i32, a: u8) {
            self.log.push(format!("present {} {x},{y} a{a}", id.0));
        }
        fn hide(&mut self, id: WinId) {
            self.log.push(format!("hide {}", id.0));
        }
        fn set_click_through(&mut self, id: WinId, on: bool) {
            self.log.push(format!("ct {} {on}", id.0));
        }
        fn place(&mut self, id: WinId, p: Place) {
            self.log.push(format!("place {} {p:?}", id.0));
        }
        fn lowest_taskbar(&self) -> Option<WinId> {
            self.taskbar
        }
        fn taskbar_above(&self, _: WinId) -> bool {
            self.taskbar_above
        }
        fn covered(&self, _: WinId, _: (i32, i32)) -> bool {
            self.covered
        }
        fn fullscreen(&self) -> Vec<PixelRect> {
            self.fullscreen.clone()
        }
        fn raise_front(&mut self, ids: &[WinId]) {
            self.log.push(format!("raise {:?}", ids.iter().map(|i| i.0).collect::<Vec<_>>()));
        }
        fn stack_desktop(&mut self, ids: &[WinId]) {
            self.log.push(format!("stack {:?}", ids.iter().map(|i| i.0).collect::<Vec<_>>()));
        }
        fn pump(&mut self) {
            self.log.push("pump".into());
        }
    }

    fn layout() -> Layout {
        Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "a".into(),
            pixels: PixelRect::new(0, 0, 1920, 1080),
            work: PixelRect::new(0, 0, 1920, 1032),
            scale: 1.0,
            primary: true,
        }])
    }

    fn sprite(id: u64, z: ZBand, hit: HitMode, x: f64, y: f64) -> Sprite {
        Sprite {
            key: SpriteKey::new(Owner::Ball, id),
            frame: Frame::new(x, y, 20.0, 20.0),
            z,
            hit,
            rev: 1,
            opacity: 1.0,
        }
    }

    fn solid_pm(w: u32, h: u32) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        p.fill(fp_paint::tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        p
    }

    fn run(core: &mut Core<Mock>, sprites: &[Sprite], now: Instant) -> usize {
        let mut paints = 0;
        core.sync(
            sprites,
            &layout(),
            &mut |_, _| {
                paints += 1;
                Some(solid_pm(20, 20))
            },
            now,
        );
        paints
    }

    fn log(core: &mut Core<Mock>) -> Vec<String> {
        std::mem::take(&mut core.be.log)
    }

    #[test]
    fn creates_paints_and_presents_once() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s = [sprite(1, ZBand::Front, HitMode::Never, 100.0, 100.0)];
        assert_eq!(run(&mut c, &s, t0), 1);
        assert_eq!(
            log(&mut c),
            vec!["create 1", "place 1 Topmost", "ct 1 true", "bitmap 1 20x20", "present 1 100,960 a255", "raise [1]"]
        );
        assert_eq!(run(&mut c, &s, t0), 0);
        assert!(log(&mut c).is_empty());
        let mut m = s;
        m[0].frame.x = 110.0;
        assert_eq!(run(&mut c, &m, t0), 0);
        assert_eq!(log(&mut c), vec!["present 1 110,960 a255"]);
        m[0].opacity = 0.5;
        run(&mut c, &m, t0);
        assert_eq!(log(&mut c), vec!["present 1 110,960 a128"]);
        m[0].rev = 2;
        assert_eq!(run(&mut c, &m, t0), 1);
        assert_eq!(log(&mut c), vec!["bitmap 1 20x20", "present 1 110,960 a128"]);
    }

    #[test]
    fn hide_show_and_destroy() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Front, HitMode::Never, 0.0, 0.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        s[0].opacity = 0.0;
        assert_eq!(run(&mut c, &s, t0), 0);
        assert_eq!(log(&mut c), vec!["hide 1"]);
        run(&mut c, &s, t0);
        assert!(log(&mut c).is_empty(), "already hidden");
        s[0].opacity = 1.0;
        assert_eq!(run(&mut c, &s, t0), 0, "bitmap kept while hidden");
        assert_eq!(log(&mut c), vec!["present 1 0,1060 a255"]);
        assert_eq!(c.entries.len(), 1);
        run(&mut c, &[], t0);
        assert_eq!(log(&mut c), vec!["destroy 1"]);
        assert!(c.entries.is_empty());
    }

    #[test]
    fn size_drift_repaints_only_when_big() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Front, HitMode::Never, 0.0, 0.0)];
        run(&mut c, &s, t0);
        s[0].frame.w = 21.0;
        assert_eq!(run(&mut c, &s, t0), 0);
        s[0].frame.w = 30.0;
        assert_eq!(run(&mut c, &s, t0), 1);
    }

    #[test]
    fn paint_none_retries_and_skips_present() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s = [sprite(1, ZBand::Front, HitMode::Never, 0.0, 0.0)];
        let mut n = 0;
        for _ in 0..2 {
            c.sync(
                &s,
                &layout(),
                &mut |_, _| {
                    n += 1;
                    None
                },
                t0,
            );
        }
        assert_eq!(n, 2);
        assert!(!log(&mut c).iter().any(|l| l.starts_with("present")));
        let mut m = s;
        assert_eq!(run(&mut c, &m, t0), 1);
        m[0].rev = 9;
        c.sync(&m, &layout(), &mut |_, _| None, t0);
        assert!(c.entries.values().next().unwrap().painted);
    }

    #[test]
    fn create_failure_skips_sprite() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock { fail_create: true, ..Default::default() });
        assert_eq!(run(&mut c, &[sprite(1, ZBand::Front, HitMode::Never, 0.0, 0.0)], t0), 0);
        assert!(c.entries.is_empty());
        assert!(c.order.is_empty());
    }

    #[test]
    fn duplicate_keys_first_wins() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let a = sprite(1, ZBand::Front, HitMode::Never, 0.0, 0.0);
        let b = sprite(1, ZBand::Front, HitMode::Never, 500.0, 0.0);
        run(&mut c, &[a, b], t0);
        assert_eq!(c.entries.len(), 1);
        assert_eq!(c.order.len(), 1);
    }

    #[test]
    fn under_dock_ids_lists_shown_under_dock_only() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s =
            [sprite(1, ZBand::UnderDock, HitMode::Never, 0.0, 0.0), sprite(2, ZBand::Front, HitMode::Never, 0.0, 0.0)];
        run(&mut c, &s, t0);
        let ids: Vec<isize> = c.band_ids(ZBand::UnderDock, false).into_iter().map(|i| i.0).collect();
        assert_eq!(ids.len(), 1);
        assert!(LayeredHost::new().under_dock_ids().is_empty());
    }

    #[test]
    fn under_dock_places_after_taskbar() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock { taskbar: Some(WinId(99)), ..Default::default() });
        run(&mut c, &[sprite(1, ZBand::UnderDock, HitMode::Never, 0.0, 0.0)], t0);
        assert!(log(&mut c).contains(&"place 1 After(WinId(99))".to_string()));
        let mut c = Core::new(Mock::default());
        run(&mut c, &[sprite(1, ZBand::UnderDock, HitMode::Never, 0.0, 0.0)], t0);
        assert!(log(&mut c).contains(&"place 1 Topmost".to_string()));
    }

    #[test]
    fn maintenance_runs_every_second() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s =
            [sprite(1, ZBand::Front, HitMode::Never, 0.0, 500.0), sprite(2, ZBand::Front, HitMode::Never, 50.0, 500.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        run(&mut c, &s, t0 + Duration::from_millis(500));
        assert!(log(&mut c).is_empty());
        run(&mut c, &s, t0 + Duration::from_millis(1000));
        assert_eq!(log(&mut c), vec!["raise [1, 2]"]);
    }

    #[test]
    fn host_public_api_smoke() {
        let mut h = LayeredHost::new();
        h.raise_front();
        h.pump_messages();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert!(!h.fast_poll());
        assert_eq!(h.hit(None), None);
        h.sync(&[], &layout(), &mut |_, _| None);
        h.clear();
    }

    #[test]
    fn desktop_stack_and_settle() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        // Window 2 sits on the taskbar strip (pixels y 1032..1080).
        let s = [
            sprite(1, ZBand::Desktop, HitMode::Never, 100.0, 500.0),
            sprite(2, ZBand::Desktop, HitMode::Never, 200.0, 5.0),
        ];
        run(&mut c, &s, t0);
        let l = log(&mut c);
        assert!(l.contains(&"place 1 NotTopmost".to_string()));
        assert!(l.contains(&"place 2 Topmost".to_string()), "overlaps the taskbar: {l:?}");
        assert!(l.contains(&"stack [1]".to_string()), "over-taskbar window not stacked: {l:?}");
        run(&mut c, &s, t0);
        assert!(log(&mut c).is_empty());
        let mut m = s;
        m[1].frame.y = 400.0;
        run(&mut c, &m, t0);
        let l = log(&mut c);
        assert!(l.contains(&"place 2 NotTopmost".to_string()), "{l:?}");
        assert!(l.contains(&"stack [1, 2]".to_string()), "{l:?}");
        c.be.fullscreen = vec![PixelRect::new(0, 0, 1920, 1080)];
        m[1].frame.y = 5.0;
        run(&mut c, &m, t0);
        assert!(!log(&mut c).contains(&"place 2 Topmost".to_string()));
    }

    #[test]
    fn settle_rechecks_taskbar_above() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Desktop, HitMode::Never, 200.0, 5.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        s[0].frame.x = 210.0;
        run(&mut c, &s, t0);
        assert_eq!(log(&mut c), vec!["present 1 210,1055 a255"]);
        c.be.taskbar_above = true;
        s[0].frame.x = 220.0;
        run(&mut c, &s, t0);
        assert!(log(&mut c).contains(&"place 1 Topmost".to_string()));
    }

    #[test]
    fn maintenance_settles_desktop() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s = [sprite(1, ZBand::Desktop, HitMode::Never, 200.0, 500.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        // a fullscreen app appears and goes away; periodic pass keeps state sane
        run(&mut c, &s, t0 + Duration::from_secs(2));
        assert_eq!(log(&mut c), vec!["stack [1]"]);
    }

    #[test]
    fn periodic_settle_every_300ms() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s = [sprite(1, ZBand::Desktop, HitMode::Never, 200.0, 5.0)];
        run(&mut c, &s, t0);
        assert!(log(&mut c).contains(&"place 1 Topmost".to_string()));
        // The taskbar gets re-raised above a still window: only the timed
        // settle can notice it.
        c.be.taskbar_above = true;
        run(&mut c, &s, t0 + Duration::from_millis(200));
        assert!(log(&mut c).is_empty(), "not due yet");
        run(&mut c, &s, t0 + Duration::from_millis(300));
        assert_eq!(log(&mut c), vec!["place 1 Topmost"]);
    }

    #[test]
    fn maximized_foreground_does_not_block_raise() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock { fullscreen: vec![PixelRect::new(0, 0, 1920, 1032)], ..Default::default() });
        run(&mut c, &[sprite(1, ZBand::Desktop, HitMode::Never, 200.0, 5.0)], t0);
        assert!(log(&mut c).contains(&"place 1 Topmost".to_string()));
    }

    #[test]
    fn desktop_order_change_restacks_at_once() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let a = sprite(1, ZBand::Desktop, HitMode::Never, 100.0, 500.0);
        let b = sprite(2, ZBand::Desktop, HitMode::Never, 300.0, 500.0);
        run(&mut c, &[a, b], t0);
        assert!(log(&mut c).contains(&"stack [1, 2]".to_string()));
        run(&mut c, &[b, a], t0 + Duration::from_millis(10));
        assert_eq!(log(&mut c), vec!["stack [2, 1]"]);
    }

    #[test]
    fn bitmap_failure_hides_and_retries() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Front, HitMode::Alpha, 0.0, 500.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        c.be.fail_bitmap = true;
        s[0].rev = 2;
        assert_eq!(run(&mut c, &s, t0), 1);
        assert_eq!(log(&mut c), vec!["bitmap 1 20x20", "hide 1"]);
        assert_eq!(c.hit(Some((10, 570))), None, "unpainted window is not hit");
        c.be.fail_bitmap = false;
        assert_eq!(run(&mut c, &s, t0), 1, "retried next frame");
        assert_eq!(log(&mut c), vec!["bitmap 1 20x20", "present 1 0,560 a255"]);
    }

    #[test]
    fn bad_monitor_scale_paints_at_one() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let l = Layout::new(vec![MonitorInfo {
            id: "z".into(),
            name: "z".into(),
            pixels: PixelRect::new(0, 0, 1920, 1080),
            work: PixelRect::new(0, 0, 1920, 1032),
            scale: 0.0,
            primary: true,
        }]);
        let mut seen = Vec::new();
        c.sync(
            &[sprite(1, ZBand::Front, HitMode::Never, 0.0, 500.0)],
            &l,
            &mut |_, sc| {
                seen.push(sc);
                Some(solid_pm(20, 20))
            },
            t0,
        );
        assert_eq!(seen, vec![1.0]);
    }

    #[test]
    fn band_change_replaces() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Front, HitMode::Never, 0.0, 500.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        s[0].z = ZBand::Desktop;
        run(&mut c, &s, t0);
        let l = log(&mut c);
        assert!(l.contains(&"place 1 NotTopmost".to_string()));
        assert!(l.contains(&"stack [1]".to_string()));
    }

    #[test]
    fn hit_toggles_click_through() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let s = [
            sprite(1, ZBand::Front, HitMode::Alpha, 100.0, 500.0),
            sprite(2, ZBand::Front, HitMode::Always, 300.0, 500.0),
            sprite(3, ZBand::Front, HitMode::Never, 500.0, 500.0),
        ];
        run(&mut c, &s, t0);
        let l = log(&mut c);
        assert!(l.contains(&"ct 1 true".to_string()));
        assert!(l.contains(&"ct 2 false".to_string()), "Always is clickable");
        assert!(l.contains(&"ct 3 true".to_string()));
        // window 1 spans pixels x 100..120, y 560..580
        let k1 = s[0].key;
        assert_eq!(c.hit(Some((110, 570))), Some(k1));
        assert_eq!(log(&mut c), vec!["ct 1 false"]);
        assert!(c.near);
        assert_eq!(c.hit(Some((110, 570))), Some(k1));
        assert!(log(&mut c).is_empty());
        assert_eq!(c.hit(Some((310, 570))), Some(s[1].key));
        assert_eq!(log(&mut c), vec!["ct 1 true"]);
        assert!(!c.near, "always sprites do not request fast polling");
        assert_eq!(c.hit(Some((510, 570))), None);
        assert_eq!(c.hit(None), None);
        c.be.covered = true;
        assert_eq!(c.hit(Some((110, 570))), None);
        assert!(!log(&mut c).contains(&"ct 1 false".to_string()));
    }

    #[test]
    fn hit_ignores_hidden_and_unpainted() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Front, HitMode::Alpha, 100.0, 500.0)];
        s[0].opacity = 0.0;
        run(&mut c, &s, t0);
        assert_eq!(c.hit(Some((110, 570))), None);
        s[0].opacity = 1.0;
        run(&mut c, &s, t0);
        assert_eq!(c.hit(Some((110, 570))), Some(s[0].key));
        run(&mut c, &[], t0);
        assert_eq!(c.hit(Some((110, 570))), None);
        assert!(c.hovered.is_none());
    }

    #[test]
    fn hit_mode_changes_follow() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        let mut s = [sprite(1, ZBand::Front, HitMode::Never, 100.0, 500.0)];
        run(&mut c, &s, t0);
        log(&mut c);
        s[0].hit = HitMode::Always;
        run(&mut c, &s, t0);
        assert_eq!(log(&mut c), vec!["ct 1 false"]);
        s[0].hit = HitMode::Alpha;
        assert_eq!(run(&mut c, &s, t0), 1, "mask needed: repaint");
        assert!(log(&mut c).contains(&"ct 1 true".to_string()), "alpha starts click-through");
        assert!(c.entries.values().next().unwrap().mask.is_some());
        s[0].hit = HitMode::Never;
        run(&mut c, &s, t0);
        assert!(c.entries.values().next().unwrap().mask.is_none());
    }

    #[test]
    fn clear_destroys_all() {
        let t0 = Instant::now();
        let mut c = Core::new(Mock::default());
        run(
            &mut c,
            &[sprite(1, ZBand::Front, HitMode::Alpha, 0.0, 0.0), sprite(2, ZBand::Front, HitMode::Alpha, 50.0, 0.0)],
            t0,
        );
        log(&mut c);
        c.clear();
        assert_eq!(log(&mut c).iter().filter(|s| s.starts_with("destroy")).count(), 2);
        assert!(c.entries.is_empty());
    }

    #[test]
    fn null_backend_is_inert() {
        let mut b = NullBackend::default();
        let id = b.create().unwrap();
        assert_ne!(b.create().unwrap(), id);
        b.destroy(id);
        b.set_bitmap(id, &solid_pm(1, 1));
        b.present(id, 0, 0, 1);
        b.hide(id);
        b.set_click_through(id, true);
        b.place(id, Place::Top);
        assert!(b.lowest_taskbar().is_none());
        assert!(!b.taskbar_above(id));
        assert!(!b.covered(id, (0, 0)));
        assert!(b.fullscreen().is_empty());
        b.raise_front(&[id]);
        b.stack_desktop(&[id]);
        b.pump();
    }
}
