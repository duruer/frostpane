//! Stand-ins for targets without a native backend yet. They never fail and
//! never draw: sprite windows are bookkeeping only, the overlay reports
//! itself unavailable, there are no hooks, no system events and no OS timer.
//! Backend crates fill in what they already know (data folder, language,
//! time zone, UTC offset).

use crate::overlay::{OverlayBackend, PanelWindow, pick_alpha};
use crate::sprites::{Place, SpriteBackend, WinId};
use crate::{Driver, Guard, Instance, InstanceGuard, Platform, PowerStatus, Shell, SystemEvent, SystemEvents};
use crate::{NATIVE_BACKENDS, WallpaperScreen};
use fp_core::desktop::MonitorInfo;
use fp_core::geom::PixelRect;
use fp_tracker::{HookEvent, WindowInfo};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use tiny_skia::Pixmap;

/// Error text of [`NullOverlay::create_panel`].
pub const OVERLAY_UNAVAILABLE: &str = "overlay not yet supported on this platform";

/// `Platform` + `Shell` + `SystemEvents` + `Driver` without OS calls.
#[derive(Clone, Debug)]
pub struct NullPlatform {
    pub data_dir: PathBuf,
    /// Two-letter UI language code.
    pub language: String,
    pub time_zone: String,
    pub monitors: Vec<MonitorInfo>,
    /// Source of [`Platform::utc_offset_s`].
    pub utc_offset: fn() -> i32,
}

impl NullPlatform {
    /// English, UTC, no monitors.
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir, language: "en".into(), time_zone: "UTC".into(), monitors: Vec::new(), utc_offset: || 0 }
    }
}

/// The lock of a platform without single-instance support: never signalled.
struct NullGuard;

impl InstanceGuard for NullGuard {
    fn listen(&self, _: Box<dyn Fn() + Send>) {}
}

impl Platform for NullPlatform {
    fn monitors(&self) -> Vec<MonitorInfo> {
        self.monitors.clone()
    }
    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
    fn os_language(&self) -> String {
        self.language.clone()
    }
    fn power(&self) -> PowerStatus {
        PowerStatus::default()
    }
    fn reduced_motion(&self) -> bool {
        false
    }
    fn cursor(&self) -> Option<(i32, i32)> {
        None
    }
    fn left_button_down(&self) -> bool {
        false
    }
    fn right_button_down(&self) -> bool {
        false
    }
    fn context_menu(&self, _: &str, _: (i32, i32)) -> bool {
        false
    }
    fn windows(&self) -> Vec<WindowInfo> {
        Vec::new()
    }
    fn taskbar_shelves(&self) -> Vec<PixelRect> {
        Vec::new()
    }
    fn time_zone_id(&self) -> String {
        self.time_zone.clone()
    }
    fn window_corner(&self) -> f64 {
        0.0
    }
    fn utc_offset_s(&self) -> i32 {
        (self.utc_offset)()
    }
    /// Unknown: never counts as a start at login.
    fn uptime_ms(&self) -> u64 {
        u64::MAX
    }
    fn over_own(&self, _: (i32, i32)) -> bool {
        false
    }
    fn hooks(&self, _: Sender<HookEvent>) -> Option<Guard> {
        None
    }
    fn has_geometry(&self) -> bool {
        false
    }
    fn shelves_reader(&self) -> Box<dyn FnOnce() -> Vec<PixelRect> + Send> {
        Box::new(Vec::new)
    }
    /// No lock: every launch is a first one.
    fn single_instance(&self, _: bool) -> Instance {
        Instance::First(Box::new(NullGuard))
    }
}

impl Shell for NullPlatform {
    fn open_url(&self, _: &str) {}
    fn login_is_on(&self) -> bool {
        false
    }
    fn login_blocked(&self) -> bool {
        false
    }
    fn login_set(&self, _: bool) -> bool {
        false
    }
    fn show_startup_apps(&self) {}
    fn taskbar_light(&self) -> bool {
        false
    }
    fn tray_icon_promoted(&self) -> bool {
        true
    }
    fn tray_icon_spot(&self, _: PixelRect) -> Option<PixelRect> {
        None
    }
    fn region_latitude(&self) -> Option<f64> {
        None
    }
    fn wallpaper_screens(&self) -> Vec<WallpaperScreen> {
        Vec::new()
    }
    fn wallpaper_set(&self, _: &Path) -> bool {
        false
    }
    fn wallpaper_set_each(&self, _: &[(String, PathBuf)]) -> bool {
        false
    }
    fn wallpaper_current(&self) -> Option<PathBuf> {
        None
    }
    fn wallpaper_supported(&self) -> bool {
        false
    }
    fn open_folder(&self, _: &Path) {}
}

impl SystemEvents for NullPlatform {
    fn watch(&self, _: Box<dyn FnMut(SystemEvent, Option<bool>)>) -> Option<Guard> {
        None
    }
}

impl Driver for NullPlatform {
    fn start(&self, _: u32, _: Box<dyn FnMut()>) -> Option<Guard> {
        None
    }
}

