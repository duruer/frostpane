//! Home screen frame: sky backdrop, header, tab bar, update button, body.

use crate::art;
use crate::model::Tab;
use crate::shell::{Shell, UPDATE_RESULT_SHOWN, UiAction, UpdateLine};
use crate::theme;
use crate::widgets;
use eframe::egui::{self, Color32, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use fp_core::settings::Weathers;

/// Flakes of the Home backdrop (ui 2.1).
pub const HOME_FLAKES: usize = 40;

impl Shell {
    pub(crate) fn home_ui(&mut self, ui: &mut Ui, w: &Weathers) {
        let tk = self.tk;
        let full = ui.max_rect();
        let ctx = ui.ctx().clone();

        // Sky, falling snow, snow bank (not hit-testable).
        let (top, bottom) = tk.sky();
        art::gradient(ui.painter(), full, top, bottom);
        paint_snow(ui, full, HOME_FLAKES, self.now, 1.0);
        paint_bank(ui, full, 34.0, 0.07);
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        // Header: logo, name, version.
        let header_y = full.top() + 30.0;
        let logo = self.tex.pixmap(&ctx, "mascot:28", |s| mascot_pixmap(28.0, s, false));
        if let Some(t) = &logo {
            let r = Rect::from_min_size(pos2(full.left() + 22.0, header_y), vec2(22.4, 28.0));
            ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let name_pos = pos2(full.left() + 22.0 + 22.4 + 12.0, header_y + 14.0);
        ui.painter().text(
            name_pos,
            egui::Align2::LEFT_CENTER,
            fp_text::t(self.lang, fp_text::Key::AppName),
            widgets::font(15.0),
            tk.ink(),
        );
        let ver = fp_text::version(self.lang, &self.host.version);
        ui.painter().text(
            pos2(full.right() - 24.0, header_y + 14.0),
            egui::Align2::RIGHT_CENTER,
            ver,
            widgets::font(12.0),
            tk.secondary(),
        );

        // Tab row (height 36, margin 18,16,18,12).
        let row_top = header_y + 28.0 + 16.0;
        let row = Rect::from_min_size(pos2(full.left() + 18.0, row_top), vec2(full.width() - 36.0, 36.0));
        let mut row_ui = ui.new_child(UiBuilder::new().max_rect(row));
        self.tab_bar(&mut row_ui, row);
        self.update_button(&mut row_ui, row);

        // Divider.
        let div_y = row.bottom() + 12.0;
        ui.painter().rect_filled(
            Rect::from_min_size(pos2(full.left(), div_y), vec2(full.width(), 1.0)),
            0.0,
            tk.divider(),
        );

        // Body.
        let body = Rect::from_min_max(pos2(full.left(), div_y + 1.0), full.max);
        let mut body_ui = ui.new_child(UiBuilder::new().max_rect(body));
        match self.tab {
            Tab::Effects => self.scroll_body(&mut body_ui, "effects", |s, ui| s.effects_tab(ui, w)),
            Tab::Wallpapers => self.scroll_body(&mut body_ui, "wallpapers", |s, ui| s.wallpapers_tab(ui, w)),
            Tab::HowTo => self.scroll_body(&mut body_ui, "howto", |s, ui| s.howto_tab(ui)),
            Tab::Changelog => self.scroll_body(&mut body_ui, "changelog", |s, ui| s.changelog_tab(ui)),
            Tab::Settings => self.settings_tab(&mut body_ui, w),
        }
        self.time_card_ui(&ctx, w);
    }

    /// Scrollable tab body with the page margin (24,24,24,42).
    fn scroll_body(&mut self, ui: &mut Ui, id: &str, add: impl FnOnce(&mut Shell, &mut Ui)) {
        let out = egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 24, bottom: 42 }).show(ui, |ui| {
                add(self, ui);
            });
        });
        // Bottom fade as a scroll cue while more content lies below.
        let more = out.content_size.y - out.state.clamped_offset().y - out.inner_rect.height() > 4.0;
        if more {
            let r = out.inner_rect;
            let (_, sky_bottom) = self.tk.sky();
            let strip = Rect::from_min_max(pos2(r.left(), r.bottom() - 36.0), r.max);
            art::gradient(ui.painter(), strip, sky_bottom.gamma_multiply(0.0), sky_bottom);
        }
    }

    /// Tab buttons with the sliding pill (ui 2.2).
    fn tab_bar(&mut self, ui: &mut Ui, row: Rect) {
        let tk = self.tk;
        let ctx = ui.ctx().clone();
        let mut x = row.left();
        let mut hit: Option<Tab> = None;
        let mut target = (row.left(), 60.0);
        let mut cells = Vec::new();
        for tab in Tab::ALL {
            let title = fp_text::t(self.lang, tab.title());
            let g = ui.painter().layout_no_wrap(title.to_string(), widgets::font(13.0), tk.ink());
            let badge = tab == Tab::Changelog && self.badge;
            let w = g.size().x + 22.0 + if badge { 22.0 } else { 0.0 };
            let rect = Rect::from_min_size(pos2(x, row.top() + 3.0), vec2(w, 30.0));
            if tab == self.tab {
                target = (rect.left(), rect.width());
            }
            cells.push((tab, rect, g, badge));
            x += w + 2.0;
        }
        // Pill.
        let (px, pw) = match self.tab_pill {
            None => target,
            Some(_) => (
                ctx.animate_value_with_time(egui::Id::unique("fp_pill_x"), target.0, 0.28),
                ctx.animate_value_with_time(egui::Id::unique("fp_pill_w"), target.1, 0.28),
            ),
        };
        if self.tab_pill.is_none() {
            // First placement without animation.
            ctx.animate_value_with_time(egui::Id::unique("fp_pill_x"), target.0, 0.0);
            ctx.animate_value_with_time(egui::Id::unique("fp_pill_w"), target.1, 0.0);
        }
        self.tab_pill = Some(target);
        ui.painter().rect_filled(Rect::from_min_size(pos2(px, row.top() + 3.0), vec2(pw, 30.0)), 15.0, tk.ink10());
        for (tab, rect, g, badge) in cells {
            let resp = ui.interact(rect, egui::Id::unique(("fp_tab", tab as u8)), Sense::click());
            let selected = tab == self.tab;
            let op = if selected {
                1.0
            } else if resp.hovered() {
                0.85
            } else {
                0.55
            };
            if !selected && resp.hovered() {
                ui.painter().rect_filled(rect, 15.0, tk.ink07());
            }
            let text_pos = pos2(rect.left() + 11.0, rect.center().y - g.size().y / 2.0);
            ui.painter().galley(text_pos, g.clone(), tk.ink().gamma_multiply(op));
            if badge {
                let c = pos2(rect.left() + 11.0 + g.size().x + 6.0 + 8.0, rect.center().y);
                ui.painter().circle_filled(c, 8.0, tk.accent());
                ui.painter().text(c, egui::Align2::CENTER_CENTER, "1", widgets::font(10.0), Color32::WHITE);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                hit = Some(tab);
            }
        }
        if let Some(t) = hit {
            self.pick_tab(t);
        }
    }

    /// "Check for updates" with its 4 s status line (ui 2.3).
    fn update_button(&mut self, ui: &mut Ui, row: Rect) {
        let tk = self.tk;
        if let UpdateLine::UpToDate { at } | UpdateLine::Offline { at } = self.update
            && self.now - at >= UPDATE_RESULT_SHOWN
        {
            self.update = UpdateLine::Idle;
        }
        let cell =
            Rect::from_min_max(pos2(row.right() - 340.0, row.top() + 2.0), pos2(row.right() - 6.0, row.bottom() - 2.0));
        let mut inner =
            ui.new_child(UiBuilder::new().max_rect(cell).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let ui = &mut inner;
        match self.update.clone() {
            UpdateLine::Idle => {
                if widgets::ghost_button(ui, &tk, self.t(fp_text::Key::SetCheckUpdates)).clicked() {
                    self.update = UpdateLine::Checking;
                    self.emit(UiAction::CheckUpdates);
                }
            }
            UpdateLine::Checking => {
                widgets::label(ui, self.t(fp_text::Key::UpdChecking), 12.0, tk.secondary());
            }
            UpdateLine::UpToDate { .. } => {
                widgets::label(ui, &format!("\u{2713} {}", self.t(fp_text::Key::SetUpToDate)), 12.0, tk.secondary());
            }
            UpdateLine::Offline { .. } => {
                if widgets::ghost_button(ui, &tk, self.t(fp_text::Key::SetCheckUpdates)).clicked() {
                    self.update = UpdateLine::Checking;
                    self.emit(UiAction::CheckUpdates);
                }
                widgets::label(ui, self.t(fp_text::Key::UpdOffline), 12.0, theme::rgb(theme::WARNING));
            }
            UpdateLine::Available { version, url } => {
                let text = fp_text::update_available(self.lang, &version);
                let st = widgets::PillStyle {
                    height: 32.0,
                    pad: 14.0,
                    size: 13.0,
                    fill: tk.accent(),
                    pressed_fill: tk.accent().gamma_multiply(0.75),
                    hover_fill: tk.accent().gamma_multiply(0.88),
                    border: None,
                    text: Color32::WHITE,
                };
                if widgets::pill(ui, &text, st, true).clicked() {
                    self.emit(UiAction::OpenUrl(url));
                }
            }
        }
    }
}

