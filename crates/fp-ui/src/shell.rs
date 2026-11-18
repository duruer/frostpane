//! The main window: onboarding or Home (tabs), the shared chrome, toast and
//! sheet. State lives in [`Shell`]; the tabs are in `tabs`, `settings_ui`
//! and `onboard`.

use crate::TrayAction;
use crate::art::{self, Textures};
use crate::cardmodel::{CitySearch, city_label};
use crate::model::{self, DisplayInfo, Glyph, Pane, Screen, Tab};
use crate::onboarding;
use crate::theme::{self, Tokens};
use crate::tween::Toast;
use crate::wallpapers::{Paper, Picked, WallAction};
use crate::widgets;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Ui, ViewportCommand, pos2, vec2};
use fp_core::clock::WallTime;
use fp_core::effect::{EffectKind, Lang, Visitor};
use fp_core::settings::{City, Weathers};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Decodes an image file to straight-alpha RGBA, scaled so its larger side is
/// at most `max_side` pixels: `(width, height, bytes)`. The host provides it
/// (the UI crate has no JPEG decoder).
pub type Decoder = Arc<dyn Fn(&Path, u32) -> Option<(u32, u32, Vec<u8>)> + Send + Sync>;

/// What the host tells the UI about the machine and the build.
#[derive(Clone)]
pub struct HostInfo {
    /// `x.y.z`.
    pub version: String,
    pub has_battery: bool,
    pub displays: Vec<DisplayInfo>,
    /// `%LOCALAPPDATA%\Frostpane\Wallpapers`.
    pub wallpapers_dir: PathBuf,
    /// Pack of the running day-and-night cycle (pref `dayWallpaper`).
    pub day_pack: Option<String>,
    /// Launch-at-login state read from the OS.
    pub login_on: bool,
    /// Issue tracker for the Help pane.
    pub issues_url: String,
    /// Pref `seenChangelog`.
    pub seen_changelog: Option<String>,
    /// Draw our own caption buttons (window has no native decorations).
    pub custom_chrome: bool,
    pub decoder: Option<Decoder>,
    /// Monitor work areas in logical points (work rect / monitor scale), primary first.
    pub work_areas: Vec<Rect>,
}

impl Default for HostInfo {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            has_battery: false,
            displays: Vec::new(),
            wallpapers_dir: PathBuf::new(),
            day_pack: None,
            login_on: false,
            issues_url: "https://github.com/duruer/frostpane/issues".to_string(),
            seen_changelog: None,
            custom_chrome: true,
            decoder: None,
            work_areas: Vec::new(),
        }
    }
}

/// Everything the UI asks the app to do. The UI never changes settings
/// itself; the app applies the action to `Weathers`, saves and refreshes.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    /// Tray-equivalent actions (effects tab tile click, "turn off effects").
    Tray(TrayAction),
    /// Second segment of the snow or rain pill (`Weathers::grid_variant`).
    GridVariant {
        effect: EffectKind,
        second: bool,
    },
    SetLanguage(Lang),
    SetSnowballs(bool),
    SetBehindWindows(bool),
    SetKeepClear(bool),
    SetOftenVisits(bool),
    SetPauseOnBattery(bool),
    SetVisitor(Visitor, bool),
    SetScreen {
        id: String,
        on: bool,
    },
    SetMainScreen(String),
    SetLaunchAtLogin(bool),
    /// Live weather of the onboarding steps (`Weathers::set_weather`).
    SetWeather(Option<EffectKind>),
    /// Onboarding finished: `seenWelcome = true`.
    FinishOnboarding,
    /// Apply a wallpaper (the host copies the file and calls the OS), then
    /// reports with [`Shell::wallpaper_result`].
    Wallpaper(WallAction),
    /// The "with <weather>" link: apply, then set the weather even on failure.
    WallpaperWith {
        action: WallAction,
        effect: EffectKind,
    },
    SearchCity(String),
    SetCity(Option<City>),
    CheckUpdates,
    OpenUrl(String),
    /// Reveal the wallpapers folder in the file manager.
    OpenWallpapersFolder,
    /// Point at the tray icon (answer with [`Shell::pointer_result`]).
    ShowTrayPointer,
    /// `seenChangelog = number`.
    SeenChangelog(String),
    /// The window close button: hide to the tray.
    HideWindow,
    Quit,
}

