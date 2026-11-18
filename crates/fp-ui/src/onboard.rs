//! Onboarding pages (ui 3.6) on the snow-day backdrop, and the tray sketch
//! (core-app 2.8) drawn in our own style.

use crate::art;
use crate::home::{mascot_pixmap, paint_bank, paint_snow};
use crate::menu::icon_for;
use crate::onboarding as ob;
use crate::shell::{Shell, UiAction};
use crate::theme;
use crate::wallpapers;
use crate::widgets;
use eframe::egui::{self, Align2, Color32, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use fp_core::effect::{EffectKind, Lang};
use fp_core::settings::Weathers;
use fp_text::Key;

/// Snow flakes on the onboarding page.
const FLAKES: usize = 60;
/// Footer row height.
const FOOT_H: f32 = 44.0;

impl Shell {
    pub(crate) fn onboarding_ui(&mut self, ui: &mut Ui, w: &Weathers) {
        let tk = self.tk;
        let full = ui.max_rect();
        let ctx = ui.ctx().clone();
        let (top, bottom) = theme::snow_day();
        art::gradient(ui.painter(), full, top, bottom);
        // Effect steps are full-bleed wallpaper cards: no page snow or bank.
        let on_card = ob::effect_of(self.step).is_some();
        if !on_card {
            paint_snow(ui, full, FLAKES, self.now, 0.9);
            paint_bank(ui, full, 96.0, 0.8);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        // Live weather side effects: the step's weather runs on the desktop.
        let want = ob::step_weather(self.step);
        if self.live != Some(want) {
            self.live = Some(want);
            self.emit(UiAction::SetWeather(want));
        }

        // The snowman on welcome and done.
        let show_snowman = ob::snowman_visible(self.step);
        let t = ctx.animate_bool_with_time(egui::Id::unique("fp_ob_snowman"), show_snowman, 0.35);
        if t > 0.0
            && let Some(tex) = self.tex.pixmap(&ctx, "mascot:92", |s| mascot_pixmap(92.0, s, true))
        {
            let r = Rect::from_min_size(pos2(full.left() + 210.0, full.bottom() - 34.0 - 92.0), vec2(73.6, 92.0));
            ui.painter().image(
                tex.id(),
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE.gamma_multiply(t),
            );
        }

        // Page transition: fade/slide in over 0.35 s after each step change.
        let page_t = ctx.animate_value_with_time(egui::Id::unique(("fp_ob_page", self.step)), 1.0, 0.35);
        let content = Rect::from_min_max(
            pos2(full.left() + 40.0, full.top() + 38.0),
            pos2(full.right() - 40.0, full.bottom() - 26.0),
        );
        let footer_h = FOOT_H;
        let pages = Rect::from_min_max(content.min, pos2(content.right(), content.bottom() - footer_h));
        let mut page_ui = ui.new_child(UiBuilder::new().max_rect(pages.translate(vec2(0.0, 10.0 * (1.0 - page_t)))));
        page_ui.set_opacity(page_t);
        self.page(&mut page_ui, pages, w);

        // Footer: back | dots | primary.
        let foot = Rect::from_min_max(pos2(content.left(), content.bottom() - footer_h), content.max);
        let mut fui = ui.new_child(UiBuilder::new().max_rect(foot));
        let mut go: Option<isize> = None;
        let step = self.step;
        if ob::back_visible(step) {
            let mut b = fui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(foot.min, vec2(160.0, 36.0))));
            let back = if on_card {
                let st = widgets::PillStyle {
                    height: 36.0,
                    pad: 18.0,
                    size: 13.0,
                    fill: Color32::from_white_alpha(theme::alpha_byte(0.18)),
                    pressed_fill: Color32::from_white_alpha(theme::alpha_byte(0.34)),
                    hover_fill: Color32::from_white_alpha(theme::alpha_byte(0.26)),
                    border: Some(Color32::from_white_alpha(theme::alpha_byte(0.3))),
                    text: Color32::WHITE,
                };
                widgets::pill(&mut b, self.t(Key::ObBack), st, true)
            } else {
                widgets::quiet_button(&mut b, &tk, self.t(Key::ObBack), 36.0, true)
            };
            if back.clicked() {
                go = Some(-1);
            }
        }
        if ob::dots_visible(step) {
            let total = ob::DOTS as f32 * 6.0 + (ob::DOTS as f32 - 1.0) * 7.0 + 12.0;
            let mut x = foot.center().x - total / 2.0;
            for i in 0..ob::DOTS {
                let target = ob::dot_width(i, step);
                let wd = ctx.animate_value_with_time(egui::Id::unique(("fp_dot", i)), target, 0.3);
                let idle = if on_card {
                    Color32::from_white_alpha(theme::alpha_byte(0.25))
                } else {
                    Color32::from_black_alpha(theme::alpha_byte(0.15))
                };
                let color = if i == step { tk.accent() } else { idle };
                let r = Rect::from_min_size(pos2(x, foot.top() + 18.0 - 3.0), vec2(wd, 6.0));
                fui.painter().rect_filled(r, 3.0, color);
                x += wd + 7.0;
            }
        }
        {
            let key = ob::primary_key(step);
            let label = self.t(key);
            let g = fui.painter().layout_no_wrap(label.to_string(), widgets::font(13.0), Color32::WHITE);
            let wd = (g.size().x + 2.0 * 22.0).max(96.0);
            let r = Rect::from_min_size(pos2(foot.right() - wd, foot.top()), vec2(wd, 36.0));
            let mut b = fui.new_child(UiBuilder::new().max_rect(r));
            let resp = widgets::primary_button(&mut b, &tk, label, true);
            let enter = fui.input(|i| i.key_pressed(egui::Key::Enter));
            if resp.clicked() || enter {
                go = Some(1);
            }
        }
        match go {
            Some(-1) => self.step = ob::back(step),
            Some(_) => {
                if step == ob::LANGUAGE {
                    // Commit the language chosen on the first page.
                    self.emit(UiAction::SetLanguage(self.lang));
                }
                match ob::forward(step) {
                    Some(n) => self.step = n,
                    None => {
                        self.emit(UiAction::FinishOnboarding);
                        self.live = None;
                    }
                }
            }
            None => {}
        }
    }

    fn page(&mut self, ui: &mut Ui, rect: Rect, w: &Weathers) {
        match self.step {
            ob::LANGUAGE => self.page_language(ui, rect, w),
            ob::WELCOME => self.page_welcome(ui, rect),
            s if ob::effect_of(s).is_some() => {
                if let Some(e) = ob::effect_of(s) {
                    self.page_effect(ui, rect, e, s);
                }
            }
            ob::WALLPAPERS => self.page_wallpapers(ui, rect),
            _ => self.page_done(ui, rect),
        }
    }

    /// Centred vertical stack helper: runs `add` inside a centred column.
    fn centered(&mut self, ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Shell, &mut Ui)) {
        let mut inner = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::top_down(egui::Align::Center).with_main_justify(false)),
        );
        add(self, &mut inner);
    }

    fn heading(&mut self, ui: &mut Ui, text: &str, size: f32) {
        let tk = self.tk;
        let g = ui.painter().layout_no_wrap(text.to_string(), theme::bold(size), tk.ink());
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y), Sense::hover());
        ui.painter().galley(pos2(r.center().x - g.size().x / 2.0, r.top()), g, tk.ink());
    }

    fn paragraph(&mut self, ui: &mut Ui, text: &str, size: f32, color: Color32) {
        for line in text.split('\n') {
            let g = ui.painter().layout_no_wrap(line.to_string(), widgets::font(size), color);
            let (r, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), theme::line_height(size) + 3.0), Sense::hover());
            ui.painter().galley(pos2(r.center().x - g.size().x / 2.0, r.top()), g, color);
        }
    }

    /// App icon (accent tile with the snow glyph), distinct from the mascot.
    fn logo(&mut self, ui: &mut Ui, side: f64) {
        let ctx = ui.ctx().clone();
        let side = side as f32;
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), side), Sense::hover());
        let tile = Rect::from_center_size(r.center(), vec2(side, side));
        ui.painter().rect_filled(tile, side * 0.24, self.tk.accent());
        let g = side * 0.58;
        let px = (g * ctx.pixels_per_point()).ceil() as u32;
        if let Some(t) = self.tex.icon(&ctx, fp_paint::IconName::Snow, 0xFFFFFF, px) {
            ui.painter().image(
                t.id(),
                Rect::from_center_size(tile.center(), vec2(g, g)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
    }

    fn page_language(&mut self, ui: &mut Ui, rect: Rect, w: &Weathers) {
        let tk = self.tk;
        let current = self.lang;
        let _ = w;
        let mut picked: Option<Lang> = None;
        self.centered(ui, rect, |sh, ui| {
            ui.add_space(20.0);
            sh.logo(ui, 72.0);
            ui.add_space(30.0);
            sh.heading(ui, sh.t(Key::ObLangTitle), 32.0);
            ui.add_space(10.0);
            sh.paragraph(ui, sh.t(Key::ObLangNote), 15.0, tk.secondary());
            ui.add_space(28.0);
            for row in Lang::ALL.chunks(5) {
                let total = row.len() as f32 * 150.0 + (row.len() as f32 - 1.0) * 12.0;
                let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 118.0), Sense::hover());
                let mut x = area.center().x - total / 2.0;
                for &l in row {
                    let r = Rect::from_min_size(pos2(x, area.top()), vec2(150.0, 118.0));
                    let resp = ui.interact(r, egui::Id::unique(("fp_langcard", l.code())), Sense::click());
                    let on = l == current;
                    let shadow =
                        Color32::from_rgba_unmultiplied(77, 89, 128, theme::alpha_byte(if on { 0.18 } else { 0.08 }));
                    ui.painter().add(
                        egui::epaint::Shadow { offset: [0, 8], blur: 18, spread: 0, color: shadow }.as_shape(r, 20.0),
                    );
                    ui.painter().rect_filled(
                        r,
                        20.0,
                        Color32::WHITE.gamma_multiply(if resp.is_pointer_button_down_on() { 0.6 } else { 1.0 }),
                    );
                    let stroke = if on {
                        egui::Stroke::new(2.5, tk.accent())
                    } else {
                        egui::Stroke::new(1.0, Color32::from_black_alpha(theme::alpha_byte(0.08)))
                    };
                    ui.painter().rect_stroke(r.shrink(stroke.width / 2.0), 20.0, stroke, egui::StrokeKind::Inside);
                    let badge = pos2(r.center().x, r.top() + 40.0);
                    ui.painter().circle_filled(badge, 22.0, theme::rgb(art::lang_color(l)));
                    ui.painter().text(
                        badge,
                        Align2::CENTER_CENTER,
                        l.code().to_uppercase(),
                        widgets::font(15.0),
                        Color32::WHITE,
                    );
                    ui.painter().text(
                        pos2(r.center().x, r.top() + 84.0),
                        Align2::CENTER_CENTER,
                        l.native_name(),
                        widgets::font(17.0),
                        Color32::from_black_alpha(theme::alpha_byte(0.85)),
                    );
                    if on {
                        let c = pos2(r.right() - 10.0 - 11.0, r.top() + 10.0 + 11.0);
                        ui.painter().circle_filled(c, 11.0, tk.accent());
                        widgets::glyph(ui.painter(), crate::model::Glyph::Check, c, 12.0, Color32::WHITE);
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
                        picked = Some(l);
                    }
                    x += 162.0;
                }
                ui.add_space(12.0);
            }
        });
        if let Some(l) = picked {
            // Applies at once; the page rebuilds in the new language.
            self.lang = l;
            self.emit(UiAction::SetLanguage(l));
        }
    }

    fn page_welcome(&mut self, ui: &mut Ui, rect: Rect) {
        let tk = self.tk;
        self.centered(ui, rect, |sh, ui| {
            ui.add_space(rect.height() * 0.12);
            sh.logo(ui, 96.0);
            ui.add_space(34.0);
            sh.heading(ui, sh.t(Key::ObWelcomeTitle), 36.0);
            ui.add_space(10.0);
            sh.paragraph(ui, sh.t(Key::ObWelcomeLine), 15.0, tk.secondary());
        });
    }

    fn page_effect(&mut self, ui: &mut Ui, rect: Rect, e: EffectKind, step: usize) {
        let ctx = ui.ctx().clone();
        // Full-bleed card: the spec's negative margin reaches the window edge.
        let card = Rect::from_min_max(
            pos2(rect.left() - 40.0, rect.top() - 38.0),
            pos2(rect.right() + 40.0, rect.bottom() + FOOT_H + 26.0),
        );
        let (cw, ch) = (card.width(), card.height());
        let file = self.papers.iter().find(|p| p.effect == e).map(|p| p.main.clone());
        let tex = match file {
            Some(f) => self.pack_picture(&ctx, &f, e, cw, ch),
            None => self.tex.pixmap(&ctx, &format!("obcard:{e:?}:{cw:.0}x{ch:.0}"), |s| art::tile_art(e, cw, ch, s)),
        };
        if let Some(t) = tex {
            // Cover-crop the picture to the card.
            let ts = t.size_vec2();
            let (ca, ta) = (cw / ch, ts.x / ts.y.max(1.0));
            let uv = if ta > ca {
                let k = ca / ta;
                Rect::from_min_max(pos2((1.0 - k) / 2.0, 0.0), pos2((1.0 + k) / 2.0, 1.0))
            } else {
                let k = ta / ca;
                Rect::from_min_max(pos2(0.0, (1.0 - k) / 2.0), pos2(1.0, (1.0 + k) / 2.0))
            };
            ui.painter().image(t.id(), card, uv, Color32::WHITE);
        }
        // Bottom fade: alpha 0 at 50%, 140/255 black at the bottom, smooth.
        let half = Rect::from_min_max(pos2(card.left(), card.center().y), card.max);
        art::gradient(ui.painter(), half, Color32::TRANSPARENT, Color32::from_black_alpha(140));

        let lang = self.lang;
        // Align with the content frame (Back pill / dots at +40); keep a clear gap above the footer.
        let mut y = card.bottom() - 28.0 - 160.0 - 20.0;
        let x = card.left() + 40.0;
        let shadow = |p: &egui::Painter, pos: egui::Pos2, a: Align2, text: &str, size: f32, c: Color32| {
            // Effect titles (34) are Bold; the step and blurb lines are regular.
            let f = if size >= 34.0 { theme::bold(size) } else { widgets::font(size) };
            p.text(pos + vec2(0.0, 1.0), a, text, f.clone(), Color32::from_black_alpha(100));
            p.text(pos, a, text, f, c);
        };
        let n = ob::effect_number(step).unwrap_or(1);
        shadow(
            ui.painter(),
            pos2(x, y),
            Align2::LEFT_TOP,
            &fp_text::step(lang, n, ob::EFFECTS),
            12.0,
            Color32::from_white_alpha(theme::alpha_byte(0.8)),
        );
        y += 22.0;
        let px = (38.0 * ctx.pixels_per_point()).ceil() as u32;
        if let Some(t) = self.tex.icon(&ctx, icon_for(e), 0xFFFFFF, px) {
            ui.painter().image(
                t.id(),
                Rect::from_min_size(pos2(x, y), vec2(38.0, 38.0)),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        shadow(ui.painter(), pos2(x + 46.0, y), Align2::LEFT_TOP, fp_text::effect(lang, e), 34.0, Color32::WHITE);
        y += 52.0;
        shadow(
            ui.painter(),
            pos2(x, y),
            Align2::LEFT_TOP,
            fp_text::pitch(lang, e),
            15.0,
            Color32::from_white_alpha(theme::alpha_byte(0.9)),
        );
        y += 26.0;
        shadow(
            ui.painter(),
            pos2(x, y),
            Align2::LEFT_TOP,
            &fp_text::try_it(lang, e),
            12.0,
            Color32::from_white_alpha(theme::alpha_byte(0.7)),
        );
    }

    fn page_wallpapers(&mut self, ui: &mut Ui, rect: Rect) {
        let tk = self.tk;
        let ctx = ui.ctx().clone();
        let packs: Vec<EffectKind> = if self.papers.is_empty() {
            wallpapers::ORDER.to_vec()
        } else {
            self.papers.iter().map(|p| p.effect).collect()
        };
        let cw = 116.0;
        let est = theme::line_height(32.0)
            + 10.0
            + Self::para_h(self.t(Key::ObWallLine), 15.0)
            + 34.0
            + cw * 0.625
            + 24.0
            + 22.0
            + Self::para_h(self.t(Key::ObWallNote), 14.0);
        let top = ((rect.height() - est) / 2.0).max(0.0);
        self.centered(ui, rect, |sh, ui| {
            ui.add_space(top);
            sh.heading(ui, sh.t(Key::ObWallTitle), 32.0);
            ui.add_space(10.0);
            sh.paragraph(ui, sh.t(Key::ObWallLine), 15.0, tk.secondary());
            ui.add_space(34.0);
            let total = packs.len() as f32 * cw + (packs.len() as f32 - 1.0) * 12.0;
            let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), cw * 0.625 + 24.0), Sense::hover());
            let mut x = area.center().x - total / 2.0;
            for e in packs {
                let r = Rect::from_min_size(pos2(x, area.top()), vec2(cw, cw * 0.625));
                let file = sh.papers.iter().find(|p| p.effect == e).map(|p| p.main.clone());
                let tex = match file {
                    Some(f) => sh.pack_picture(&ctx, &f, e, cw, cw * 0.625),
                    None => {
                        sh.tex.pixmap(&ctx, &format!("tile:{e:?}:{cw:.0}"), |s| art::tile_art(e, cw, cw * 0.625, s))
                    }
                };
                if let Some(t) = tex {
                    egui::Image::new((t.id(), r.size())).corner_radius(cw * 0.045).paint_at(ui, r);
                }
                ui.painter().rect_filled(
                    r.translate(vec2(0.0, 2.0)).expand(1.0),
                    cw * 0.06,
                    Color32::from_black_alpha(theme::alpha_byte(0.10)),
                );
                ui.painter().rect_stroke(
                    r,
                    cw * 0.045,
                    egui::Stroke::new(1.0, Color32::from_black_alpha(theme::alpha_byte(0.42))),
                    egui::StrokeKind::Outside,
                );
                ui.painter().rect_stroke(
                    r,
                    cw * 0.045,
                    egui::Stroke::new(1.5, Color32::from_white_alpha(theme::alpha_byte(0.7))),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    pos2(r.center().x, r.bottom() + 14.0),
                    Align2::CENTER_CENTER,
                    fp_text::wallpaper_name(sh.lang, e),
                    widgets::font(12.0),
                    tk.secondary(),
                );
                x += cw + 12.0;
            }
            ui.add_space(22.0);
            sh.paragraph(ui, sh.t(Key::ObWallNote), 14.0, tk.secondary());
        });
    }

    fn page_done(&mut self, ui: &mut Ui, rect: Rect) {
        let tk = self.tk;
        let sketch_h = 116.0;
        let est = theme::line_height(36.0)
            + 10.0
            + Self::para_h(self.t(Key::ObDoneLine), 15.0)
            + 30.0
            + sketch_h
            + 28.0
            + Self::para_h(self.t(Key::ObDoneNote), 13.0);
        let top = ((rect.height() - est) / 2.0).max(0.0);
        self.centered(ui, rect, |sh, ui| {
            ui.add_space(top);
            sh.heading(ui, sh.t(Key::ObDoneTitle), 36.0);
            ui.add_space(10.0);
            sh.paragraph(ui, sh.t(Key::ObDoneLine), 15.0, tk.secondary());
            ui.add_space(30.0);
            let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), sketch_h), Sense::hover());
            let r = Rect::from_center_size(area.center(), vec2(620.0, sketch_h));
            sh.tray_sketch(ui, r);
            ui.add_space(28.0);
            sh.paragraph(ui, sh.t(Key::ObDoneNote), 13.0, tk.secondary());
        });
    }

    /// Height `paragraph` takes for `text`.
    fn para_h(text: &str, size: f32) -> f32 {
        (text.matches('\n').count() + 1) as f32 * (theme::line_height(size) + 3.0)
    }

    /// A drawn taskbar with the overflow flyout pointing at our icon.
    fn tray_sketch(&mut self, ui: &mut Ui, r: Rect) {
        let tk = self.tk;
        let ctx = ui.ctx().clone();
        let p = ui.painter().with_clip_rect(r.expand(30.0));
        // Card with sky gradient.
        p.rect_filled(
            r.translate(vec2(0.0, 6.0)).expand(3.0),
            12.0,
            Color32::from_rgba_unmultiplied(51, 64, 102, theme::alpha_byte(0.18)),
        );
        art::gradient(&p, r.shrink2(vec2(1.0, 6.0)), theme::rgb(0xD6E6FF), theme::rgb(0x99BAF5));
        p.rect_filled(
            Rect::from_min_size(r.min, vec2(r.width(), 12.0)),
            egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 },
            theme::rgb(0xD6E6FF),
        );
        p.rect_filled(
            Rect::from_min_size(pos2(r.left(), r.bottom() - 12.0), vec2(r.width(), 12.0)),
            egui::CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 },
            theme::rgb(0xF5F7FC),
        );
        // Faint window suggestion on the desktop.
        let win = Rect::from_min_size(pos2(r.left() + 34.0, r.top() + 18.0), vec2(190.0, 44.0));
        p.rect_filled(win, 6.0, Color32::from_white_alpha(theme::alpha_byte(0.35)));
        p.rect_filled(
            Rect::from_min_size(win.min, vec2(win.width(), 10.0)),
            egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 },
            Color32::from_white_alpha(theme::alpha_byte(0.35)),
        );
        // Taskbar.
        let bar = Rect::from_min_max(pos2(r.left(), r.bottom() - 44.0), r.max);
        p.rect_filled(bar, egui::CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 }, theme::rgb_a(0xF5F7FC, 0.94));
        p.rect_filled(
            Rect::from_min_size(bar.min, vec2(bar.width(), 1.0)),
            0.0,
            Color32::from_black_alpha(theme::alpha_byte(0.07)),
        );
        // Left cluster: start tiles and three app tiles.
        let mut x = r.center().x - 100.0;
        let cy = bar.center().y;
        for (dx, dy) in [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)] {
            p.rect_filled(
                Rect::from_min_size(pos2(x + dx - 9.0 + 0.0, cy - 9.0 + dy), vec2(8.0, 8.0)),
                1.5,
                tk.accent(),
            );
        }
        x += 18.0 + 14.0;
        for c in [0xFAC74C, 0x3399F2, 0x59BF80] {
            p.rect_filled(Rect::from_center_size(pos2(x + 11.0 - 9.0, cy), vec2(22.0, 22.0)), 6.0, theme::rgb(c));
            x += 22.0 + 14.0;
        }
        // Right cluster: chevron, system glyphs and clock.
        let mut rx = r.right() - 14.0;
        let clock = fp_text::t(self.lang, Key::ObClockCaption);
        let ink = Color32::from_black_alpha(theme::alpha_byte(0.75));
        // Measured width; shrink the font when the label is wider than its slot.
        let slot = 150.0;
        let mut size = 12.0;
        let mut g = p.layout_no_wrap(clock.to_string(), widgets::font(size), ink);
        if g.size().x > slot {
            size = (size * slot / g.size().x).max(8.0);
            g = p.layout_no_wrap(clock.to_string(), widgets::font(size), ink);
        }
        let gw = g.size().x.min(slot);
        p.galley(pos2(rx - gw, cy - g.size().y / 2.0), g, ink);
        rx -= gw + 22.0;
        for _ in 0..3 {
            p.circle_filled(pos2(rx, cy), 3.5, Color32::from_black_alpha(theme::alpha_byte(0.55)));
            rx -= 20.0;
        }
        let chevron = Rect::from_center_size(pos2(rx - 8.0, cy), vec2(26.0, 28.0));
        p.rect_filled(chevron, 5.0, Color32::from_black_alpha(theme::alpha_byte(0.07)));
        widgets::glyph(
            &p,
            crate::model::Glyph::Chevron,
            chevron.center() + vec2(0.0, 2.0),
            10.0,
            Color32::from_black_alpha(theme::alpha_byte(0.75)),
        );
        // Flyout above the chevron holding three cells; the middle one is ours.
        let fly = Rect::from_min_size(pos2(chevron.left() - 43.0, chevron.top() - 50.0), vec2(106.0, 38.0));
        p.rect_filled(fly, 8.0, theme::rgb(0xFAFAFC));
        p.rect_stroke(
            fly,
            8.0,
            egui::Stroke::new(1.0, Color32::from_black_alpha(theme::alpha_byte(0.08))),
            egui::StrokeKind::Inside,
        );
        for i in 0..3 {
            let cell = Rect::from_min_size(fly.min + vec2(6.0 + 2.0 + 34.0 * i as f32, 6.0), vec2(30.0, 26.0));
            if i == 1 {
                p.rect_filled(cell, 6.0, tk.accent());
                let px = (17.0 * ctx.pixels_per_point()).ceil() as u32;
                if let Some(t) = self.tex.icon(&ctx, fp_paint::IconName::Snow, 0xFFFFFF, px) {
                    p.image(
                        t.id(),
                        Rect::from_center_size(cell.center(), vec2(17.0, 17.0)),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                // Brand name and down arrow above the cell.
                p.text(
                    pos2(cell.center().x, cell.top() - 22.0),
                    Align2::CENTER_CENTER,
                    fp_text::t(self.lang, Key::AppName),
                    widgets::font(12.0),
                    tk.accent(),
                );
                p.text(
                    pos2(cell.center().x, cell.top() - 8.0),
                    Align2::CENTER_CENTER,
                    "\u{25BE}",
                    widgets::font(13.0),
                    tk.accent(),
                );
            } else {
                p.circle_filled(cell.center(), 4.0, Color32::from_black_alpha(theme::alpha_byte(0.45)));
            }
        }
    }
}
