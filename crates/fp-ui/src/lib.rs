//! Tray icon, tray menu and the egui windows. Not part of the coverage gate.
//!
//! Spec: core-app 2.3 (main window: hide on close, Ctrl+W/Ctrl+Q), 2.6 (tray
//! icon and the full menu layout, rebuilt on every change), 2.7 (tray pointer
//! via `fp-layered`), 2.9 (theme tokens), 2.17 (language picker, own flags);
//! ui 1-3 and 6-8 (Effects, Wallpapers = day-and-night packs, How-to,
//! Settings panes General/Screens/About with Open-Meteo attribution and city
//! search, onboarding, toast, sheet, shared controls); snowman F4 dress-up
//! card, trees 3.9 style card, decorations 3.1 picker, quote card, limit
//! snowman bubble, train station bubble (egui viewports: transparent,
//! undecorated, always on top).
//! Dropped: Friends tab, licence/pricing/account/trust/wish panes, changelog
//! from a server (an own CHANGELOG is optional).

pub use eframe;
use fp_core::effect::{EffectKind, Lang};
use fp_core::settings::Weathers;
use fp_core::wind::Wind;
use fp_text::Key;
use tray_icon::Icon;

pub mod art;
pub mod cardmodel;
pub mod cards;
pub mod changelog;
mod home;
pub mod menu;
pub mod model;
mod onboard;
pub mod onboarding;
mod settings_ui;
pub mod shell;
mod tabs;
pub mod theme;
pub mod tray;
pub mod tween;
pub mod wallpapers;
pub mod widgets;

pub use cards::{CardAction, CardCtx, Cards};
pub use shell::{Decoder, HostInfo, Notices, Shell, UiAction, UpdateLine};
pub use tray::Tray;

#[cfg(test)]
mod shots;
#[cfg(test)]
mod tests;

/// Headless egui frames for tests.
#[cfg(test)]
pub(crate) mod testutil {
    use eframe::egui;

    /// Run one frame with a default context; textures are discarded.
    pub fn frame(ctx: &egui::Context, mut f: impl FnMut(&mut egui::Ui)) {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| f(ui));
        out.textures_delta.clear();
    }
}

/// What a tray click asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrayAction {
    SnowVariant { heavy: bool },
    RainVariant { storm: bool },
    Toggle(EffectKind),
    Wind(Wind),
    ToggleMatchWeather,
    ToggleRandom,
    ToggleSeason,
    Off,
    SendPlough,
    SendSnowmen,
    SendYeti,
    SendSanta,
    SendTrain,
    Decorate,
    Open,
    Quit,
}

/// Temporary tray icon until `fp_paint::icon_rgba` exists: a soft blue disc.
pub fn placeholder_icon() -> Icon {
    let n = 32u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - 15.5, y as f32 - 15.5);
            let a = (14.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            px.extend_from_slice(&[0x59, 0xB2, 0xFF, (a * 255.0) as u8]);
        }
    }
    Icon::from_rgba(px, n, n).expect("valid icon")
}

/// One-time egui setup for the app: transparent panel fill (card viewports
/// clear to it; the main window paints its own background) and the 0.18 s
/// control animation time. Call once from the eframe creation callback.
pub fn install(ctx: &eframe::egui::Context) {
    theme::install_fonts(ctx);
    ctx.all_styles_mut(|s| {
        s.animation_time = 0.18;
        s.visuals.panel_fill = eframe::egui::Color32::TRANSPARENT;
        s.visuals.window_fill = eframe::egui::Color32::TRANSPARENT;
        s.visuals.selection.bg_fill = theme::rgb(theme::ACCENT);
    });
}

/// Viewport settings for the main window (ui 1.1): fixed size, no resize,
/// no native frame when the UI draws its own caption buttons.
pub fn viewport_builder(
    lang: Lang,
    seen_welcome: bool,
    work_height: f32,
    custom_chrome: bool,
    visible: bool,
) -> eframe::egui::ViewportBuilder {
    let (w, h) = match model::pick_screen(seen_welcome) {
        model::Screen::Onboarding => onboarding::WINDOW,
        model::Screen::Home => (model::HOME_WIDTH, model::home_height(work_height)),
    };
    eframe::egui::ViewportBuilder::default()
        .with_title(fp_text::t(lang, Key::AppName))
        .with_inner_size([w, h])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_decorations(!custom_chrome)
        .with_visible(visible)
        .with_icon(art::app_icon_data().unwrap_or_default())
}

/// Shader-visible descriptors the egui device reserves: one per live texture
/// bind group, so this leaves room for thousands of images.
pub const UI_DESCRIPTORS: u32 = 16_384;

/// Renderer settings for the egui windows: DX12 only on Windows (no Vulkan
/// or GL instance next to it), low power adapter preference, small GPU memory
/// blocks and a small descriptor heap. The defaults (`Performance` hint, one
/// million descriptors) commit about 270 MB per device up front, which on an
/// integrated GPU is ordinary process memory. `WGPU_BACKEND` and
/// `WGPU_POWER_PREF` still override.
pub fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    use eframe::wgpu;
    let mut create = WgpuSetupCreateNew::without_display_handle();
    // DX12 on Windows, Metal on macOS, Vulkan with a GL fallback elsewhere.
    let native = if cfg!(windows) {
        wgpu::Backends::DX12
    } else if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else {
        wgpu::Backends::VULKAN | wgpu::Backends::GL
    };
    create.instance_descriptor.backends = wgpu::Backends::from_env().unwrap_or(native);
    create.power_preference = wgpu::PowerPreference::from_env().unwrap_or(wgpu::PowerPreference::LowPower);
    let base = create.device_descriptor.clone();
    create.device_descriptor = std::sync::Arc::new(move |adapter| {
        let d = base(adapter);
        wgpu::DeviceDescriptor {
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            required_limits: wgpu::Limits { max_non_sampler_bindings: UI_DESCRIPTORS, ..d.required_limits.clone() },
            ..d
        }
    });
    WgpuConfiguration { wgpu_setup: WgpuSetup::CreateNew(create), ..Default::default() }
}

/// Stateless compatibility entry for the scaffold app: draws the main
/// window through a private [`Shell`] and returns only the tray-style
/// actions. New code should own a [`Shell`] and handle [`UiAction`]s.
pub fn settings_ui(ui: &mut eframe::egui::Ui, w: &Weathers, lang: Lang) -> Vec<TrayAction> {
    use std::cell::RefCell;
    thread_local! {
        static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
    }
    let wall = fp_core::clock::WallTime::default();
    let actions = SHELL.with(|cell| {
        let mut slot = cell.borrow_mut();
        let shell = slot.get_or_insert_with(|| Shell::new(HostInfo { custom_chrome: false, ..HostInfo::default() }));
        shell.ui(ui, w, lang, wall)
    });
    actions
        .into_iter()
        .filter_map(|a| match a {
            UiAction::Tray(t) => Some(t),
            UiAction::Quit => Some(TrayAction::Quit),
            _ => None,
        })
        .collect()
}