/// State of the update line next to the tabs.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateLine {
    Idle,
    Checking,
    UpToDate { at: f64 },
    Offline { at: f64 },
    Available { version: String, url: String },
}

/// Seconds a result stays before the button comes back (ui 2.3).
pub const UPDATE_RESULT_SHOWN: f64 = 4.0;

/// A confirm sheet (ui 3.8).
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub title: String,
    pub message: String,
    pub confirm: String,
    pub action: UiAction,
}

/// The wallpapers time card (popup under a pack's day-and-night pill).
#[derive(Clone, Debug, PartialEq)]
pub struct TimeCard {
    pub pack: &'static str,
    pub anchor: Pos2,
    /// Egui time the card opened (the opening click must not close it).
    pub opened: f64,
}

pub struct Shell {
    pub(crate) host: HostInfo,
    pub(crate) tex: Textures,
    pub(crate) tk: Tokens,
    pub(crate) lang: Lang,
    pub(crate) now: f64,
    pub(crate) wall: WallTime,
    pub(crate) out: Vec<UiAction>,
    pub(crate) tab: Tab,
    pub(crate) pane: Pane,
    pub(crate) papers: Vec<Paper>,
    pub(crate) papers_scanned: bool,
    pub(crate) picked: Picked,
    pub(crate) time_card: Option<TimeCard>,
    pub(crate) lang_popup: Option<Pos2>,
    pub(crate) toast: Toast,
    pub(crate) step: usize,
    pub(crate) live: Option<Option<EffectKind>>,
    pub(crate) city: CitySearch,
    pub(crate) login_on: bool,
    pub(crate) login_note: Option<LoginNote>,
    pub(crate) update: UpdateLine,
    pub(crate) pointer_warning: bool,
    pub(crate) sheet: Option<Sheet>,
    pub(crate) screen: Option<Screen>,
    /// Inner size (points) the window was last fitted to; the window is not
    /// resizable, so any other size is drift (see [`Shell::keep_size`]).
    pub(crate) fitted: Option<(f32, f32)>,
    pub(crate) badge: bool,
    pub(crate) tab_pill: Option<(f32, f32)>,
    pub(crate) groundlife: art::Animated,
    /// Egui time the ground animation (re)started; None until shown.
    pub(crate) ground_started: Option<f64>,
    /// Egui time the ground was last drawn (detects hidden gaps).
    pub(crate) ground_seen: f64,
    /// OS animations off: ground life is drawn once, settled.
    pub(crate) reduce_motion: bool,
}

/// Note under the open-at-login row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginNote {
    Blocked,
    Failed,
}

impl Shell {
    pub fn new(host: HostInfo) -> Self {
        let picked = Picked::initial(host.day_pack.as_deref());
        let login_on = host.login_on;
        let badge = crate::changelog::unseen(host.seen_changelog.as_deref());
        Self {
            host,
            tex: Textures::new(),
            tk: Tokens::new(true),
            lang: Lang::En,
            now: 0.0,
            wall: WallTime::default(),
            out: Vec::new(),
            tab: Tab::Effects,
            pane: Pane::General,
            papers: Vec::new(),
            papers_scanned: false,
            picked,
            time_card: None,
            lang_popup: None,
            toast: Toast::new(),
            step: onboarding::LANGUAGE,
            live: None,
            city: CitySearch::default(),
            login_on,
            login_note: None,
            update: UpdateLine::Idle,
            pointer_warning: false,
            sheet: None,
            screen: None,
            fitted: None,
            badge,
            tab_pill: None,
            groundlife: art::Animated::default(),
            ground_started: None,
            ground_seen: 0.0,
            reduce_motion: false,
        }
    }

    /// OS reduced-motion preference (other-weather 2.5 static pose).
    pub fn set_reduce_motion(&mut self, on: bool) {
        self.reduce_motion = on;
    }

    // ---- host -> UI ----

    /// Replace the host facts (displays changed, battery found, ...).
    pub fn set_host(&mut self, host: HostInfo) {
        self.login_on = host.login_on;
        self.host = host;
        self.papers_scanned = false;
    }

