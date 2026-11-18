//! Headless frames through the main window and the cards: layout code runs,
//! clicks produce the right actions.

use crate::model::{DisplayInfo, Pane, Tab};
use crate::onboarding as ob;
use crate::shell::{HostInfo, Shell, UiAction, UpdateLine};
use crate::{TrayAction, wallpapers};
use eframe::egui::{self, Context, Event, Modifiers, PointerButton, RawInput, pos2, vec2};
use fp_core::clock::WallTime;
use fp_core::effect::{EffectKind, Lang, Visitor};
use fp_core::settings::{Settings, Weathers};

struct Rig {
    ctx: Context,
    shell: Shell,
    w: Weathers,
    size: (f32, f32),
    t: f64,
    lang: Lang,
}

fn host() -> HostInfo {
    HostInfo {
        has_battery: true,
        displays: vec![
            DisplayInfo { id: "d1".into(), name: "Monitor 1".into() },
            DisplayInfo { id: "d2".into(), name: String::new() },
        ],
        ..HostInfo::default()
    }
}

impl Rig {
    fn new(seen_welcome: bool) -> Self {
        let s = Settings { seen_welcome, ..Settings::default() };
        let size = if seen_welcome { (1078.0, 735.0) } else { (860.0, 620.0) };
        Self {
            ctx: Context::default(),
            shell: Shell::new(host()),
            w: Weathers::new(s, false),
            size,
            t: 0.0,
            lang: Lang::En,
        }
    }

    fn input(&mut self, events: Vec<Event>) -> RawInput {
        self.t += 0.05;
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(self.size.0, self.size.1))),
            time: Some(self.t),
            events,
            ..RawInput::default()
        }
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<UiAction> {
        let input = self.input(events);
        let (shell, w, lang) = (&mut self.shell, &self.w, self.lang);
        let mut acts = Vec::new();
        let mut out = self.ctx.run_ui(input, |ui| {
            acts = shell.ui(ui, w, lang, WallTime::new(1_700_000_000, 3600));
        });
        out.textures_delta.clear();
        acts
    }

    /// Settle animations (a few quiet frames).
    fn settle(&mut self) -> Vec<UiAction> {
        let mut all = Vec::new();
        for _ in 0..6 {
            all.extend(self.frame(vec![]));
        }
        all
    }

    fn click(&mut self, x: f32, y: f32) -> Vec<UiAction> {
        let pos = pos2(x, y);
        let mut all = self.frame(vec![Event::PointerMoved(pos)]);
        all.extend(self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]));
        all.extend(self.frame(vec![Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]));
        all.extend(self.frame(vec![]));
        all
    }
}

#[test]
fn every_tab_and_pane_draws() {
    let mut r = Rig::new(true);
    r.settle();
    for tab in Tab::ALL {
        r.shell.show_tab(tab);
        r.settle();
    }
    r.shell.show_tab(Tab::Settings);
    for pane in [Pane::General, Pane::Screens, Pane::About, Pane::Help] {
        r.shell.pane = pane;
        r.settle();
    }
    // Every language draws too.
    for l in Lang::ALL {
        r.lang = l;
        r.shell.pane = Pane::General;
        r.settle();
    }
}

#[test]
fn changelog_tab_clears_the_badge_once() {
    let mut r = Rig::new(true);
    r.settle();
    assert!(r.shell.badge);
    r.shell.show_tab(Tab::Changelog);
    // The action is queued when the tab is picked and returned by the next frame.
    let a = r.settle();
    assert_eq!(a.iter().filter(|a| matches!(a, UiAction::SeenChangelog(_))).count(), 1);
    assert!(!r.shell.badge);
    // Picking again adds nothing.
    r.shell.show_tab(Tab::Effects);
    r.shell.show_tab(Tab::Changelog);
    assert!(r.settle().iter().all(|a| !matches!(a, UiAction::SeenChangelog(_))));
}

#[test]
fn pick_tab_queues_seen_changelog() {
    let mut r = Rig::new(true);
    r.shell.pick_tab(Tab::Changelog);
    let a = r.frame(vec![]);
    assert!(a.contains(&UiAction::SeenChangelog(crate::changelog::latest().to_string())));
    // Re-selecting the current tab does nothing.
    r.shell.pick_tab(Tab::Changelog);
    assert!(r.frame(vec![]).is_empty());
}