/// Own mascot art at `side` points high, for the header and pages.
pub(crate) fn mascot_pixmap(side: f64, scale: f32, waving: bool) -> Option<fp_paint::tiny_skia::Pixmap> {
    let w = (0.8 * side * f64::from(scale)).ceil().max(1.0) as u32;
    let h = (side * f64::from(scale)).ceil().max(1.0) as u32;
    let mut pm = fp_paint::tiny_skia::Pixmap::new(w, h)?;
    fp_snowman::paint_mascot(&mut pm, scale, side, waving);
    Some(pm)
}

/// Halo ring segments.
const HALO_SEGMENTS: usize = 16;
/// Curve samples per bank bezier.
const BANK_STEPS: usize = 24;
/// Halo colour rgb(0.1, 0.3, 0.7) (snow-sim 3.17).
const HALO_RGB: [u8; 3] = [26, 77, 179];

/// Halo radius of a flake of radius `r` (snow-sim 3.17).
pub(crate) fn halo_radius(r: f64) -> f32 {
    (r + 2.0) as f32
}

fn scaled_alpha(base: f32, opacity: f32) -> u8 {
    (base * opacity.clamp(0.0, 1.0)).round() as u8
}

/// Triangle-fan stand-in for the radial halo gradient: alpha 64/255 at the
/// centre falling to 0 at `radius`.
fn halo_mesh(c: egui::Pos2, radius: f32, opacity: f32) -> egui::Mesh {
    let [r, g, b] = HALO_RGB;
    let mut m = egui::Mesh::default();
    m.colored_vertex(c, Color32::from_rgba_unmultiplied(r, g, b, scaled_alpha(64.0, opacity)));
    for i in 0..HALO_SEGMENTS {
        let a = i as f32 / HALO_SEGMENTS as f32 * std::f32::consts::TAU;
        m.colored_vertex(c + vec2(a.cos(), a.sin()) * radius, Color32::from_rgba_unmultiplied(r, g, b, 0));
    }
    for i in 0..HALO_SEGMENTS as u32 {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % HALO_SEGMENTS as u32);
    }
    m
}