    pub fn host(&self) -> &HostInfo {
        &self.host
    }

    /// Show a toast (ui 3.7) at time `now` (egui time).
    pub fn toast(&mut self, text: &str, now: f64) {
        self.toast.show(text, now);
    }

    /// Open a tab (the static `Show(tab)` of ui 1.2).
    pub fn show_tab(&mut self, tab: Tab) {
        self.pick_tab(tab);
    }

    /// Open a settings pane (the Settings tab shows it).
    pub fn show_pane(&mut self, pane: Pane) {
        self.pane = pane;
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn pane(&self) -> Pane {
        self.pane
    }

    pub fn step(&self) -> usize {
        self.step
    }

    /// The day cycle stopped outside the tab (other-weather 2.6 Stop): drop a cycle pick.
    pub fn cycle_stopped(&mut self) {
        if self.picked.0.as_deref().is_some_and(|p| p.ends_with("-day")) {
            self.picked.0 = None;
        }
    }

    /// Id of the picked wallpaper entry (ring), if any.
    pub fn picked_id(&self) -> Option<&str> {
        self.picked.0.as_deref()
    }

    /// Reply to [`UiAction::Wallpaper`]: on success the ring moves to
    /// `picked`; a toast reports the result either way.
    pub fn wallpaper_result(&mut self, picked: Option<String>, ok: bool, now: f64) {
        let key = if ok { fp_text::Key::WallpaperSet } else { fp_text::Key::WallpaperFailed };
        if ok && let Some(p) = picked {
            self.picked.set(p);
        }
        self.toast.show(fp_text::t(self.lang, key), now);
    }

    /// Reply to [`UiAction::ShowTrayPointer`]: false = the icon is hidden in
    /// the overflow, so show the warning (ui 3.3).
    pub fn pointer_result(&mut self, shown: bool) {
        self.pointer_warning = !shown;
    }

    /// Reply to [`UiAction::SearchCity`].
    pub fn city_results(&mut self, query: &str, result: Result<Vec<City>, ()>) {
        self.city.finish(query, result);
    }

    /// Reply to [`UiAction::SetLaunchAtLogin`] when Windows blocked it (the
    /// switch flips back and a note appears) or it failed otherwise.
    pub fn login_failed(&mut self, blocked: bool) {
        self.login_on = false;
        self.login_note = Some(if blocked { LoginNote::Blocked } else { LoginNote::Failed });
    }

    /// The OS state of launch-at-login changed (re-check on activation).
    pub fn login_state(&mut self, on: bool) {
        self.login_on = on;
        if on {
            self.login_note = None;
        }
    }

    /// True while the blocked note is visible on the General pane, so the
    /// host re-reads the OS state when the window is activated (ui 3.5 Hooks).
    pub fn login_recheck_wanted(&self) -> bool {
        self.tab == Tab::Settings && self.pane == Pane::General && self.login_note == Some(LoginNote::Blocked)
    }

    /// Reply to [`UiAction::CheckUpdates`].
    pub fn update_result(&mut self, line: UpdateLine) {
        self.update = line;
    }

    pub fn update_line(&self) -> &UpdateLine {
        &self.update
    }

    /// The window is hidden or minimised: reset what is not remembered and
    /// return the actions to apply (ui 3.6: live weather clears before
    /// onboarding finished). Onboarding restarts at step 0 next time.
    pub fn on_hide(&mut self, seen_welcome: bool) -> Vec<UiAction> {
        let mut out = Vec::new();
        if !seen_welcome {
            if onboarding::clears_on_leave(false) && self.live.flatten().is_some() {
                out.push(UiAction::SetWeather(None));
            }
            self.step = onboarding::LANGUAGE;
            self.live = None;
        }
        self.time_card = None;
        self.lang_popup = None;
        self.sheet = None;
        self.screen = None;
        out
    }

    /// Window size for the current screen on a work area of `work_height`.
    pub fn desired_size(&self, seen_welcome: bool, work_height: f32) -> (f32, f32) {
        match model::pick_screen(seen_welcome) {
            Screen::Onboarding => onboarding::WINDOW,
            Screen::Home => (model::HOME_WIDTH, model::home_height(work_height)),
        }
    }

    // ---- state helpers ----

    pub(crate) fn emit(&mut self, a: UiAction) {
        self.out.push(a);
    }

    /// Switch tab: rebuild-on-switch semantics, clears the changelog badge.
    pub(crate) fn pick_tab(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        self.time_card = None;
        if tab == Tab::Changelog && self.badge {
            self.badge = false;
            self.emit(UiAction::SeenChangelog(crate::changelog::latest().to_string()));
        }
    }

    pub(crate) fn rescan_papers(&mut self) {
        let dir = self.host.wallpapers_dir.clone();
        self.papers = crate::wallpapers::scan(|f| !dir.as_os_str().is_empty() && dir.join(f).is_file());
        self.papers_scanned = true;
        self.tex.forget_failures();
    }

    pub(crate) fn t(&self, k: fp_text::Key) -> &'static str {
        fp_text::t(self.lang, k)
    }

