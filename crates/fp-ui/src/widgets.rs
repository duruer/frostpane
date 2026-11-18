//! Shared controls (ui 2.6, 3.9): buttons, switch, two-way pill, tiles,
//! dividers, vector glyphs. Everything is drawn with the egui painter.

use crate::model::Glyph;
use crate::theme::{self, Tokens};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

/// Font id for a size.
pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

/// A single-line label.
pub fn label(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    ui.add(egui::Label::new(egui::RichText::new(text).size(size).color(color)).selectable(false))
}

/// A wrapped label (fixed line height `ceil(size * 1.22)`).
pub fn wrapped(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text).size(size).color(color).line_height(Some(theme::line_height(size))))
            .wrap()
            .selectable(false),
    )
}

/// A wrapped label in a faux-bold weight: egui ships one face, so the glyphs
/// are drawn twice, half a point apart.
pub fn wrapped_bold(ui: &mut Ui, text: &str, size: f32, color: Color32) -> Response {
    let job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping::wrap_at_width(ui.available_width()),
        ..egui::text::LayoutJob::single_section(
            text.to_string(),
            egui::TextFormat {
                font_id: font(size),
                color,
                line_height: Some(theme::line_height(size)),
                ..Default::default()
            },
        )
    };
    let galley = ui.painter().layout_job(job);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min, galley.clone(), color);
    ui.painter().galley(rect.min + vec2(0.5, 0.0), galley, color);
    resp
}

/// Upper-cased 11 pt section title (ui 2.5 SectionTitle).
pub fn section_title(ui: &mut Ui, tk: &Tokens, text: &str) -> Response {
    label(ui, &text.to_uppercase(), 11.0, tk.secondary())
}

/// 1 px divider across the available width.
pub fn divider(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, color);
}

/// Style of a pill button.
#[derive(Clone, Copy)]
pub struct PillStyle {
    pub height: f32,
    pub pad: f32,
    pub size: f32,
    pub fill: Color32,
    pub pressed_fill: Color32,
    pub hover_fill: Color32,
    pub border: Option<Color32>,
    pub text: Color32,
}

/// Generic pill button; returns the response (clicked = `clicked()`).
pub fn pill(ui: &mut Ui, text: &str, st: PillStyle, enabled: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), font(st.size), st.text);
    let w = (galley.size().x + 2.0 * st.pad).max(if st.height >= 36.0 { 96.0 } else { 0.0 });
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, st.height), sense);
    let fill = if !enabled {
        st.fill
    } else if resp.is_pointer_button_down_on() {
        st.pressed_fill
    } else if resp.hovered() {
        st.hover_fill
    } else {
        st.fill
    };
    let op = if enabled { 1.0 } else { 0.5 };
    // Pressed: draw at 0.96 scale.
    let rect = if enabled && resp.is_pointer_button_down_on() {
        Rect::from_center_size(rect.center(), rect.size() * 0.96)
    } else {
        rect
    };
    let r = rect.height() / 2.0;
    ui.painter().rect_filled(rect, r, fill.gamma_multiply(op));
    if let Some(b) = st.border {
        ui.painter().rect_stroke(rect, r, Stroke::new(1.0, b.gamma_multiply(op)), StrokeKind::Inside);
    }
    let pos = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos, galley, st.text.gamma_multiply(op));
    if enabled { resp.on_hover_cursor(CursorIcon::PointingHand) } else { resp }
}

pub fn primary_button(ui: &mut Ui, tk: &Tokens, text: &str, enabled: bool) -> Response {
    let a = tk.accent();
    pill(
        ui,
        text,
        PillStyle {
            height: 36.0,
            pad: 22.0,
            size: 13.0,
            fill: a,
            pressed_fill: lerp_color(a, Color32::BLACK, 0.18),
            hover_fill: lerp_color(a, Color32::WHITE, 0.22),
            border: None,
            text: Color32::WHITE,
        },
        enabled,
    )
}

pub fn small_primary(ui: &mut Ui, tk: &Tokens, text: &str, enabled: bool) -> Response {
    let a = tk.accent();
    pill(
        ui,
        text,
        PillStyle {
            height: 32.0,
            pad: 14.0,
            size: 13.0,
            fill: a,
            pressed_fill: a.gamma_multiply(0.8),
            hover_fill: lerp_color(a, Color32::WHITE, 0.22),
            border: None,
            text: Color32::WHITE,
        },
        enabled,
    )
}