/// Falling flakes over `rect` (not hit-testable): `fp_snow::decor::dots`,
/// each with its halo first, then the white disc (snow-sim 3.17).
pub(crate) fn paint_snow(ui: &Ui, rect: Rect, count: usize, t: f64, opacity: f32) {
    for d in fp_snow::decor::dots(count, f64::from(rect.width()), f64::from(rect.height()), t) {
        let c = pos2(rect.left() + d.x as f32, rect.top() + d.y as f32);
        ui.painter().add(egui::Shape::mesh(halo_mesh(c, halo_radius(d.radius), opacity)));
        let fill = Color32::from_white_alpha((255.0 * d.alpha * f64::from(opacity)) as u8);
        ui.painter().circle_filled(c, d.radius as f32, fill);
    }
}

fn cubic(p: [(f32, f32); 4], t: f32) -> (f32, f32) {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    (
        w[0] * p[0].0 + w[1] * p[1].0 + w[2] * p[2].0 + w[3] * p[3].0,
        w[0] * p[0].1 + w[1] * p[1].1 + w[2] * p[2].1 + w[3] * p[3].1,
    )
}

/// Top edge of the snow bank in local points (origin top-left, y down) for a
/// `w` x `h` bank: three cubic beziers (snow-sim 3.17), `BANK_STEPS` samples
/// each, joints included once.
pub(crate) fn bank_path(w: f32, h: f32) -> Vec<egui::Pos2> {
    const CURVES: [[(f32, f32); 4]; 3] = [
        [(0.0, 0.42), (0.12, 0.05), (0.26, 0.05), (0.38, 0.30)],
        [(0.38, 0.30), (0.5, 0.55), (0.6, 0.1), (0.72, 0.34)],
        [(0.72, 0.34), (0.84, 0.58), (0.94, 0.18), (1.0, 0.22)],
    ];
    let mut out = vec![pos2(0.0, 0.42 * h)];
    for c in CURVES {
        for i in 1..=BANK_STEPS {
            let (x, y) = cubic(c, i as f32 / BANK_STEPS as f32);
            out.push(pos2(x * w, y * h));
        }
    }
    out
}