#[test]
fn onboarding_runs_from_language_to_done() {
    let mut r = Rig::new(false);
    let first = r.settle();
    assert!(first.contains(&UiAction::SetWeather(None)), "{first:?}");
    assert_eq!(r.shell.step(), ob::LANGUAGE);
    // Primary button, bottom right of the footer.
    let (px, py) = (780.0, 568.0);
    let a = r.click(px, py);
    assert!(a.contains(&UiAction::SetLanguage(Lang::En)), "{a:?}");
    assert_eq!(r.shell.step(), ob::WELCOME);
    let mut weathers = Vec::new();
    for _ in 0..(ob::DONE - ob::WELCOME) {
        let a = r.click(px, py);
        weathers.extend(a.into_iter().filter_map(|a| if let UiAction::SetWeather(w) = a { Some(w) } else { None }));
        r.settle();
    }
    // Effect steps ran their weathers in order, snow on done.
    assert_eq!(weathers.first(), Some(&Some(EffectKind::Snow)));
    assert_eq!(weathers.last(), Some(&Some(EffectKind::Snow)));
    assert!(weathers.contains(&Some(EffectKind::Starfall)));
    assert_eq!(r.shell.step(), ob::DONE);
    // Back is hidden on done; finishing emits FinishOnboarding.
    let a = r.click(px, py);
    assert!(a.contains(&UiAction::FinishOnboarding), "{a:?}");
}

#[test]
fn leaving_onboarding_early_clears_the_weather() {
    let mut r = Rig::new(false);
    r.settle();
    r.click(780.0, 568.0);
    r.click(780.0, 568.0);
    assert_eq!(r.shell.step(), ob::FIRST_EFFECT);
    r.settle();
    let a = r.shell.on_hide(false);
    assert_eq!(a, vec![UiAction::SetWeather(None)]);
    assert_eq!(r.shell.step(), ob::LANGUAGE);
    // After finishing nothing is cleared.
    assert!(r.shell.on_hide(true).is_empty());
}

#[test]
fn effects_tile_click_toggles_the_weather() {
    let mut r = Rig::new(true);
    r.settle();
    let a = r.click(150.0, 290.0);
    assert!(a.contains(&UiAction::Tray(TrayAction::Toggle(EffectKind::Snow))), "{a:?}");
}

#[test]
fn turn_off_button_appears_with_a_weather() {
    let mut r = Rig::new(true);
    r.w.set_weather(Some(EffectKind::Rain));
    r.settle();
    // Top right of the effects page.
    let a = r.click(1078.0 - 24.0 - 60.0, 140.0);
    assert!(a.contains(&UiAction::Tray(TrayAction::Off)), "{a:?}");
}

#[test]
fn caption_buttons_hide_and_ctrl_keys_work() {
    let mut r = Rig::new(true);
    r.settle();
    // Close button, top right.
    let a = r.click(1078.0 - 23.0, 15.0);
    assert!(a.contains(&UiAction::HideWindow), "{a:?}");
    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    let key = |k| Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: ctrl };
    let a = r.frame(vec![Event::ModifiersChanged(ctrl), key(egui::Key::Q)]);
    assert!(a.contains(&UiAction::Quit), "{a:?}");
    let a = r.frame(vec![key(egui::Key::W)]);
    assert!(a.contains(&UiAction::HideWindow), "{a:?}");
}

#[test]
fn cycle_stopped_drops_only_a_cycle_pick() {
    let mut r = Rig::new(true);
    r.shell.wallpaper_result(Some("winter-day".into()), true, r.t);
    assert_eq!(r.shell.picked_id(), Some("winter-day"));
    r.shell.cycle_stopped();
    assert_eq!(r.shell.picked_id(), None);
    r.shell.wallpaper_result(Some("winter-night".into()), true, r.t);
    r.shell.cycle_stopped();
    assert_eq!(r.shell.picked_id(), Some("winter-night"));
}