pub fn quiet_button(ui: &mut Ui, tk: &Tokens, text: &str, height: f32, enabled: bool) -> Response {
    pill(
        ui,
        text,
        PillStyle {
            height,
            pad: if height > 32.0 { 18.0 } else { 14.0 },
            size: 13.0,
            fill: tk.ink10(),
            pressed_fill: tk.ink16(),
            hover_fill: tk.ink16().gamma_multiply(0.7),
            border: None,
            text: tk.ink(),
        },
        enabled,
    )
}

pub fn ghost_button(ui: &mut Ui, tk: &Tokens, text: &str) -> Response {
    pill(
        ui,
        text,
        PillStyle {
            height: 32.0,
            pad: 14.0,
            size: 13.0,
            fill: Color32::TRANSPARENT,
            pressed_fill: tk.ink10(),
            hover_fill: tk.ink16().gamma_multiply(0.7),
            border: Some(tk.ink20()),
            text: tk.ink(),
        },
        true,
    )
}

/// Chrome-less link text.
pub fn link_button(ui: &mut Ui, color: Color32, text: &str, size: f32) -> Response {
    let galley = ui.painter().layout_no_wrap(text.to_string(), font(size), color);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + vec2(0.0, 4.0), Sense::click());
    let c = if resp.is_pointer_button_down_on() { color.gamma_multiply(0.7) } else { color };
    ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, c);
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// The switch (ui 3.9). Returns `Some(new value)` on a user flip.
pub fn switch(ui: &mut Ui, tk: &Tokens, on: bool, big: bool, enabled: bool) -> Option<bool> {
    let (w, h, kw) = if big { (62.0, 30.0, 38.0) } else { (40.0, 22.0, 16.0) };
    let kh = if big { 24.0 } else { 16.0 };
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), sense);
    let t = ui.ctx().animate_bool_with_easing(resp.id, on, |t| crate::tween::ease_out_quint(f64::from(t)) as f32);
    let off_fill = tk.ink_a(if big { 0.26 } else { 0.20 });
    let op = if enabled { 1.0 } else { 0.5 };
    let fill = lerp_color(off_fill, tk.accent(), t).gamma_multiply(op);
    ui.painter().rect_filled(rect, h / 2.0, fill);
    if big {
        let a = f64::from((1.0 - t) * 0.35);
        ui.painter().rect_stroke(rect, h / 2.0, Stroke::new(1.0, tk.ink_a(a)), StrokeKind::Inside);
    }
    let x0 = rect.left() + 3.0;
    let x1 = rect.right() - kw - 3.0;
    let kx = x0 + (x1 - x0) * t;
    let knob = Rect::from_min_size(pos2(kx, rect.center().y - kh / 2.0), vec2(kw, kh));
    ui.painter().rect_filled(knob, kh / 2.0, lerp_color(Color32::from_gray(225), Color32::WHITE, t).gamma_multiply(op));
    if enabled && resp.clicked() {
        return Some(!on);
    }
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand);
    }
    None
}

/// Linear colour mix.
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// Segmented two-option control (ui 3.9 TwoWays). `Some(second)` on a click.
///
/// `id_salt` must differ between instances that share a parent `Ui` (the
/// effect tiles are sibling `vertical` children with the same `Ui::id`, so
/// an unsalted id clashes between the snow and the rain pill).
pub fn two_ways(
    ui: &mut Ui,
    tk: &Tokens,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    labels: [&str; 2],
    second: bool,
) -> Option<bool> {
    two_ways_state(ui, tk, id_salt, labels, second, true)
}

/// [`two_ways`] with the parent effect's state: the selection is muted grey
/// while the effect is off, accent blue only while it is on.
pub fn two_ways_state(
    ui: &mut Ui,
    tk: &Tokens,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    labels: [&str; 2],
    second: bool,
    active: bool,
) -> Option<bool> {
    let mut result = None;
    let galleys: Vec<_> =
        labels.iter().map(|l| ui.painter().layout_no_wrap((*l).to_string(), font(11.0), tk.ink75())).collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 20.0).collect();
    let total = widths[0] + widths[1] + 2.0 + 4.0;
    let (outer, _) = ui.allocate_exact_size(vec2(total, 30.0), Sense::hover());
    let inner = Rect::from_min_size(outer.min, vec2(total, 26.0));
    ui.painter().rect_filled(inner, 13.0, tk.ink07());
    let mut x = inner.left() + 2.0;
    for (i, g) in galleys.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, inner.top() + 2.0), vec2(widths[i], 22.0));
        let id = ui.make_persistent_id(("two_ways", &id_salt, i));
        let resp = ui.interact(r, id, Sense::click());
        let on = (i == 1) == second;
        let t = ui.ctx().animate_bool_with_easing(id, on, |t| crate::tween::ease_out_quint(f64::from(t)) as f32);
        if resp.hovered() && !on {
            // Outline + light fill: distinct from the filled (selected) look.
            ui.painter().rect_filled(r, 11.0, tk.ink10());
            ui.painter().rect_stroke(r, 11.0, Stroke::new(1.0, tk.ink_a(0.55)), StrokeKind::Inside);
        }
        let sel = if active { tk.accent() } else { tk.ink_a(0.38) };
        ui.painter().rect_filled(r, 11.0, sel.gamma_multiply(t));
        let color = lerp_color(tk.ink75(), if active { Color32::WHITE } else { tk.ink() }, t);
        let pos = r.center() - g.size() / 2.0;
        ui.painter().galley(pos, g, color);
        if resp.clicked() {
            result = Some(i == 1);
        }
        resp.on_hover_cursor(CursorIcon::PointingHand);
        x += widths[i] + 2.0;
    }
    result
}