    // ---- frame ----

    /// Draw the window contents for this frame and return the actions the
    /// app must apply. Call from `eframe::App::ui` while the window is shown.
    pub fn ui(&mut self, ui: &mut Ui, w: &Weathers, lang: Lang, wall: WallTime) -> Vec<UiAction> {
        let ctx = ui.ctx().clone();
        self.lang = lang;
        self.wall = wall;
        self.now = ctx.input(|i| i.time);
        if !self.papers_scanned {
            self.rescan_papers();
        }
        let screen = model::pick_screen(w.settings.seen_welcome);
        if self.screen != Some(screen) {
            if self.screen.is_some() || screen == Screen::Home {
                self.fit_window(&ctx, screen);
            }
            self.screen = Some(screen);
        }
        self.keep_size(&ctx, screen);
        self.tk = Tokens::new(model::screen_is_dark(screen));
        // Ctrl+W hides, Ctrl+Q quits (exactly Ctrl held).
        let (w_key, q_key) = ctx.input(|i| {
            let only = i.modifiers.command_only();
            (only && i.key_pressed(egui::Key::W), only && i.key_pressed(egui::Key::Q))
        });
        if w_key {
            self.emit(UiAction::HideWindow);
        }
        if q_key {
            self.emit(UiAction::Quit);
        }
        let frame = egui::Frame::new().fill(self.tk.window());
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            match screen {
                Screen::Onboarding => self.onboarding_ui(ui, w),
                Screen::Home => self.home_ui(ui, w),
            }
            if self.host.custom_chrome {
                self.caption_buttons(ui);
            }
            self.toast_ui(ui);
            self.sheet_ui(ui);
        });
        if self.toast.active(self.now) {
            ctx.request_repaint();
        }
        std::mem::take(&mut self.out)
    }

    /// Resize between screens keeping the centre (ui 1.1 "Fit").
    fn fit_window(&mut self, ctx: &egui::Context, screen: Screen) {
        let monitor_h = ctx.input(|i| i.viewport().monitor_size.map_or(1080.0, |m| m.y));
        let (old, outer) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect));
        let (target, pos) = self.fit_plan(screen, old, outer, monitor_h);
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(target.0, target.1)));
        self.fitted = Some(target);
        if let Some((x, y)) = pos {
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(x, y)));
        }
    }

    /// Put the fixed window size back after it drifted. Dragging the window
    /// between monitors with different scaling can leave it resized by the
    /// wrong factor (shrunk or doubled), because the scale change and our
    /// fixed size disagree mid-drag. Once the drag is over (no button held),
    /// the size the window was fitted to is requested again at the current
    /// scale.
    fn keep_size(&mut self, ctx: &egui::Context, screen: Screen) {
        let want = self.fitted.or((screen == Screen::Onboarding).then_some(onboarding::WINDOW));
        let (inner, held) = ctx.input(|i| (i.viewport().inner_rect, i.pointer.any_down()));
        if let (Some(want), Some(inner)) = (want, inner)
            && !held
            && model::size_drifted((inner.width(), inner.height()), want)
        {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(want.0, want.1)));
        }
    }

    /// New inner size and outer position for a screen change. The work area
    /// is the one holding the window centre, else the primary, else the whole
    /// monitor height from the top.
    pub(crate) fn fit_plan(
        &self,
        screen: Screen,
        old: Option<Rect>,
        outer: Option<Rect>,
        monitor_h: f32,
    ) -> ((f32, f32), Option<(f32, f32)>) {
        let wa = outer
            .and_then(|o| self.host.work_areas.iter().find(|r| r.contains(o.center())).or(self.host.work_areas.first()))
            .or(self.host.work_areas.first())
            .copied();
        let (work_h, work_top) = wa.map_or((monitor_h, 0.0), |r| (r.height(), r.min.y));
        let target = match screen {
            Screen::Onboarding => onboarding::WINDOW,
            Screen::Home => (model::HOME_WIDTH, model::home_height(work_h)),
        };
        let pos = match (old, outer) {
            (Some(o), Some(out)) => Some(model::fit((out.min.x, out.min.y), (o.width(), o.height()), target, work_top)),
            _ => None,
        };
        (target, pos)
    }

    // ---- chrome ----

    /// Minimise and close buttons, 46 x 30 each, top right (ui 1.1).
    fn caption_buttons(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        let tk = self.tk;
        // Effect cards: controls follow the card's top-edge luminance (light
        // glyphs on dark skies, ink on pale ones like sakura).
        let card = if self.screen == Some(Screen::Onboarding) { onboarding::effect_of(self.step) } else { None };
        let on_card = card.is_some();
        let light_top = card.is_some_and(|e| {
            let c = crate::art::tile_sky(e).0;
            let (r, g, b) = ((c >> 16 & 0xFF) as f32, (c >> 8 & 0xFF) as f32, (c & 0xFF) as f32);
            (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 > 0.6
        });
        // Drag area: the top 30 px, minus the buttons.
        let drag = Rect::from_min_max(full.min, pos2(full.right() - 92.0, full.top() + 30.0));
        let r = ui.interact(drag, egui::Id::unique("fp_drag"), Sense::click_and_drag());
        if r.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        for (i, g) in [Glyph::Minimise, Glyph::Close].into_iter().enumerate() {
            let rect = Rect::from_min_size(pos2(full.right() - 92.0 + 46.0 * i as f32, full.top()), vec2(46.0, 30.0));
            let resp = ui.interact(rect, egui::Id::unique(("fp_caption", i)), Sense::click());
            let close = g == Glyph::Close;
            let (hover_bg, down_bg) = if on_card && light_top {
                (Color32::from_black_alpha(theme::alpha_byte(0.12)), Color32::from_black_alpha(theme::alpha_byte(0.22)))
            } else if on_card {
                (Color32::from_white_alpha(theme::alpha_byte(0.18)), Color32::from_white_alpha(theme::alpha_byte(0.28)))
            } else {
                (tk.ink10(), tk.ink16())
            };
            let idle_fg =
                if on_card && !light_top { Color32::from_white_alpha(theme::alpha_byte(0.85)) } else { tk.ink() };
            let (bg, fg) = if resp.is_pointer_button_down_on() {
                if close { (theme::rgb(0xB3271C), Color32::WHITE) } else { (down_bg, idle_fg) }
            } else if resp.hovered() {
                if close { (theme::rgb(0xC42B1C), Color32::WHITE) } else { (hover_bg, idle_fg) }
            } else {
                (Color32::TRANSPARENT, idle_fg)
            };
            ui.painter().rect_filled(rect, 0.0, bg);
            widgets::glyph(ui.painter(), g, rect.center(), 10.0, fg);
            let tip = fp_text::t(self.lang, if close { fp_text::Key::CloseWindow } else { fp_text::Key::Minimise });
            if resp.on_hover_text(tip).clicked() {
                if close {
                    self.emit(UiAction::HideWindow);
                } else {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Minimized(true));
                }
            }
        }
    }

    /// Bottom-centre toast (ui 3.7).
    fn toast_ui(&mut self, ui: &mut Ui) {
        if !self.toast.visible(self.now) {
            return;
        }
        let (op, dy) = self.toast.look(self.now);
        let g = ui.painter().layout_no_wrap(self.toast.text().to_string(), widgets::font(12.0), Color32::WHITE);
        let size = vec2(g.size().x + 28.0, 30.0);
        let full = ui.max_rect();
        // Clear of the decorative bank strip (34 on home, 96 on onboarding) by 12.
        let lift = if self.screen == Some(Screen::Onboarding) { 96.0 + 12.0 } else { 34.0 + 22.0 };
        let pos = pos2(full.center().x - size.x / 2.0, full.bottom() - lift - size.y + dy as f32);
        let rect = Rect::from_min_size(pos, size);
        let p = ui.painter();
        p.rect_filled(rect.translate(vec2(0.0, 3.0)), 15.0, Color32::from_black_alpha(theme::alpha_byte(0.3 * op)));
        if self.tk.dark {
            p.rect_filled(rect, 15.0, Color32::from_white_alpha(theme::alpha_byte(0.2 * op)));
            let edge = Color32::from_white_alpha(theme::alpha_byte(0.35 * op));
            p.rect_stroke(rect, 15.0, egui::Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        } else {
            p.rect_filled(rect, 15.0, Color32::from_black_alpha(theme::alpha_byte(0.8 * op)));
        }
        p.galley(rect.center() - g.size() / 2.0, g, Color32::WHITE.gamma_multiply(op as f32));
    }

    /// Modal confirm sheet (ui 3.8).
    fn sheet_ui(&mut self, ui: &mut Ui) {
        let Some(sheet) = self.sheet.clone() else { return };
        let tk = self.tk;
        let full = ui.max_rect();
        let t = ui.ctx().animate_bool_with_time(egui::Id::unique("fp_sheet"), true, 0.2);
        let mut result: Option<bool> = None;
        egui::Area::new(egui::Id::unique("fp_sheet_area")).order(egui::Order::Foreground).fixed_pos(full.min).show(
            ui.ctx(),
            |ui| {
                // Dim backdrop; it only blocks input.
                let (rect, _) = ui.allocate_exact_size(full.size(), Sense::click_and_drag());
                ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(theme::alpha_byte(0.55 * f64::from(t))));
                let card_w = 340.0;
                let card = Rect::from_center_size(rect.center(), vec2(card_w, 164.0));
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(card.shrink2(vec2(20.0, 20.0))));
                ui.painter().rect_filled(card.translate(vec2(0.0, 6.0)), 14.0, Color32::from_black_alpha(60));
                ui.painter().rect_filled(card, 14.0, tk.menu().gamma_multiply(t));
                ui.painter().rect_stroke(card, 14.0, egui::Stroke::new(1.0, tk.ink10()), egui::StrokeKind::Inside);
                widgets::wrapped(&mut inner, &sheet.title, 16.0, tk.ink());
                inner.add_space(8.0);
                widgets::wrapped(&mut inner, &sheet.message, 13.0, tk.ink_a(0.9));
                inner.add_space(16.0);
                inner.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, &tk, &sheet.confirm, true).clicked() {
                        result = Some(true);
                    }
                    ui.add_space(10.0);
                    if widgets::quiet_button(ui, &tk, self.t(fp_text::Key::SetCancel), 36.0, true).clicked() {
                        result = Some(false);
                    }
                });
            },
        );
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            result = Some(false);
        }
        match result {
            Some(true) => {
                self.sheet = None;
                self.emit(sheet.action);
            }
            Some(false) => self.sheet = None,
            None => {}
        }
    }

    /// Ask for confirmation before an action (ui 3.8).
    pub fn confirm(&mut self, title: &str, message: &str, confirm: &str, action: UiAction) {
        self.sheet = Some(Sheet {
            title: title.to_string(),
            message: message.to_string(),
            confirm: confirm.to_string(),
            action,
        });
    }

    pub fn has_sheet(&self) -> bool {
        self.sheet.is_some()
    }

    /// One-line description of the chosen city for the settings row.
    pub(crate) fn city_line(&self, w: &Weathers) -> String {
        match &w.settings.city {
            Some(c) => fp_text::fmt(self.lang, fp_text::Key::CityCurrent, &[&city_label(c)]),
            None => fp_text::t(self.lang, fp_text::Key::CityNotChosen).to_string(),
        }
    }
}
