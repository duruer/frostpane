//! Settings tab (ui 3.5): nav, General / Screens / About / Help panes, the
//! language picker, the city search and the snowy ground decoration.

use crate::art;
use crate::cardmodel::{CityState, city_label};
use crate::model::{self, Glyph, Pane};
use crate::shell::{LoginNote, Shell, UiAction};
use crate::theme;
use crate::widgets;
use eframe::egui::{self, Color32, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use fp_core::effect::{Lang, Visitor};
use fp_core::settings::Weathers;
use fp_text::Key;

/// Open-Meteo attribution link (weather data licence page).
pub const OPEN_METEO_URL: &str = "https://open-meteo.com/";

/// Nav column width and page margins (ui 3.5).
const NAV_W: f32 = 264.0;

impl Shell {
    pub(crate) fn settings_tab(&mut self, ui: &mut Ui, w: &Weathers) {
        let tk = self.tk;
        let full = ui.max_rect();
        let ctx = ui.ctx().clone();
        let count = self.host.displays.len();
        self.pane = model::pane_to_open(self.pane, count);

        // Decorative snowy ground under everything (not hit-testable).
        self.ground_ui(ui, full);

        // Nav column (white 4%) and separator (white 8%).
        let nav = Rect::from_min_size(full.min, vec2(NAV_W, full.height()));
        ui.painter().rect_filled(nav, 0.0, Color32::from_white_alpha(theme::alpha_byte(0.04)));
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(full.left() + NAV_W, full.top()), vec2(1.0, full.height())),
            0.0,
            Color32::from_white_alpha(theme::alpha_byte(0.08)),
        );
        let mut y = full.top() + 16.0;
        for p in model::panes(count) {
            let rect = Rect::from_min_size(pos2(full.left() + 14.0, y), vec2(NAV_W - 28.0, 36.0));
            let resp = ui.interact(rect, egui::Id::unique(("fp_nav", p as u8)), Sense::click());
            let selected = p == self.pane;
            if selected {
                ui.painter().rect_filled(rect, 18.0, Color32::from_white_alpha(theme::alpha_byte(0.14)));
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 18.0, Color32::from_white_alpha(theme::alpha_byte(0.08)));
            }
            let ink = Color32::from_white_alpha(theme::alpha_byte(if selected { 1.0 } else { 0.72 }));
            widgets::glyph(ui.painter(), p.glyph(), pos2(rect.left() + 12.0 + 8.0, rect.center().y), 15.0, ink);
            let mut job =
                egui::text::LayoutJob::simple_singleline(self.t(p.title()).to_string(), widgets::font(15.0), ink);
            job.wrap = egui::text::TextWrapping {
                max_width: NAV_W - 28.0 - 43.0 - 12.0,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('…'),
            };
            let g = ui.painter().layout_job(job);
            ui.painter().galley(pos2(rect.left() + 43.0, rect.center().y - g.size().y / 2.0), g, ink);
            let full_title = self.t(p.title()).to_string();
            let natural = ui.painter().layout_no_wrap(full_title.clone(), widgets::font(15.0), ink).size().x;
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let resp = if natural > NAV_W - 28.0 - 43.0 - 12.0 { resp.on_hover_text(full_title) } else { resp };
            if resp.clicked() && !selected {
                self.pane = p;
            }
            y += 38.0;
        }

        // Page.
        let page = Rect::from_min_max(pos2(full.left() + NAV_W + 1.0, full.top()), full.max);
        let mut inner = ui.new_child(UiBuilder::new().max_rect(page));
        let pane = self.pane;
        inner.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        inner.spacing_mut().scroll.bar_width = 10.0;
        // Always reserve the gutter so content does not jump between panes.
        let sa = egui::ScrollArea::vertical()
            .id_salt(("pane", pane as u8))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(&mut inner, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 28, right: 28, top: 28, bottom: 0 }).show(
                    ui,
                    |ui| {
                        ui.set_width(ui.available_width());
                        widgets::label(ui, self.t(pane.title()), 19.0, tk.ink());
                        if let Some(d) = pane.detail() {
                            ui.add_space(4.0);
                            widgets::wrapped(ui, self.t(d), 13.5, tk.secondary());
                        }
                        ui.add_space(4.0);
                        match pane {
                            Pane::General => self.general_pane(ui, w),
                            Pane::Screens => self.screens_pane(ui, w),
                            Pane::About => self.about_pane(ui),
                            Pane::Help => self.help_pane(ui),
                        }
                        ui.add_space(130.0);
                    },
                );
            });
        // Bottom fade: tells the reader there is more below.
        let more = sa.state.clamped_offset().y + sa.inner_rect.height() < sa.content_size.y - 1.0;
        if more {
            let fade_h = 48.0;
            let top = page.bottom() - fade_h;
            let steps = 12;
            for i in 0..steps {
                let a = (i + 1) as f32 / steps as f32;
                let r = Rect::from_min_max(
                    pos2(page.left(), top + fade_h * i as f32 / steps as f32),
                    pos2(page.right(), top + fade_h * (i + 1) as f32 / steps as f32),
                );
                ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha((a * 90.0) as u8));
            }
        }
        self.language_popup(&ctx);
    }

    /// Ground-life clock: stopwatch restarted whenever the tab is shown
    /// anew; 30 fps frame index; reduced motion = settled pose at t=100.
    pub(crate) fn ground_clock(&mut self) -> (f64, i64) {
        if self.now - self.ground_seen > 0.25 {
            self.ground_started = None;
        }
        self.ground_seen = self.now;
        if self.reduce_motion {
            return (100.0, -1);
        }
        let start = *self.ground_started.get_or_insert(self.now);
        let t = self.now - start;
        (t, (t * 30.0).floor() as i64)
    }

    /// Snowy ground: little tree, ground critters, snow bank (ui 3.5).
    fn ground_ui(&mut self, ui: &mut Ui, full: Rect) {
        let ctx = ui.ctx().clone();
        crate::home::paint_bank(ui, Rect::from_min_max(full.min, full.max), 64.0, 0.09);
        if let Some(t) = self.tex.pixmap(&ctx, "littletree", fp_trees::little_tree) {
            let sz = t.size_vec2() / ctx.pixels_per_point();
            let r = Rect::from_min_size(pos2(full.left() + 64.0, full.bottom() - 44.0 - sz.y), sz);
            ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let (t, frame) = self.ground_clock();
        let scale = ctx.pixels_per_point();
        let tex = self.groundlife.update(&ctx, "groundlife", frame, || {
            let w = (fp_characters::groundlife::WIDTH as f32 * scale).ceil() as u32;
            let h = (fp_characters::groundlife::HEIGHT as f32 * scale).ceil() as u32;
            let mut pm = fp_paint::tiny_skia::Pixmap::new(w, h)?;
            fp_characters::groundlife::paint(&mut pm, scale, t);
            Some(art::pixmap_image(&pm))
        });
        if let Some(t) = tex {
            let sz = vec2(fp_characters::groundlife::WIDTH as f32, fp_characters::groundlife::HEIGHT as f32);
            let r = Rect::from_min_size(pos2(full.left(), full.bottom() - 36.0 - sz.y), sz);
            // Faded so it never competes with row text painted above it.
            ui.painter().image(
                t.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE.gamma_multiply(0.22),
            );
        }
    }

    /// One settings row (ui 3.5 Row): title, optional detail, optional
    /// element under the text, a control at the right. `last` hides the
    /// divider.
    // A settings row is title, detail, width of the control, last flag, indent,
    // and two closures; a parameter struct would only rename them.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        ui: &mut Ui,
        title: &str,
        detail: Option<&str>,
        control_w: f32,
        last: bool,
        indent: f32,
        under: impl FnOnce(&mut Shell, &mut Ui),
        control: impl FnOnce(&mut Shell, &mut Ui),
    ) {
        self.row_styled(ui, title, detail, control_w, last, indent, false, under, control);
    }

    /// `row` with an optional semibold title (About section headings).
    #[allow(clippy::too_many_arguments)]
    fn row_styled(
        &mut self,
        ui: &mut Ui,
        title: &str,
        detail: Option<&str>,
        control_w: f32,
        last: bool,
        indent: f32,
        bold: bool,
        under: impl FnOnce(&mut Shell, &mut Ui),
        control: impl FnOnce(&mut Shell, &mut Ui),
    ) {
        let tk = self.tk;
        ui.add_space(16.0);
        // Centre a bare title against a taller control.
        let pad = if detail.is_none() && control_w > 0.0 { 5.0 } else { 0.0 };
        let area = ui.max_rect();
        let left_w = (area.right() - ui.cursor().left() - control_w - 28.0 - indent).max(120.0);
        let top = ui.cursor().top();
        let left =
            ui.allocate_ui_with_layout(vec2(left_w + indent, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.add_space(pad);
                ui.horizontal(|ui| {
                    ui.add_space(indent);
                    ui.vertical(|ui| {
                        ui.set_width(left_w);
                        if bold {
                            widgets::wrapped_bold(ui, title, 14.5, tk.ink());
                        } else {
                            widgets::label(ui, title, 14.5, tk.ink());
                        }
                        if let Some(d) = detail {
                            ui.add_space(5.0);
                            widgets::wrapped(ui, d, 13.5, tk.secondary());
                        }
                        under(self, ui);
                    });
                });
            });
        let r = left.response.rect;
        let height = r.height().max(28.0);
        let cr = Rect::from_min_size(pos2(area.right() - control_w, top), vec2(control_w, height));
        let mut cui =
            ui.new_child(UiBuilder::new().max_rect(cr).layout(egui::Layout::right_to_left(egui::Align::Center)));
        control(self, &mut cui);
        if height > r.height() {
            ui.add_space(height - r.height());
        }
        ui.add_space(16.0);
        if !last {
            widgets::divider(ui, Color32::from_white_alpha(theme::alpha_byte(0.08)));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn switch_row(
        &mut self,
        ui: &mut Ui,
        title: &str,
        detail: Option<&str>,
        on: bool,
        indent: f32,
        last: bool,
        make: impl FnOnce(bool) -> UiAction,
    ) {
        let mut flip = None;
        self.row(
            ui,
            title,
            detail,
            62.0,
            last,
            indent,
            |_, _| {},
            |s, ui| {
                flip = widgets::switch(ui, &s.tk, on, true, true);
            },
        );
        if let Some(v) = flip {
            self.emit(make(v));
        }
    }

    fn general_pane(&mut self, ui: &mut Ui, w: &Weathers) {
        let tk = self.tk;
        let s = w.settings.clone();
        let lang = self.lang;

        // Language picker.
        let mut open_at = None;
        self.row(
            ui,
            fp_text::language_label(),
            None,
            190.0,
            false,
            0.0,
            |_, _| {},
            |sh, ui| {
                let g =
                    ui.painter().layout_no_wrap(lang.native_name().to_string(), widgets::font(13.0), Color32::WHITE);
                let size = vec2(20.0 + 8.0 + g.size().x + 8.0 + 10.0 + 28.0, 32.0);
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                let fill = Color32::from_white_alpha(theme::alpha_byte(if resp.hovered() { 0.16 } else { 0.12 }));
                ui.painter().rect_filled(rect, 16.0, fill);
                let badge = pos2(rect.left() + 14.0 + 10.0, rect.center().y);
                ui.painter().circle_filled(badge, 10.0, theme::rgb(art::lang_color(lang)));
                ui.painter().text(badge, egui::Align2::CENTER_CENTER, lang.code(), widgets::font(9.0), Color32::WHITE);
                ui.painter().galley(
                    pos2(rect.left() + 14.0 + 20.0 + 8.0, rect.center().y - g.size().y / 2.0),
                    g.clone(),
                    Color32::WHITE,
                );
                widgets::glyph(
                    ui.painter(),
                    Glyph::Chevron,
                    pos2(rect.right() - 14.0 - 4.0, rect.center().y),
                    10.0,
                    Color32::WHITE,
                );
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    open_at = Some(rect.right_bottom() + vec2(0.0, 4.0));
                }
                let _ = sh;
            },
        );
        if open_at.is_some() {
            self.lang_popup = open_at;
        }

        // Weather location (Open-Meteo city search).
        self.city_row(ui, w);

        self.switch_row(
            ui,
            self.t(Key::SetSnowballs),
            Some(self.t(Key::SetSnowballsDetail)),
            s.snowballs,
            0.0,
            false,
            UiAction::SetSnowballs,
        );

        // Open at login: state from the OS; failures flip it back with a note.
        let note = self.login_note;
        let mut flip = None;
        let on = self.login_on;
        self.row(
            ui,
            self.t(Key::SetLogin),
            Some(self.t(Key::SetLoginDetail)),
            62.0,
            false,
            0.0,
            |sh, ui| {
                if let Some(n) = note {
                    ui.add_space(8.0);
                    let key = if n == LoginNote::Blocked { Key::SetLoginBlocked } else { Key::SetLoginFailed };
                    widgets::wrapped(ui, sh.t(key), 13.0, lighten(theme::rgb(theme::ERROR_RED)));
                }
            },
            |sh, ui| flip = widgets::switch(ui, &sh.tk, on, true, true),
        );
        if let Some(v) = flip {
            self.login_note = None;
            self.login_on = v;
            self.emit(UiAction::SetLaunchAtLogin(v));
        }

        self.switch_row(
            ui,
            self.t(Key::SetBehind),
            Some(self.t(Key::SetBehindDetail)),
            s.behind_windows,
            0.0,
            false,
            UiAction::SetBehindWindows,
        );
        self.switch_row(
            ui,
            self.t(Key::SetKeepClear),
            Some(self.t(Key::SetKeepClearDetail)),
            s.keep_clear,
            0.0,
            false,
            UiAction::SetKeepClear,
        );

        // Visitors: master + four children.
        let master = model::visitors_master_on(&s);
        let mut flip = None;
        self.row(
            ui,
            self.t(Key::SetVisitors),
            Some(self.t(Key::SetVisitorsDetail)),
            62.0,
            false,
            0.0,
            |_, _| {},
            |sh, ui| {
                flip = widgets::switch(ui, &sh.tk, master, true, true);
            },
        );
        if let Some(v) = flip {
            for vis in Visitor::ALL {
                self.emit(UiAction::SetVisitor(vis, v));
            }
        }
        for vis in Visitor::ALL {
            self.switch_row(ui, self.t(model::visitor_key(vis)), None, s.comes(vis), 24.0, false, move |v| {
                UiAction::SetVisitor(vis, v)
            });
        }

        // Automatic updates only where this copy can replace itself; it is
        // then the last row.
        let auto = self.host.update_manual.is_none();
        self.switch_row(
            ui,
            self.t(Key::SetOften),
            Some(self.t(Key::SetOftenDetail)),
            s.often_visits,
            0.0,
            !self.host.has_battery && !auto,
            UiAction::SetOftenVisits,
        );
        if self.host.has_battery {
            self.switch_row(
                ui,
                self.t(Key::SetBattery),
                Some(self.t(Key::SetBatteryDetail)),
                s.pause_on_battery,
                0.0,
                !auto,
                UiAction::SetPauseOnBattery,
            );
        }
        if auto {
            self.switch_row(
                ui,
                self.t(Key::SetAutoUpdate),
                Some(self.t(Key::SetAutoUpdateDetail)),
                s.auto_update,
                0.0,
                true,
                UiAction::SetAutoUpdate,
            );
        }
        let _ = tk;
    }

    /// "Weather location" row with the search box and results.
    fn city_row(&mut self, ui: &mut Ui, w: &Weathers) {
        let tk = self.tk;
        let lang = self.lang;
        ui.add_space(16.0);
        widgets::label(ui, fp_text::t(lang, fp_text::Key::CityTitle), 14.5, tk.ink());
        ui.add_space(5.0);
        widgets::wrapped(ui, fp_text::t(lang, fp_text::Key::CityDetail), 13.5, tk.secondary());
        ui.add_space(6.0);
        let needed = w.settings.match_weather && w.settings.city.is_none();
        if needed {
            // One warning line carries the whole message.
            widgets::wrapped(ui, fp_text::t(lang, fp_text::Key::CityNeeded), 13.0, lighten(theme::rgb(theme::WARNING)));
        } else {
            let current = match &w.settings.city {
                Some(c) => fp_text::fmt(lang, fp_text::Key::CityCurrent, &[&city_text(c)]),
                None => self.city_line(w),
            };
            widgets::label(ui, &current, 13.0, tk.ink());
        }
        ui.add_space(8.0);
        let mut search = false;
        ui.horizontal(|ui| {
            let edit = egui::TextEdit::singleline(&mut self.city.query)
                .hint_text(
                    egui::RichText::new(fp_text::t(lang, fp_text::Key::CityPlaceholder))
                        .color(Color32::from_white_alpha(theme::alpha_byte(0.6))),
                )
                .desired_width(260.0)
                .margin(egui::Margin::symmetric(12, 9));
            let r = ui.add(edit);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                search = true;
            }
            let can = !matches!(self.city.state, CityState::Searching(_));
            let label = if matches!(self.city.state, CityState::Searching(_)) {
                fp_text::t(lang, fp_text::Key::CitySearching)
            } else {
                fp_text::t(lang, fp_text::Key::CitySearch)
            };
            if widgets::small_primary(ui, &tk, label, can).clicked() {
                search = true;
            }
            if w.settings.city.is_some()
                && widgets::quiet_button(ui, &tk, fp_text::t(lang, fp_text::Key::CityClear), 32.0, true).clicked()
            {
                self.emit(UiAction::SetCity(None));
            }
        });
        if search && let Some(q) = self.city.begin() {
            self.emit(UiAction::SearchCity(q));
        }
        match self.city.state.clone() {
            CityState::Results(list) => {
                ui.add_space(10.0);
                let mut picked = None;
                for (i, c) in list.iter().enumerate() {
                    let g = ui.painter().layout_no_wrap(city_text(c), widgets::font(13.5), tk.link());
                    let width = ui.available_width().min(360.0);
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 6.0, Color32::from_white_alpha(theme::alpha_byte(0.06)));
                    }
                    if i > 0 {
                        ui.painter().hline(
                            rect.x_range(),
                            rect.top(),
                            egui::Stroke::new(1.0, Color32::from_white_alpha(theme::alpha_byte(0.06))),
                        );
                    }
                    ui.painter().galley(pos2(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, tk.link());
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        picked = Some(i);
                    }
                }
                if let Some(i) = picked
                    && let Some(c) = self.city.pick(i)
                {
                    self.emit(UiAction::SetCity(Some(c)));
                }
            }
            CityState::NoMatch => {
                ui.add_space(6.0);
                widgets::label(ui, fp_text::t(lang, fp_text::Key::CityNone), 13.0, lighten(theme::rgb(theme::WARNING)));
            }
            CityState::Offline => {
                ui.add_space(6.0);
                widgets::wrapped(
                    ui,
                    fp_text::t(lang, fp_text::Key::CityOffline),
                    13.0,
                    lighten(theme::rgb(theme::ERROR_RED)),
                );
            }
            CityState::Searching(_) => ui.add_space(38.0),
            CityState::Idle => {}
        }
        ui.add_space(16.0);
        widgets::divider(ui, Color32::from_white_alpha(theme::alpha_byte(0.08)));
    }

    fn screens_pane(&mut self, ui: &mut Ui, w: &Weathers) {
        let rows = model::screen_rows(&w.settings, &self.host.displays);
        let n = rows.len();
        for (i, r) in rows.into_iter().enumerate() {
            let mut main = false;
            let mut flip = None;
            let title = if r.name.is_empty() { fp_text::screen_number(self.lang, i + 1) } else { r.name.clone() };
            self.row(
                ui,
                &title,
                None,
                230.0,
                i + 1 == n,
                0.0,
                |_, _| {},
                |sh, ui| {
                    flip = widgets::switch(ui, &sh.tk, r.enabled, true, r.switch_enabled);
                    ui.add_space(14.0);
                    if r.is_main {
                        let g = ui.painter().layout_no_wrap(
                            sh.t(Key::ScreenMain).to_string(),
                            widgets::font(12.0),
                            Color32::WHITE,
                        );
                        let (rect, _) = ui.allocate_exact_size(vec2(g.size().x + 20.0, 22.0), Sense::hover());
                        ui.painter().rect_filled(rect, 11.0, sh.tk.accent());
                        ui.painter().galley(rect.center() - g.size() / 2.0, g, Color32::WHITE);
                    } else if r.can_make_main
                        && widgets::quiet_button(ui, &sh.tk, sh.t(Key::ScreenMakeMain), 24.0, true).clicked()
                    {
                        main = true;
                    }
                },
            );
            if let Some(on) = flip {
                self.emit(UiAction::SetScreen { id: r.id.clone(), on });
            }
            if main {
                self.emit(UiAction::SetMainScreen(r.id.clone()));
            }
        }
    }

    fn about_pane(&mut self, ui: &mut Ui) {
        let tk = self.tk;
        let items = [
            (Key::AboutLocalTitle, Key::AboutLocalDetail),
            (Key::AboutNetTitle, Key::AboutNetDetail),
            (Key::AboutMatchTitle, Key::AboutMatchDetail),
        ];
        for (n, (t, d)) in items.into_iter().enumerate() {
            let match_row = n == 2;
            self.row_styled(
                ui,
                self.t(t),
                Some(self.t(d)),
                0.0,
                false,
                0.0,
                true,
                |sh, ui| {
                    if match_row {
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            widgets::label(ui, sh.t(Key::AboutAttribution), 13.0, sh.tk.ink());
                            if widgets::link_button(ui, sh.tk.link(), sh.t(Key::AboutSources), 13.0).clicked() {
                                sh.emit(UiAction::OpenUrl(OPEN_METEO_URL.to_string()));
                            }
                        });
                    }
                },
                |_, _| {},
            );
        }
        ui.add_space(16.0);
        let repo = self.host.issues_url.trim_end_matches("/issues").to_string();
        if widgets::link_button(ui, tk.link(), self.t(Key::AboutLicense), 13.5).clicked() {
            self.emit(UiAction::OpenUrl(repo));
        }
    }

    fn help_pane(&mut self, ui: &mut Ui) {
        let url = self.host.issues_url.clone();
        let mut open = false;
        // The link sits right under the title, no divider between them.
        let link_url = url.clone();
        self.row(
            ui,
            self.t(Key::HelpOpenIssues),
            None,
            0.0,
            true,
            0.0,
            |sh, ui| {
                ui.add_space(5.0);
                if widgets::link_button(ui, sh.tk.link(), &link_url, 13.5).clicked() {
                    open = true;
                }
            },
            |_, _| {},
        );
        widgets::divider(ui, Color32::from_white_alpha(theme::alpha_byte(0.08)));
        if open {
            self.emit(UiAction::OpenUrl(url));
        }
        let mut quit = false;
        self.row(
            ui,
            self.t(Key::SetQuit),
            Some(self.t(Key::SetQuitDetail)),
            150.0,
            true,
            0.0,
            |_, _| {},
            |sh, ui| {
                if widgets::quiet_button(ui, &sh.tk, sh.t(Key::SetQuitButton), 32.0, true).clicked() {
                    quit = true;
                }
            },
        );
        if quit {
            self.emit(UiAction::Quit);
        }
    }

    /// The language list under the picker pill.
    fn language_popup(&mut self, ctx: &egui::Context) {
        let Some(at) = self.lang_popup else { return };
        let tk = self.tk;
        let mut pick: Option<Lang> = None;
        let mut close = false;
        let area = egui::Area::new(egui::Id::unique("fp_langpopup"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(tk.menu())
                    .corner_radius(8.0)
                    .stroke(egui::Stroke::new(1.0, tk.ink10()))
                    .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(80) })
                    .inner_margin(5.0)
                    .show(ui, |ui| {
                        ui.set_width(200.0);
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for l in Lang::ALL {
                            let (rect, resp) = ui.allocate_exact_size(vec2(200.0, 28.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 5.0, tk.accent());
                            }
                            let fg = if resp.hovered() { Color32::WHITE } else { tk.ink() };
                            let badge = pos2(rect.left() + 18.0 + 10.0, rect.center().y);
                            ui.painter().circle_filled(badge, 10.0, theme::rgb(art::lang_color(l)));
                            ui.painter().text(
                                badge,
                                egui::Align2::CENTER_CENTER,
                                l.code(),
                                widgets::font(9.0),
                                Color32::WHITE,
                            );
                            ui.painter().text(
                                pos2(rect.left() + 18.0 + 20.0 + 8.0, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                l.native_name(),
                                widgets::font(13.0),
                                fg,
                            );
                            if l == self.lang {
                                widgets::glyph(
                                    ui.painter(),
                                    Glyph::Check,
                                    pos2(rect.right() - 14.0, rect.center().y),
                                    11.0,
                                    fg,
                                );
                            }
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                pick = Some(l);
                            }
                        }
                    });
            });
        let outside = ctx.input(|i| i.pointer.any_pressed())
            && !ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p));
        if pick.is_some() || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if let Some(l) = pick
            && l != self.lang
        {
            // Choosing the current language does nothing.
            self.emit(UiAction::SetLanguage(l));
        }
        if close {
            self.lang_popup = None;
        }
    }
}

/// City label without a repeated admin region ("Tokyo, Tokyo, Japan").
fn city_text(c: &fp_core::settings::City) -> String {
    let mut c = c.clone();
    if c.admin == c.name {
        c.admin.clear();
    }
    city_label(&c)
}

/// Brighter amber for warning text on the dark gradient.
fn lighten(c: Color32) -> Color32 {
    let m = |v: u8| v.saturating_add((255 - v) / 3);
    Color32::from_rgb(m(c.r()), m(c.g()), m(c.b()))
}