/// "ON" badge (ui 3.1).
pub fn on_badge(ui: &mut Ui, tk: &Tokens, text: &str) {
    let g = ui.painter().layout_no_wrap(text.to_string(), font(9.0), Color32::WHITE);
    let (rect, _) = ui.allocate_exact_size(g.size() + vec2(10.0, 4.0), Sense::hover());
    ui.painter().rect_filled(rect, 7.0, tk.accent());
    ui.painter().galley(rect.center() - g.size() / 2.0, g, Color32::WHITE);
}

/// Vector glyph drawn into a `size` box centred at `c`.
pub fn glyph(painter: &egui::Painter, g: Glyph, c: Pos2, size: f32, color: Color32) {
    let s = size / 2.0;
    let stroke = Stroke::new((size / 12.0).max(1.2), color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([pos2(c.x + a.0 * s, c.y + a.1 * s), pos2(c.x + b.0 * s, c.y + b.1 * s)], stroke);
    };
    match g {
        Glyph::Gear => {
            painter.circle_stroke(c, s * 0.42, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                line((cs * 0.62, sn * 0.62), (cs * 0.92, sn * 0.92));
            }
        }
        Glyph::Monitor | Glyph::Screens => {
            painter.rect_stroke(
                Rect::from_center_size(pos2(c.x, c.y - s * 0.1), vec2(size * 0.9, size * 0.6)),
                2.0,
                stroke,
                StrokeKind::Middle,
            );
            line((0.0, 0.55), (0.0, 0.85));
            line((-0.4, 0.85), (0.4, 0.85));
        }
        Glyph::Shield => {
            let pts = [(-0.7, -0.7), (0.0, -0.95), (0.7, -0.7), (0.7, 0.1), (0.0, 0.95), (-0.7, 0.1), (-0.7, -0.7)];
            for w in pts.windows(2) {
                line(w[0], w[1]);
            }
        }
        Glyph::Question => {
            painter.circle_stroke(c, s * 0.9, stroke);
            painter.text(c, Align2::CENTER_CENTER, "?", font(size * 0.7), color);
        }
        Glyph::Check => {
            line((-0.6, 0.05), (-0.15, 0.5));
            line((-0.15, 0.5), (0.65, -0.45));
        }
        Glyph::Chevron => {
            line((-0.5, -0.25), (0.0, 0.25));
            line((0.0, 0.25), (0.5, -0.25));
        }
        Glyph::Sun => {
            painter.circle_stroke(c, s * 0.36, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let (sn, cs) = a.sin_cos();
                line((cs * 0.62, sn * 0.62), (cs * 0.92, sn * 0.92));
            }
        }
        Glyph::Minimise => line((-0.7, 0.0), (0.7, 0.0)),
        Glyph::Close => {
            line((-0.6, -0.6), (0.6, 0.6));
            line((-0.6, 0.6), (0.6, -0.6));
        }
        Glyph::Battery => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(size * 0.9, size * 0.5)),
                2.0,
                stroke,
                StrokeKind::Middle,
            );
            line((1.0, -0.2), (1.0, 0.2));
        }
        Glyph::Behind => {
            painter.rect_stroke(
                Rect::from_center_size(c + vec2(s * 0.2, -s * 0.2), vec2(s, s)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
            painter.rect_stroke(
                Rect::from_center_size(c + vec2(-s * 0.2, s * 0.2), vec2(s, s)),
                1.5,
                stroke,
                StrokeKind::Middle,
            );
        }
        Glyph::SnowOnly => {
            // Six arms, each with a small V branch near the tip.
            for i in 0..6 {
                let a = i as f32 * std::f32::consts::FRAC_PI_3 - std::f32::consts::FRAC_PI_2;
                let (sn, cs) = a.sin_cos();
                line((0.0, 0.0), (cs * 0.9, sn * 0.9));
                let (bx, by) = (cs * 0.55, sn * 0.55);
                for d in [-0.6_f32, 0.6] {
                    let (ds, dc) = (a + d).sin_cos();
                    line((bx, by), (bx + dc * 0.32, by + ds * 0.32));
                }
            }
        }
    }
}