#[test]
fn host_replies_update_state() {
    let mut r = Rig::new(true);
    r.settle();
    r.shell.toast("hello", r.t);
    r.settle();
    r.shell.wallpaper_result(Some("winter".into()), true, r.t);
    assert!(r.shell.picked.ring("winter"));
    r.shell.wallpaper_result(Some("rain".into()), false, r.t);
    assert!(!r.shell.picked.ring("rain"));
    r.shell.pointer_result(false);
    assert!(r.shell.pointer_warning);
    r.shell.pointer_result(true);
    assert!(!r.shell.pointer_warning);
    r.shell.login_failed(true);
    assert!(!r.shell.login_on && r.shell.login_note.is_some());
    r.shell.show_tab(Tab::Settings);
    r.shell.show_pane(Pane::General);
    assert!(r.shell.login_recheck_wanted());
    r.shell.show_pane(Pane::Screens);
    assert!(!r.shell.login_recheck_wanted());
    r.shell.show_pane(Pane::General);
    r.shell.show_tab(Tab::HowTo);
    assert!(!r.shell.login_recheck_wanted());
    r.shell.login_failed(false);
    r.shell.show_tab(Tab::Settings);
    assert!(!r.shell.login_recheck_wanted());
    r.shell.login_state(true);
    assert!(r.shell.login_on && r.shell.login_note.is_none());
    r.shell.show_tab(Tab::HowTo);
    r.shell.pointer_result(false);
    r.settle();
    r.shell.update_result(UpdateLine::Available { version: "9.9.9".into(), url: "https://example.org".into() });
    r.settle();
    r.shell.update_result(UpdateLine::UpToDate { at: r.t });
    r.settle();
    assert!(matches!(r.shell.update_line(), UpdateLine::UpToDate { .. }));
    r.t += 5.0;
    r.settle();
    assert_eq!(r.shell.update_line(), &UpdateLine::Idle);
    r.shell.update_result(UpdateLine::Offline { at: r.t });
    r.settle();
    r.shell.update_result(UpdateLine::Checking);
    r.settle();
    r.shell.confirm("t", "m", "ok", UiAction::Quit);
    assert!(r.shell.has_sheet());
    r.settle();
    r.shell.city_results("x", Ok(vec![]));
    assert_eq!(r.shell.host().version, env!("CARGO_PKG_VERSION"));
    r.shell.set_host(HostInfo { login_on: true, ..host() });
    assert!(r.shell.login_on);
    assert_eq!(r.shell.tab(), Tab::HowTo);
    assert_eq!(r.shell.pane(), Pane::General);
}

#[test]
fn desired_window_sizes() {
    let r = Rig::new(true);
    assert_eq!(r.shell.desired_size(false, 1000.0), (860.0, 620.0));
    assert_eq!(r.shell.desired_size(true, 1000.0), (1078.0, 735.0));
    assert_eq!(r.shell.desired_size(true, 500.0), (1078.0, 450.0));
}

#[test]
fn sheet_confirm_and_cancel() {
    let mut r = Rig::new(true);
    r.settle();
    r.shell.confirm("Title", "Message", "Do it", UiAction::Quit);
    r.settle();
    // Escape cancels.
    let esc = Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    let a = r.frame(vec![esc]);
    assert!(!r.shell.has_sheet());
    assert!(!a.contains(&UiAction::Quit));
}