/// Fill of the bank: vertical strips from each top sample down to `h`
/// (the shape is concave, so a convex fill would be wrong).
fn bank_mesh(path: &[egui::Pos2], h: f32, color: Color32) -> egui::Mesh {
    let mut m = egui::Mesh::default();
    for p in path {
        m.colored_vertex(*p, color);
        m.colored_vertex(pos2(p.x, h), color);
    }
    for i in 0..path.len().saturating_sub(1) as u32 {
        let k = i * 2;
        m.add_triangle(k, k + 1, k + 2);
        m.add_triangle(k + 1, k + 3, k + 2);
    }
    m
}

/// A snow bank of `height` at the bottom of `rect`: white fill with a 1.5 px
/// Frost (#CCDBF2, 0.7) stroke on the top edge; `opacity` scales both.
pub(crate) fn paint_bank(ui: &Ui, rect: Rect, height: f32, opacity: f32) {
    let painter = ui.painter().with_clip_rect(rect);
    let origin = pos2(rect.left(), rect.bottom() - height);
    let local = bank_path(rect.width(), height);
    let mut mesh = bank_mesh(&local, height, Color32::from_white_alpha(scaled_alpha(255.0, opacity)));
    mesh.translate(origin.to_vec2());
    painter.add(egui::Shape::mesh(mesh));
    let top: Vec<_> = local.iter().map(|p| origin + p.to_vec2()).collect();
    let stroke = egui::Stroke::new(1.5, Color32::from_rgba_unmultiplied(204, 219, 242, scaled_alpha(179.0, opacity)));
    painter.add(egui::Shape::line(top, stroke));
}

#[cfg(test)]
mod decor_tests {
    use super::*;

    #[test]
    fn halo_is_two_points_wider_than_flake() {
        assert_eq!(halo_radius(3.0), 5.0);
        let m = halo_mesh(pos2(10.0, 10.0), halo_radius(3.0), 1.0);
        assert_eq!(m.vertices.len(), 1 + HALO_SEGMENTS);
        assert_eq!(m.vertices[0].color, Color32::from_rgba_unmultiplied(26, 77, 179, 64));
        let r = (m.vertices[1].pos - pos2(10.0, 10.0)).length();
        assert!((r - 5.0).abs() < 1e-4);
        assert_eq!(m.vertices[1].color, Color32::TRANSPARENT);
        assert_eq!(halo_mesh(pos2(0.0, 0.0), 1.0, 0.5).vertices[0].color.a(), 32);
    }

    #[test]
    fn bank_path_hits_spec_joints() {
        let (w, h) = (200.0, 34.0);
        let p = bank_path(w, h);
        assert_eq!(p.len(), 1 + 3 * BANK_STEPS);
        let near = |a: egui::Pos2, x: f32, y: f32| (a.x - x * w).abs() < 1e-3 && (a.y - y * h).abs() < 1e-3;
        assert!(near(p[0], 0.0, 0.42));
        assert!(near(p[BANK_STEPS], 0.38, 0.30));
        assert!(near(p[2 * BANK_STEPS], 0.72, 0.34));
        assert!(near(p[3 * BANK_STEPS], 1.0, 0.22));
    }

    #[test]
    fn bank_mesh_strips_reach_bottom() {
        let p = bank_path(100.0, 20.0);
        let m = bank_mesh(&p, 20.0, Color32::WHITE);
        assert_eq!(m.vertices.len(), p.len() * 2);
        assert_eq!(m.indices.len(), (p.len() - 1) * 6);
        assert!(m.vertices.iter().skip(1).step_by(2).all(|v| v.pos.y == 20.0));
        assert!(bank_mesh(&[], 1.0, Color32::WHITE).is_empty());
    }
}
