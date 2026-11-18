//! Effects, Wallpapers, How-to and Changelog tabs (ui 3.1-3.4).

use crate::TrayAction;
use crate::art;
use crate::menu::icon_for;
use crate::model::Glyph;
use crate::shell::{Shell, TimeCard, UiAction};
use crate::theme;
use crate::wallpapers::{self, WallAction};
use crate::widgets;
use eframe::egui::{self, Color32, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use fp_core::effect::EffectKind;
use fp_core::settings::Weathers;
use fp_paint::IconName;
use fp_sources::daynight::Slot;

/// Column gap and row gap of the tile grids (ui 3.1).
pub const GRID_GAP: (f32, f32) = (16.0, 18.0);
/// Time card content width (ui 3.2).
pub const TIME_CARD_W: f32 = 502.0;
/// Top of the page body below the header, tab row and divider (ui 2.1).
const BODY_TOP: f32 = 123.0;

/// Darkening of a time-of-day still when no picture file exists: alpha of
/// a night-blue overlay by slot.
pub fn time_shade(slot: Slot) -> u8 {
    match slot {
        Slot::Dawn => 90,
        Slot::Sunrise => 40,
        Slot::Morning => 0,
        Slot::Midday => 0,
        Slot::Golden => 30,
        Slot::Sunset => 70,
        Slot::Dusk => 110,
        Slot::Night => 160,
    }
}

/// Icon of a how-to guide row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideIcon {
    Named(IconName),
    Glyph(Glyph),
}

/// One how-to row: icon, guide colour, shows the "show me" button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guide {
    pub icon: GuideIcon,
    pub color: u32,
    pub in_tray: bool,
}

const BLUE: u32 = 0x338FF7;

/// The 12 rows, in `fp_text::how_tip` order (ui 3.3).
pub const GUIDE: [Guide; 12] = [
    Guide { icon: GuideIcon::Named(IconName::Snow), color: BLUE, in_tray: true },
    Guide { icon: GuideIcon::Named(IconName::Snowman), color: BLUE, in_tray: false },
    Guide { icon: GuideIcon::Named(IconName::Snow), color: 0x73BFFF, in_tray: false },
    Guide { icon: GuideIcon::Named(IconName::Snowman), color: 0x6B94EB, in_tray: false },
    Guide { icon: GuideIcon::Named(IconName::Santa), color: 0xD92633, in_tray: false },
    Guide { icon: GuideIcon::Named(IconName::Tree), color: 0x218C54, in_tray: true },
    Guide { icon: GuideIcon::Named(IconName::Plough), color: 0xF28C26, in_tray: true },
    Guide { icon: GuideIcon::Named(IconName::Outside), color: 0xF2AD1A, in_tray: true },
    Guide { icon: GuideIcon::Glyph(Glyph::Behind), color: theme::GRAY_GLYPH, in_tray: false },
    Guide { icon: GuideIcon::Glyph(Glyph::SnowOnly), color: theme::GRAY_GLYPH, in_tray: false },
    Guide { icon: GuideIcon::Glyph(Glyph::Battery), color: theme::GRAY_GLYPH, in_tray: false },
    Guide { icon: GuideIcon::Glyph(Glyph::Screens), color: theme::GRAY_GLYPH, in_tray: false },
];

/// Index of the battery guide row (hidden without a battery).
pub const BATTERY_ROW: usize = 10;

/// The three how-to sections as row ranges.
pub const SECTIONS: [std::ops::Range<usize>; 3] = [0..1, 1..7, 7..12];

/// Rows of a section that are visible on this machine.
pub fn visible_rows(section: usize, has_battery: bool) -> Vec<usize> {
    SECTIONS[section].clone().filter(|i| has_battery || *i != BATTERY_ROW).collect()
}

/// Fixed-height text row: the glyph box is centred in `h`, so a CJK fallback
/// font with a taller line box cannot push the rows below it.
fn fixed_row(ui: &mut Ui, text: &str, size: f32, color: Color32, h: f32) {
    let g = ui.painter().layout_no_wrap(text.to_string(), widgets::font(size), color);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter().galley(pos2(rect.left(), rect.center().y - g.size().y / 2.0), g, color);
}