/// Picture tile frame (ui 3.9): rounded, Ink06 background, edge border,
/// optional selection ring. `paint` draws the picture into the rect.
pub fn tile(
    ui: &mut Ui,
    tk: &Tokens,
    width: f32,
    aspect: f32,
    ringed: bool,
    paint: impl FnOnce(&Ui, Rect),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, width / aspect), Sense::click());
    ui.painter().rect_filled(rect, 12.0, tk.ink06());
    paint(ui, rect);
    let p = ui.painter();
    if resp.hovered() {
        // Visible on bright pictures: white wash plus accent-tinted 2px stroke.
        p.rect_filled(rect, 12.0, Color32::from_white_alpha(18));
        p.rect_stroke(rect, 12.0, Stroke::new(2.0, tk.accent()), StrokeKind::Inside);
    } else {
        p.rect_stroke(rect, 12.0, Stroke::new(1.0, tk.ink10()), StrokeKind::Inside);
    }
    let t = ui
        .ctx()
        .animate_bool_with_easing(resp.id.with("ring"), ringed, |t| crate::tween::ease_out_quint(f64::from(t)) as f32);
    if t > 0.0 {
        p.rect_stroke(rect, 12.0, Stroke::new(2.0, tk.accent().gamma_multiply(0.75 * t)), StrokeKind::Inside);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand)
}

/// Equal-width grid cell width for `n` columns with `gap` (ui 3.9 EvenGrid).
pub fn cell_width(total: f32, n: usize, gap: f32) -> f32 {
    let n = n.max(1) as f32;
    let w = (total - gap * (n - 1.0)) / n;
    if w.is_finite() { w.max(1.0) } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_cell_width() {
        assert_eq!(cell_width(1000.0, 3, 16.0), (1000.0 - 32.0) / 3.0);
        assert_eq!(cell_width(1.0, 4, 100.0), 1.0);
        assert_eq!(cell_width(50.0, 0, 16.0), 50.0);
        assert_eq!(cell_width(f32::NAN, 3, 16.0), 1.0);
    }

    #[test]
    fn colour_mix() {
        let a = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
        let b = Color32::from_rgba_unmultiplied(100, 200, 50, 255);
        assert_eq!(lerp_color(a, b, 0.0), Color32::TRANSPARENT);
        assert_eq!(lerp_color(Color32::BLACK, Color32::WHITE, 1.0), Color32::WHITE);
        let mid = lerp_color(Color32::BLACK, Color32::WHITE, 0.5);
        assert_eq!(mid.r(), 128);
    }

    /// Run one headless frame through every widget so layout code executes.
    #[test]
    fn widgets_run_in_a_frame() {
        let ctx = egui::Context::default();
        let tk = Tokens::new(true);
        let mut flipped = None;
        crate::testutil::frame(&ctx, |ui| {
            label(ui, "a", 12.0, tk.ink());
            wrapped(ui, "b b b", 12.0, tk.ink());
            section_title(ui, &tk, "title");
            divider(ui, tk.divider());
            assert!(!primary_button(ui, &tk, "go", true).clicked());
            small_primary(ui, &tk, "go", false);
            quiet_button(ui, &tk, "q", 36.0, true);
            quiet_button(ui, &tk, "q", 30.0, true);
            ghost_button(ui, &tk, "g");
            link_button(ui, tk.link(), "l", 11.0);
            flipped = switch(ui, &tk, false, true, true);
            switch(ui, &tk, true, false, false);
            two_ways(ui, &tk, 1, ["a", "b"], true);
            two_ways(ui, &tk, 2, ["a", "b"], false);
            on_badge(ui, &tk, "ON");
            for g in [
                Glyph::Gear,
                Glyph::Monitor,
                Glyph::Shield,
                Glyph::Question,
                Glyph::Check,
                Glyph::Chevron,
                Glyph::Sun,
                Glyph::Minimise,
                Glyph::Close,
                Glyph::Battery,
                Glyph::Behind,
                Glyph::Screens,
                Glyph::SnowOnly,
            ] {
                glyph(ui.painter(), g, pos2(10.0, 10.0), 16.0, Color32::WHITE);
            }
            tile(ui, &tk, 120.0, 1.6, true, |_, _| {});
        });
        assert_eq!(flipped, None);
    }
}