#[test]
fn wallpapers_tab_uses_the_decoder_and_papers() {
    let dir = std::env::temp_dir().join(format!("fp_ui_walls_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for f in ["winter.jpg", "winter-dawn.jpg", "rain.jpg"] {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
    let mut r = Rig::new(true);
    let decoder: crate::Decoder = std::sync::Arc::new(|_, _| Some((2, 2, vec![255u8; 16])));
    r.shell.set_host(HostInfo { wallpapers_dir: dir.clone(), decoder: Some(decoder), ..host() });
    r.shell.show_tab(Tab::Wallpapers);
    r.settle();
    assert_eq!(r.shell.papers.len(), 2);
    assert!(r.shell.papers[0].daylong);
    // First tile click picks the main picture of the first pack.
    let a = r.click(150.0, 290.0);
    assert!(a.contains(&UiAction::Wallpaper(wallpapers::WallAction::Main { pack: "winter".into() })), "{a:?}");
    // Open the time card, then draw it.
    r.shell.time_card = Some(crate::shell::TimeCard { pack: "winter", anchor: pos2(600.0, 300.0), opened: r.t - 1.0 });
    r.settle();
    r.shell.picked.set("winter-day".into());
    r.settle();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn settings_toggles_emit_actions() {
    let mut r = Rig::new(true);
    r.shell.show_tab(Tab::Settings);
    r.shell.pane = Pane::General;
    r.settle();
    // Drive the switch rows by emitting through the public helpers.
    r.shell.emit(UiAction::SetVisitor(Visitor::Yeti, false));
    let a = r.frame(vec![]);
    assert_eq!(a, vec![UiAction::SetVisitor(Visitor::Yeti, false)]);
}

#[test]
fn ground_clock_restarts_runs_at_30fps_and_settles() {
    let mut s = Shell::new(host());
    s.now = 50.0;
    assert_eq!(s.ground_clock(), (0.0, 0));
    s.now = 50.1;
    let (t, f) = s.ground_clock();
    assert!((t - 0.1).abs() < 1e-9);
    assert_eq!(f, 3);
    // hidden for a while: restarts
    s.now = 90.0;
    assert_eq!(s.ground_clock(), (0.0, 0));
    s.set_reduce_motion(true);
    s.now = 91.0;
    assert_eq!(s.ground_clock(), (100.0, -1));
}

#[test]
fn size_drift_ignores_rounding_only() {
    use crate::model::size_drifted;
    assert!(!size_drifted((1078.0, 735.0), (1078.0, 735.0)));
    // Physical pixels rounded at 175 %: under a point off.
    assert!(!size_drifted((1077.7, 735.4), (1078.0, 735.0)));
    // Shrunk by the scale (dragged onto a 175 % screen) or grown by it.
    assert!(size_drifted((616.0, 420.0), (1078.0, 735.0)));
    assert!(size_drifted((1887.0, 1286.0), (1078.0, 735.0)));
    assert!(size_drifted((1078.0, 700.0), (1078.0, 735.0)));
}

#[test]
fn fit_plan_uses_the_work_area_holding_the_window() {
    let rect = |x0, y0, x1, y1| egui::Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
    let mut r = Rig::new(true);
    let old = Some(rect(100.0, 100.0, 960.0, 720.0));
    let outer = Some(rect(100.0, 90.0, 960.0, 730.0));
    // Short work area: Home is 0.9 of it, not of the monitor.
    r.shell.set_host(HostInfo { work_areas: vec![rect(0.0, 0.0, 1920.0, 700.0)], ..host() });
    let (target, pos) = r.shell.fit_plan(crate::model::Screen::Home, old, outer, 1080.0);
    assert_eq!(target.1, 630.0);
    assert!(pos.is_some());
    // Window on a monitor above the primary keeps its own top clamp.
    let areas = vec![rect(0.0, 0.0, 1920.0, 1040.0), rect(0.0, -1080.0, 1920.0, -40.0)];
    r.shell.set_host(HostInfo { work_areas: areas, ..host() });
    let old = Some(rect(100.0, -1070.0, 900.0, -1000.0));
    let outer = Some(rect(100.0, -1080.0, 900.0, -1000.0));
    let (target, pos) = r.shell.fit_plan(crate::model::Screen::Home, old, outer, 1080.0);
    assert_eq!(target.1, 735.0);
    assert_eq!(pos.unwrap().1, -1080.0);
    // Centre outside every area: primary. No areas, no window: old behaviour.
    let far = Some(rect(5000.0, 0.0, 5800.0, 600.0));
    assert!(r.shell.fit_plan(crate::model::Screen::Home, old, far, 1080.0).1.is_some());
    r.shell.set_host(HostInfo { work_areas: vec![], ..host() });
    let (target, pos) = r.shell.fit_plan(crate::model::Screen::Home, None, None, 600.0);
    assert_eq!(target.1, 540.0);
    assert!(pos.is_none());
    let (_, pos) = r.shell.fit_plan(crate::model::Screen::Onboarding, old, far, 1080.0);
    assert!(pos.is_some());
}

#[test]
fn notices_pick_the_lines_to_show() {
    use crate::Notices;
    use fp_text::Key;
    assert!(Notices::default().keys().is_empty());
    let n = Notices { no_overlay: true, no_window_geometry: true, no_global_cursor: true, ..Notices::default() };
    assert_eq!(n.keys(), vec![Key::NoteNoOverlay, Key::NoteNoCursor]);
    let n = Notices { no_overlay: true, no_compositor: true, ..Notices::default() };
    assert_eq!(n.keys(), vec![Key::NoteNoCompositor]);
    let n = Notices { no_window_geometry: true, partial_windows: true, ..Notices::default() };
    assert_eq!(n.keys(), vec![Key::NoteNoGeometry]);
    let n = Notices { partial_windows: true, ..Notices::default() };
    assert_eq!(n.keys(), vec![Key::NotePartialWindows]);
}