/// Fade the scroll viewport's last rows into the sky so rows cut by the
/// window bottom read as "more below" instead of a hard cut.
fn bottom_fade(ui: &Ui, tk: &theme::Tokens) {
    let clip = ui.clip_rect();
    let h = 30.0_f32.min(clip.height());
    let band = Rect::from_min_max(pos2(clip.left(), clip.bottom() - h), clip.max);
    let id = egui::LayerId::new(egui::Order::Middle, egui::Id::NULL.with("fp_bottom_fade"));
    let sky = tk.sky().1;
    art::gradient(&ui.ctx().layer_painter(id), band, Color32::TRANSPARENT, sky.gamma_multiply(0.92));
}

impl Shell {
    // ---- Effects (ui 3.1) ----

    pub(crate) fn effects_tab(&mut self, ui: &mut Ui, w: &Weathers) {
        let tk = self.tk;
        let s = &w.settings;
        let width = ui.available_width();
        let top = ui.cursor().top();
        if matches!(self.lang, fp_core::effect::Lang::Ja | fp_core::effect::Lang::Ko) {
            // CJK glyphs have no small caps; 11 pt reads smaller than the line below.
            fixed_row(ui, self.t(fp_text::Key::PickTitle), 13.0, tk.secondary(), 16.0);
        } else {
            fixed_row(ui, &fp_text::upper(self.lang, self.t(fp_text::Key::PickTitle)), 11.0, tk.secondary(), 16.0);
        }
        ui.add_space(4.0);
        fixed_row(ui, self.t(fp_text::Key::PickLine), 12.0, tk.secondary(), 16.0);
        if w.weather_on() {
            let r = Rect::from_min_size(pos2(ui.max_rect().right() - 220.0, top - 11.0), vec2(220.0, 36.0));
            let mut inner =
                ui.new_child(UiBuilder::new().max_rect(r).layout(egui::Layout::right_to_left(egui::Align::Min)));
            if widgets::quiet_button(&mut inner, &tk, self.t(fp_text::Key::TurnOffEffects), 36.0, true).clicked() {
                self.emit(UiAction::Tray(TrayAction::Off));
            }
        }
        ui.add_space(14.0);
        let cw = widgets::cell_width(width, 3, GRID_GAP.0);
        let ctx = ui.ctx().clone();
        for row in EffectKind::ALL.chunks(3) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = GRID_GAP.0;
                for &e in row {
                    ui.vertical(|ui| {
                        ui.set_width(cw);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let active = s.weather == Some(e);
                        let h = cw / 1.6;
                        let tex =
                            self.tex.pixmap(&ctx, &format!("tile:{e:?}:{cw:.0}"), |sc| art::tile_art(e, cw, h, sc));
                        let icon = self.tex.icon(&ctx, icon_for(e), 0xFFFFFF, (34.0 * ctx.pixels_per_point()) as u32);
                        let resp = widgets::tile(ui, &tk, cw, 1.6, active, |ui, rect| {
                            if let Some(t) = &tex {
                                egui::Image::new((t.id(), rect.size())).corner_radius(12.0).paint_at(ui, rect);
                            }
                            if let Some(i) = &icon {
                                let r = Rect::from_center_size(rect.center(), vec2(34.0, 34.0));
                                ui.painter().image(
                                    i.id(),
                                    r.translate(vec2(0.0, 1.0)),
                                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                    Color32::from_black_alpha(90),
                                );
                                ui.painter().image(
                                    i.id(),
                                    r,
                                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            }
                        });
                        if resp.clicked() {
                            self.emit(UiAction::Tray(TrayAction::Toggle(e)));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            widgets::label(ui, fp_text::effect(self.lang, e), 13.0, tk.ink());
                            if active {
                                widgets::on_badge(ui, &tk, self.t(fp_text::Key::OnBadge));
                            }
                        });
                        ui.add_space(6.0);
                        match e {
                            EffectKind::Snow => {
                                let labels = [fp_text::effect(self.lang, e), self.t(fp_text::Key::HeavySnow)];
                                if let Some(second) = widgets::two_ways_state(ui, &tk, e, labels, s.heavy_snow, active)
                                {
                                    self.emit(UiAction::GridVariant { effect: e, second });
                                }
                            }
                            EffectKind::Rain => {
                                let labels = [fp_text::effect(self.lang, e), self.t(fp_text::Key::Thunderstorm)];
                                if let Some(second) = widgets::two_ways_state(ui, &tk, e, labels, s.lightning, active) {
                                    self.emit(UiAction::GridVariant { effect: e, second });
                                }
                            }
                            _ => {
                                widgets::wrapped(ui, fp_text::effect_detail(self.lang, e), 11.0, tk.secondary());
                            }
                        }
                    });
                }
            });
            ui.add_space(GRID_GAP.1);
        }
        // Clear the decorative hill strip so the last row scrolls fully free.
        ui.add_space(40.0);
        bottom_fade(ui, &tk);
    }

    // ---- Wallpapers (ui 3.2) ----

    /// Picture of a pack: a decoded file when the host can decode, else own art.
    pub(crate) fn pack_picture(
        &mut self,
        ctx: &egui::Context,
        file: &str,
        effect: EffectKind,
        w: f32,
        h: f32,
    ) -> Option<egui::TextureHandle> {
        let path = self.host.wallpapers_dir.join(file);
        let side = (w.max(h) * ctx.pixels_per_point()).ceil() as u32;
        if let Some(dec) = self.host.decoder.clone() {
            let key = format!("wall:{file}:{side}");
            if let Some(t) = self.tex.get(ctx, &key, || {
                let (pw, ph, bytes) = dec(&path, side)?;
                art::rgba_image(pw, ph, &bytes)
            }) {
                return Some(t);
            }
        }
        self.tex.pixmap(ctx, &format!("tile:{effect:?}:{w:.0}"), |sc| art::tile_art(effect, w, h, sc))
    }

    pub(crate) fn wallpapers_tab(&mut self, ui: &mut Ui, _w: &Weathers) {
        let tk = self.tk;
        let ctx = ui.ctx().clone();
        if self.papers.is_empty() {
            let top = ((ui.available_height() - 160.0) / 2.0).max(0.0);
            ui.add_space(top.min(90.0));
            ui.vertical_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
                widgets::glyph(ui.painter(), Glyph::Monitor, r.center(), 32.0, tk.secondary());
                ui.add_space(10.0);
                widgets::label(ui, fp_text::t(self.lang, fp_text::Key::NoWallpapers), 14.0, tk.ink());
                ui.add_space(4.0);
                ui.set_max_width(ui.available_width().min(360.0));
                widgets::wrapped(ui, fp_text::t(self.lang, fp_text::Key::NoWallpapersHint), 13.0, tk.secondary());
                ui.add_space(14.0);
                if widgets::quiet_button(ui, &tk, self.t(fp_text::Key::OpenWallFolder), 32.0, true).clicked() {
                    self.emit(UiAction::OpenWallpapersFolder);
                }
            });
            bottom_fade(ui, &tk);
            return;
        }
        widgets::section_title(ui, &tk, self.lang, self.t(fp_text::Key::WallpapersTitle));
        ui.add_space(14.0);
        let cw = widgets::cell_width(ui.available_width(), 3, GRID_GAP.0);
        let papers = self.papers.clone();
        for row in papers.chunks(3) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = GRID_GAP.0;
                for p in row {
                    ui.vertical(|ui| {
                        ui.set_width(cw);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let h = cw / 1.6;
                        let tex = self.pack_picture(&ctx, &p.main, p.effect, cw, h);
                        let ringed = self.picked.ring(p.id);
                        let resp = widgets::tile(ui, &tk, cw, 1.6, ringed, |ui, rect| {
                            if let Some(t) = &tex {
                                egui::Image::new((t.id(), rect.size())).corner_radius(12.0).paint_at(ui, rect);
                            }
                        });
                        let tile_rect = resp.rect;
                        let mut pill_hit = false;
                        if p.daylong {
                            pill_hit = self.day_pill(ui, p.id, p.effect, tile_rect);
                        }
                        if resp.clicked() && !pill_hit {
                            self.emit(UiAction::Wallpaper(WallAction::Main { pack: p.id.to_string() }));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            widgets::label(ui, fp_text::wallpaper_name(self.lang, p.effect), 13.0, tk.ink());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let text = fp_text::with_weather(self.lang, p.effect);
                                if widgets::link_button(ui, tk.link(), &text, 12.5).clicked() {
                                    self.emit(UiAction::WallpaperWith {
                                        action: WallAction::Main { pack: p.id.to_string() },
                                        effect: p.effect,
                                    });
                                }
                            });
                        });
                    });
                }
            });
            ui.add_space(GRID_GAP.1);
        }
        ui.add_space(40.0);
        bottom_fade(ui, &tk);
    }

    /// The day-and-night pill at a tile's bottom right; true when clicked.
    fn day_pill(&mut self, ui: &mut Ui, pack: &'static str, _effect: EffectKind, tile: Rect) -> bool {
        let tk = self.tk;
        let label = self.t(fp_text::Key::DayNightLabel);
        let g = ui.painter().layout_no_wrap(label.to_string(), widgets::font(11.0), Color32::WHITE);
        let size = vec2(g.size().x + 9.0 * 2.0 + 14.0 + 10.0 + 10.0 + 4.0, 24.0);
        let rect = Rect::from_min_size(pos2(tile.right() - 8.0 - size.x, tile.bottom() - 8.0 - size.y), size);
        let resp = ui.interact(rect, egui::Id::unique(("fp_daypill", pack)), Sense::click());
        let accent = self.picked.pill_accent(pack);
        let bg = if accent { tk.accent() } else { Color32::from_rgba_unmultiplied(14, 20, 36, 214) };
        let bg = if resp.is_pointer_button_down_on() { bg.gamma_multiply(0.75) } else { bg };
        ui.painter().rect_filled(rect, 12.0, bg);
        widgets::glyph(ui.painter(), Glyph::Sun, pos2(rect.left() + 9.0 + 6.0, rect.center().y), 12.0, Color32::WHITE);
        ui.painter().galley(
            pos2(rect.left() + 9.0 + 12.0 + 5.0, rect.center().y - g.size().y / 2.0),
            g,
            Color32::WHITE,
        );
        widgets::glyph(
            ui.painter(),
            Glyph::Chevron,
            pos2(rect.right() - 9.0 - 4.0, rect.center().y),
            8.0,
            Color32::WHITE,
        );
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.hovered() {
            // Themed tip anchored above the pill so it never covers the name/link row.
            let help = self.t(fp_text::Key::DayNightHelp).to_string();
            egui::Area::new(egui::Id::unique(("fp_daypill_tip", pack)))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .pivot(egui::Align2::RIGHT_BOTTOM)
                .fixed_pos(rect.right_top() - vec2(0.0, 6.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(tk.menu())
                        .stroke(egui::Stroke::new(1.0, tk.ink16()))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_max_width(240.0);
                            ui.add(
                                egui::Label::new(egui::RichText::new(help).font(widgets::font(12.0)).color(tk.ink()))
                                    .wrap(),
                            );
                        });
                });
        }
        let clicked = resp.clicked();
        if clicked {
            self.time_card = Some(TimeCard { pack, anchor: pos2(tile.left(), rect.bottom() + 6.0), opened: self.now });
        }
        clicked
    }

    /// The popup time card (ui 3.2).
    pub(crate) fn time_card_ui(&mut self, ctx: &egui::Context, _w: &Weathers) {
        let Some(card) = self.time_card.clone() else { return };
        let Some(paper) = self.papers.iter().find(|p| p.id == card.pack).cloned() else {
            self.time_card = None;
            return;
        };
        let tk = self.tk;
        let screen = ctx.content_rect();
        let width = TIME_CARD_W + 32.0;
        let x = card.anchor.x.max(screen.left() + 8.0).min(screen.right() - width - 8.0);
        let mut close = false;
        let on = self.picked.cycle_on(paper.id);
        let current = wallpapers::current_slot(self.wall.local_minutes());
        // Keep an 8 px margin from the window bottom: flip above the pill (anchor is pill bottom + 6, pill 24 high)
        // when the card, sized from last frame, would not fit below; then clamp into the window.
        let card_id = egui::Id::unique("fp_timecard");
        let h = ctx.memory(|m| m.area_rect(card_id)).map_or(0.0, |r| r.height());
        let mut y = card.anchor.y;
        // Never above the divider under the tab row: the header stays visible.
        let floor = screen.top() + BODY_TOP + 8.0;
        if h > 0.0 {
            let above = card.anchor.y - 6.0 - 24.0 - 6.0 - h;
            if above >= floor {
                y = above;
            }
        }
        if h > 0.0 {
            y = y.min(screen.bottom() - 8.0 - h).max(floor);
        }
        let area = egui::Area::new(card_id).order(egui::Order::Foreground).fixed_pos(pos2(x, y)).show(ctx, |ui| {
            egui::Frame::new()
                .fill(tk.menu())
                .corner_radius(14.0)
                .stroke(egui::Stroke::new(1.0, tk.ink10()))
                .shadow(egui::Shadow { offset: [0, 8], blur: 18, spread: 0, color: Color32::from_black_alpha(90) })
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_width(TIME_CARD_W);
                    widgets::wrapped_bold(ui, fp_text::wallpaper_name(self.lang, paper.effect), 13.0, tk.ink());
                    ui.add_space(8.0);
                    // Banner.
                    egui::Frame::new().fill(tk.ink06()).corner_radius(12.0).inner_margin(10.0).show(ui, |ui| {
                        ui.set_width(TIME_CARD_W - 20.0);
                        ui.horizontal(|ui| {
                            widgets::glyph(
                                ui.painter(),
                                Glyph::Sun,
                                ui.cursor().left_top() + vec2(9.0, 9.0),
                                18.0,
                                tk.accent(),
                            );
                            ui.add_space(28.0);
                            ui.vertical(|ui| {
                                ui.set_width(TIME_CARD_W - 200.0);
                                widgets::label(ui, self.t(fp_text::Key::ChangesWithDay), 13.0, tk.ink());
                                ui.add_space(2.0);
                                widgets::wrapped(ui, self.t(fp_text::Key::DayNightHelp), 11.0, tk.secondary());
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if on {
                                    if widgets::quiet_button(ui, &tk, self.t(fp_text::Key::TurnOff), 36.0, true)
                                        .clicked()
                                    {
                                        self.emit(UiAction::Wallpaper(WallAction::TurnOff {
                                            pack: paper.id.to_string(),
                                            slot: current,
                                        }));
                                        close = true;
                                    }
                                } else if widgets::primary_button(ui, &tk, self.t(fp_text::Key::TurnOn), true).clicked()
                                {
                                    self.emit(UiAction::Wallpaper(WallAction::TurnOn { pack: paper.id.to_string() }));
                                    close = true;
                                }
                            });
                        });
                    });
                    ui.add_space(12.0);
                    widgets::section_title(ui, &tk, self.lang, self.t(fp_text::Key::OrPickTime));
                    ui.add_space(10.0);
                    let cw = widgets::cell_width(TIME_CARD_W, 4, 10.0);
                    for (r, row) in wallpapers::times().chunks(4).enumerate() {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            for (i, (slot, _)) in row.iter().enumerate() {
                                let idx = r * 4 + i;
                                ui.vertical(|ui| {
                                    ui.set_width(cw);
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    let h = cw / 2.0;
                                    let file = fp_sources::daynight::file_name(paper.id, *slot);
                                    let tex = self.pack_picture(ctx, &file, paper.effect, cw, h);
                                    let ringed = self.picked.0.as_deref()
                                        == Some(&wallpapers::still_id(paper.id, *slot))
                                        || (on && *slot == current);
                                    let shade = time_shade(*slot);
                                    let is_now = *slot == current;
                                    let resp = widgets::tile(ui, &tk, cw, 2.0, ringed, |ui, rect| {
                                        if let Some(t) = &tex {
                                            egui::Image::new((t.id(), rect.size()))
                                                .corner_radius(12.0)
                                                .paint_at(ui, rect);
                                        }
                                        if shade > 0 {
                                            ui.painter().rect_filled(
                                                rect,
                                                12.0,
                                                Color32::from_rgba_unmultiplied(8, 14, 40, shade),
                                            );
                                        }
                                        if is_now {
                                            let now = self.t(fp_text::Key::NowBadge);
                                            let g = ui.painter().layout_no_wrap(
                                                now.to_string(),
                                                widgets::font(10.0),
                                                Color32::WHITE,
                                            );
                                            let pill = Rect::from_min_size(
                                                rect.left_top() + vec2(6.0, 6.0),
                                                vec2(g.size().x + 8.0 + 10.0, 16.0),
                                            );
                                            ui.painter().rect_filled(pill, 8.0, tk.accent());
                                            ui.painter().circle_filled(
                                                pos2(pill.left() + 8.0, pill.center().y),
                                                2.5,
                                                Color32::WHITE,
                                            );
                                            ui.painter().galley(
                                                pos2(pill.left() + 14.0, pill.center().y - g.size().y / 2.0),
                                                g,
                                                Color32::WHITE,
                                            );
                                        }
                                    });
                                    ui.add_space(4.0);
                                    widgets::label(ui, fp_text::time_name(self.lang, idx), 11.0, tk.ink());
                                    if resp.clicked() {
                                        self.emit(UiAction::Wallpaper(WallAction::Still {
                                            pack: paper.id.to_string(),
                                            slot: *slot,
                                        }));
                                        close = true;
                                    }
                                });
                            }
                        });
                        ui.add_space(10.0);
                    }
                });
        });
        let outside = ctx.input(|i| i.pointer.any_pressed())
            && !ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| area.response.rect.contains(p))
            && self.now - card.opened > 0.2;
        if close || outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.time_card = None;
        }
    }

    // ---- How-to (ui 3.3) ----

    pub(crate) fn howto_tab(&mut self, ui: &mut Ui) {
        let tk = self.tk;
        let ctx = ui.ctx().clone();
        let titles = [fp_text::Key::HowStart, fp_text::Key::HowPlay, fp_text::Key::HowYours];
        for (si, key) in titles.into_iter().enumerate() {
            ui.horizontal(|ui| {
                widgets::section_title(ui, &tk, self.lang, self.t(key));
                if si == 1 {
                    ui.add_space(8.0);
                    widgets::label(ui, self.t(fp_text::Key::HowPlayNote), 12.0, tk.secondary());
                }
            });
            ui.add_space(10.0);
            let rows = visible_rows(si, self.host.has_battery);
            egui::Frame::new().fill(tk.menu()).corner_radius(12.0).stroke(egui::Stroke::new(1.0, tk.ink06())).show(
                ui,
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (n, &i) in rows.iter().enumerate() {
                        if n > 0 {
                            ui.horizontal(|ui| {
                                ui.add_space(58.0);
                                widgets::divider(ui, tk.divider());
                            });
                        }
                        self.guide_row(ui, &ctx, i);
                    }
                },
            );
            ui.add_space(22.0);
        }
        ui.add_space(40.0);
        bottom_fade(ui, &tk);
    }

    fn guide_row(&mut self, ui: &mut Ui, ctx: &egui::Context, i: usize) {
        let tk = self.tk;
        let g = GUIDE[i];
        let (title, line) = fp_text::how_tip(self.lang, i);
        egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 12 }).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (badge, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                ui.painter().rect_filled(badge, 8.0, theme::rgb_a(g.color, 0.14));
                match g.icon {
                    GuideIcon::Named(n) => {
                        let px = (18.0 * ctx.pixels_per_point()).ceil() as u32;
                        if let Some(t) = self.tex.icon(ctx, n, g.color, px) {
                            ui.painter().image(
                                t.id(),
                                Rect::from_center_size(badge.center(), vec2(18.0, 18.0)),
                                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        }
                    }
                    GuideIcon::Glyph(gl) => widgets::glyph(ui.painter(), gl, badge.center(), 16.0, theme::rgb(g.color)),
                }
                ui.add_space(14.0);
                let right = if g.in_tray { 110.0 } else { 0.0 };
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - right - 14.0).max(100.0));
                    widgets::label(ui, title, 13.0, tk.ink());
                    ui.add_space(3.0);
                    widgets::wrapped(ui, line, 12.0, tk.secondary());
                    if g.in_tray && self.pointer_warning {
                        ui.add_space(4.0);
                        widgets::wrapped(ui, self.t(fp_text::Key::HowTrayHidden), 12.0, theme::rgb(theme::WARNING));
                    }
                });
                if g.in_tray {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.add_space(2.0);
                        if widgets::quiet_button(ui, &tk, self.t(fp_text::Key::HowShowMe), 30.0, true).clicked() {
                            self.pointer_warning = false;
                            self.emit(UiAction::ShowTrayPointer);
                        }
                    });
                }
            });
        });
    }

    // ---- Changelog (ui 3.4) ----

    pub(crate) fn changelog_tab(&mut self, ui: &mut Ui) {
        let tk = self.tk;
        for r in crate::changelog::releases() {
            let rows = r.sorted();
            // Badge column: widest label among the kinds present in this release.
            let col_w = rows
                .iter()
                .map(|c| {
                    ui.painter()
                        .layout_no_wrap(c.kind.label(self.lang).to_string(), widgets::font(10.0), tk.ink())
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max)
                + 14.0
                + 10.0;
            ui.horizontal(|ui| {
                widgets::label(ui, r.number, 15.0, tk.ink());
                ui.add_space(8.0);
                widgets::label(ui, &fp_text::format_date(self.lang, r.date), 12.0, tk.secondary());
            });
            ui.add_space(10.0);
            egui::Frame::new().fill(tk.ink06()).corner_radius(12.0).stroke(egui::Stroke::new(1.0, tk.ink10())).show(
                ui,
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (n, c) in rows.into_iter().enumerate() {
                        if n > 0 {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                widgets::divider(ui, tk.divider());
                            });
                        }
                        egui::Frame::new()
                            .inner_margin(egui::Margin { left: 14, right: 14, top: 11, bottom: 11 })
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    let (cell, _) = ui.allocate_exact_size(vec2(col_w, 18.0), Sense::hover());
                                    let label = c.kind.label(self.lang);
                                    let col = theme::rgb(c.kind.color());
                                    let g = ui.painter().layout_no_wrap(label.to_string(), widgets::font(10.0), col);
                                    let pill = Rect::from_min_size(cell.min, g.size() + vec2(14.0, 6.0));
                                    ui.painter().rect_filled(pill, 9.0, theme::rgb_a(c.kind.color(), 0.16));
                                    ui.painter().galley(pill.center() - g.size() / 2.0, g, col);
                                    ui.vertical(|ui| {
                                        ui.set_width(ui.available_width());
                                        ui.add_space(1.0);
                                        widgets::wrapped(ui, c.text(self.lang), 13.0, tk.ink());
                                    });
                                });
                            });
                    }
                },
            );
            ui.add_space(22.0);
        }
        ui.add_space(40.0);
        bottom_fade(ui, &tk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_table_matches_the_spec() {
        assert_eq!(GUIDE.len(), fp_text::HOW_TIPS);
        let tray: Vec<usize> = (0..12).filter(|i| GUIDE[*i].in_tray).collect();
        assert_eq!(tray, [0, 5, 6, 7]);
        assert_eq!(GUIDE[4].color, 0xD92633);
        assert_eq!(GUIDE[BATTERY_ROW].icon, GuideIcon::Glyph(Glyph::Battery));
        assert_eq!(SECTIONS.iter().map(|r| r.len()).sum::<usize>(), 12);
    }

    #[test]
    fn battery_row_hidden_on_desktops() {
        assert_eq!(visible_rows(2, true), [7, 8, 9, 10, 11]);
        assert_eq!(visible_rows(2, false), [7, 8, 9, 11]);
        assert_eq!(visible_rows(0, false), [0]);
        assert_eq!(visible_rows(1, false).len(), 6);
    }

    #[test]
    fn shade_darkens_towards_night() {
        assert_eq!(time_shade(Slot::Midday), 0);
        assert!(time_shade(Slot::Night) > time_shade(Slot::Dusk));
        for s in Slot::ALL {
            let _ = time_shade(s);
        }
    }
}
