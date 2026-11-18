//! Smoke run of the Frostpane UI: tray icon, main window (tabs), cards.
//!
//! `cargo run -p fp-ui --example ui_smoke -- --seconds 8 [--onboarding]`
//!
//! The window walks through every tab and pane, opens each card in turn and
//! exits by itself after `--seconds` (default 8).

use eframe::egui;
use fp_core::clock::WallTime;
use fp_core::desktop::{Layout, MonitorInfo};
use fp_core::effect::Lang;
use fp_core::geom::{Frame, PixelRect, Vec2};
use fp_core::settings::{Settings, Weathers};
use fp_ui::model::{DisplayInfo, Pane, Tab};
use fp_ui::{CardCtx, Cards, HostInfo, Shell, Tray, TrayAction, UiAction};
use fp_world::CardRequest;
use std::time::Instant;

struct Smoke {
    shell: Shell,
    cards: Cards,
    tray: Tray,
    weathers: Weathers,
    layout: Layout,
    lang: Lang,
    started: Instant,
    seconds: f64,
    stage: usize,
    frames: u64,
    actions: usize,
    closing: bool,
}

fn layout() -> Layout {
    Layout::new(vec![MonitorInfo {
        id: "smoke".into(),
        name: "Smoke".into(),
        pixels: PixelRect { left: 0, top: 0, right: 1920, bottom: 1080 },
        work: PixelRect { left: 0, top: 0, right: 1920, bottom: 1040 },
        scale: 1.0,
        primary: true,
    }])
}

impl Smoke {
    fn apply(&mut self, a: UiAction) {
        self.actions += 1;
        match a {
            UiAction::Tray(TrayAction::Toggle(e)) => {
                self.weathers.toggle_weather(e);
            }
            UiAction::Tray(TrayAction::Off) => {
                self.weathers.turn_off();
            }
            UiAction::GridVariant { effect, second } => {
                self.weathers.grid_variant(effect, second);
            }
            UiAction::SetLanguage(l) => self.lang = l,
            UiAction::SetWeather(w) => {
                self.weathers.set_weather(w);
            }
            UiAction::FinishOnboarding => self.weathers.settings.seen_welcome = true,
            UiAction::Quit | UiAction::HideWindow => self.closing = true,
            _ => {}
        }
    }

    /// Open one more thing every 0.6 s.
    fn script(&mut self, t: f64, ctx: &egui::Context) {
        let stage = (t / 0.6) as usize;
        if stage == self.stage {
            return;
        }
        self.stage = stage;
        let tabs = [Tab::Wallpapers, Tab::HowTo, Tab::Changelog, Tab::Settings, Tab::Effects];
        if let Some(tab) = tabs.get(stage.wrapping_sub(1)) {
            self.shell.show_tab(*tab);
        }
        let panes = [Pane::Screens, Pane::About, Pane::Help, Pane::General];
        if let Some(p) = stage.checked_sub(5).and_then(|i| panes.get(i)) {
            self.shell.show_tab(Tab::Settings);
            self.shell.show_pane(*p);
        }
        let layout = &self.layout;
        let cx = CardCtx {
            lang: self.lang,
            layout,
            sprites: &[],
            cursor: Vec2::new(900.0, 500.0),
            tree_origin: Some(Vec2::new(700.0, 200.0)),
        };
        match stage {
            4 => self.cards.open(CardRequest::DressUp { bottom: 1, look: None }, &cx, t),
            5 => self.cards.open(
                CardRequest::TreeStyle { tree: 3, look: fp_trees::TreeLook::default(), quip: None },
                &cx,
                t,
            ),
            6 => self.cards.open(CardRequest::Quote { at: Vec2::new(1000.0, 700.0), quote: 0 }, &cx, t),
            7 => self.cards.toggle_picker(&cx, 2, 100, t),
            8 => self.cards.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 1920.0, 1040.0) }, &cx, t),
            9 => self.cards.open_update("9.9.9", "https://example.org/", &cx, t),
            _ => {}
        }
        if stage == 10 {
            self.cards.close_all();
        }
        ctx.request_repaint();
    }
}

impl eframe::App for Smoke {
    fn logic(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        let t = self.started.elapsed().as_secs_f64();
        if self.weathers.settings.seen_welcome {
            self.script(t, ctx);
        }
        for a in self.tray.poll() {
            if a == TrayAction::Quit {
                self.closing = true;
            }
        }
        if t >= self.seconds || self.closing {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        self.frames += 1;
        let acts = self.shell.ui(ui, &self.weathers, self.lang, WallTime::new(1_700_000_000, 0));
        for a in acts {
            self.apply(a);
        }
        let ctx = ui.ctx().clone();
        for a in self.cards.show(&ctx, self.lang) {
            self.actions += 1;
            println!("card action: {a:?}");
        }
    }

    fn on_exit(&mut self) {
        println!(
            "ui_smoke: {} frames, {} actions, exited after {:.1}s",
            self.frames,
            self.actions,
            self.started.elapsed().as_secs_f64()
        );
    }
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let seconds = args
        .iter()
        .position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(8.0);
    let onboarding = args.iter().any(|a| a == "--onboarding");
    let settings = Settings { seen_welcome: !onboarding, ..Settings::default() };
    let weathers = Weathers::new(settings, false);
    let lang = Lang::En;
    let host = HostInfo {
        has_battery: true,
        displays: vec![
            DisplayInfo { id: "a".into(), name: "Monitor A".into() },
            DisplayInfo { id: "b".into(), name: "Monitor B".into() },
        ],
        ..HostInfo::default()
    };
    let opts = eframe::NativeOptions {
        viewport: fp_ui::viewport_builder(lang, !onboarding, 1000.0, true, true).with_position([40.0, 40.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Frostpane smoke",
        opts,
        Box::new(move |cc| {
            fp_ui::install(&cc.egui_ctx);
            let wake = cc.egui_ctx.clone();
            let tray = Tray::new(&weathers, lang, move || wake.request_repaint())?;
            Ok(Box::new(Smoke {
                shell: Shell::new(host),
                cards: Cards::new(7),
                tray,
                weathers,
                layout: layout(),
                lang,
                started: Instant::now(),
                seconds,
                stage: usize::MAX,
                frames: 0,
                actions: 0,
                closing: false,
            }))
        }),
    )
}