/// Sprite backend whose windows are bookkeeping only.
#[derive(Default)]
pub struct NullSprites {
    next: isize,
}

impl SpriteBackend for NullSprites {
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

/// Overlay backend of a platform without panels: every panel fails.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullOverlay;

impl OverlayBackend for NullOverlay {
    const BACKENDS: wgpu::Backends = NATIVE_BACKENDS;
    fn alpha_mode(&self, caps: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode {
        pick_alpha(&caps.alpha_modes)
    }
    fn create_panel(
        &self,
        _: &wgpu::Instance,
        _: PixelRect,
        _: (u32, u32),
        _: f64,
    ) -> Result<Box<dyn PanelWindow>, String> {
        Err(OVERLAY_UNAVAILABLE.into())
    }
    fn raise_own(&self, _: &[isize], _: &[isize]) {}
    fn take_rescaled(&self) -> bool {
        false
    }
    fn start_ticker(&self, _: u32, _: Box<dyn FnMut()>) -> Option<Guard> {
        None
    }
    fn cursor_px() -> Option<(i32, i32)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nop() {}

    fn nop_event(_: SystemEvent, _: Option<bool>) {}

    fn solid_pm(w: u32, h: u32) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        p.fill(tiny_skia::Color::from_rgba8(255, 255, 255, 255));
        p
    }

    #[test]
    fn null_backend_is_inert() {
        let mut b = NullSprites::default();
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

    #[test]
    fn null_platform_reads() {
        let p = NullPlatform::new(PathBuf::from("data"));
        assert!(p.monitors().is_empty());
        assert_eq!(p.data_dir(), PathBuf::from("data"));
        assert_eq!(p.os_language(), "en");
        assert_eq!(p.time_zone_id(), "UTC");
        assert_eq!(p.power(), PowerStatus::default());
        assert!(!p.reduced_motion());
        assert!(p.cursor().is_none());
        assert!(!p.left_button_down() && !p.right_button_down());
        assert!(!p.context_menu("x", (0, 0)));
        assert!(p.windows().is_empty());
        assert!(p.taskbar_shelves().is_empty());
        assert_eq!(p.window_corner(), 0.0);
        assert_eq!(p.utc_offset_s(), 0);
        assert_eq!(p.uptime_ms(), u64::MAX);
        assert!(!p.over_own((1, 1)));
        assert!(p.hooks(std::sync::mpsc::channel().0).is_none());
        assert!(!p.has_geometry());
        assert!(p.shelves_reader()().is_empty());
        p.init_process();
        p.single_instance(false).first().expect("first").listen(Box::new(nop));
        assert!(Instance::Second.first().is_none());
        let q = NullPlatform { utc_offset: || 7200, ..p.clone() };
        assert_eq!(q.utc_offset_s(), 7200);
        assert!(format!("{q:?}").contains("UTC"));
    }

    #[test]
    fn null_shell_is_inert() {
        let p = NullPlatform::new(PathBuf::new());
        p.open_url("https://example.org");
        p.open_folder(Path::new("."));
        p.show_startup_apps();
        assert!(!p.login_is_on() && !p.login_blocked() && !p.login_set(true));
        assert!(!p.taskbar_light());
        assert!(p.tray_icon_promoted());
        assert!(p.tray_icon_spot(PixelRect::new(0, 0, 1, 1)).is_none());
        assert!(p.region_latitude().is_none());
        assert!(p.wallpaper_screens().is_empty());
        assert!(!p.wallpaper_set(Path::new("a.png")));
        assert!(!p.wallpaper_set_each(&[]));
        assert!(p.wallpaper_current().is_none());
        assert!(!p.wallpaper_supported());
        assert!(p.watch(Box::new(nop_event)).is_none());
        assert!(p.start(10, Box::new(nop)).is_none());
        nop();
        nop_event(SystemEvent::Locked, None);
    }

    #[test]
    fn null_overlay_has_no_panels() {
        let o = NullOverlay;
        let caps = wgpu::SurfaceCapabilities {
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque, wgpu::CompositeAlphaMode::PostMultiplied],
            ..Default::default()
        };
        assert_eq!(o.alpha_mode(&caps), wgpu::CompositeAlphaMode::PostMultiplied);
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::empty();
        let inst = wgpu::Instance::new(desc);
        let err = o.create_panel(&inst, PixelRect::new(0, 0, 10, 10), (10, 10), 1.0).err();
        assert_eq!(err.as_deref(), Some(OVERLAY_UNAVAILABLE));
        o.raise_own(&[1], &[]);
        assert!(!o.take_rescaled());
        assert!(o.start_ticker(50, Box::new(nop)).is_none());
        assert!(NullOverlay::cursor_px().is_none());
        NullOverlay::raise_priority();
        assert_eq!(NullOverlay::BACKENDS, NATIVE_BACKENDS);
        let _ = format!("{:?}", NullOverlay);
    }
}
