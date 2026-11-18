//! Procedural 2D art toolkit shared by every CPU-drawn object.
//!
//! Spec: core-app 2.9 (colour helpers, brand colours), 2.18 (Trace path
//! builder), characters 6 (Sketch), decorations 4 / trees 2 (Strand),
//! core-app 2.6 + snow-sim 3.4 UI (menu icons by name, wind icons).
//!
//! Art convention: drawing code works in **Y-up points** relative to the
//! sprite box origin (bottom-left); [`Sketch`] applies the flip and the
//! monitor scale so the result lands in a premultiplied RGBA pixmap.

use std::f32::consts::{FRAC_PI_2, PI};
pub use tiny_skia;
use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, Point,
    RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

/// Colour from 0..1 floats; channel bytes use round-half-to-even like the
/// original palette (`0.7 -> 0xB2`).
pub fn rgb(r: f64, g: f64, b: f64) -> Color {
    rgba(r, g, b, 1.0)
}

pub fn rgba(r: f64, g: f64, b: f64, a: f64) -> Color {
    use fp_core::math::color_byte;
    Color::from_rgba8(color_byte(r), color_byte(g), color_byte(b), color_byte(a))
}

/// `0xRRGGBB` to colour.
pub fn hex(rgb: u32) -> Color {
    Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255)
}

/// Same RGB with alpha = `round(alpha * opacity)` (core-app 2.9 `Solid`),
/// rounding half to even like the colour bytes. NaN opacity gives alpha 0.
pub fn solid(color: Color, opacity: f64) -> Color {
    let c = color.to_color_u8();
    let a = (f64::from(c.alpha()) * opacity.clamp(0.0, 1.0)).round_ties_even() as u8;
    Color::from_rgba8(c.red(), c.green(), c.blue(), a)
}

/// Brand palette (Frostpane's own accent values may differ from the spec's).
pub mod brand {
    pub const ACCENT: u32 = 0x1F70E0;
    pub const FROST: u32 = 0xCCDBF2;
    /// Vertical "snow day" gradient, top then bottom.
    pub const SNOW_DAY: (u32, u32) = (0xDBEDFF, 0x8CBDFA);
    /// Corner radius of cards and panels, in points.
    pub const RADIUS: f32 = 12.0;
}

/// Per-weather menu tints (core-app 2.6; bytes are round-half-to-even).
pub mod menu {
    pub const SNOW: u32 = 0x59B2FF;
    pub const RAIN: u32 = 0x4080FA;
    pub const SAKURA: u32 = 0xFF73A8;
    pub const LEAVES: u32 = 0xF28C26;
    pub const FIREFLIES: u32 = 0xF2C71A;
    pub const STARFALL: u32 = 0x9E73FF;
    pub const GRAY: u32 = 0x8E8E93;
    pub const HEAVY_BLUE: u32 = 0x2973F2;
}

/// Upper bound on arc segments (256 full turns); larger sweeps are clamped.
const MAX_ARC_SEGMENTS: f64 = 1024.0;

/// Path builder with the spec's semantics (core-app 2.18): Move starts a
/// filled sub-path, Arc emits clockwise segments of at most 90 degrees.
pub struct Trace {
    pb: PathBuilder,
    open: bool,
    last: (f32, f32),
    start: (f32, f32),
}

impl Default for Trace {
    fn default() -> Self {
        Self::new()
    }
}

impl Trace {
    pub fn new() -> Self {
        Self { pb: PathBuilder::new(), open: false, last: (0.0, 0.0), start: (0.0, 0.0) }
    }

    /// Starts a new sub-path.
    pub fn move_to(&mut self, x: f32, y: f32) -> &mut Self {
        self.pb.move_to(x, y);
        self.open = true;
        self.last = (x, y);
        self.start = (x, y);
        self
    }

    /// Line/Quad/Curve with no open sub-path first start one at the last point.
    fn ensure_open(&mut self) {
        if !self.open {
            let (x, y) = self.last;
            self.move_to(x, y);
        }
    }

    pub fn line(&mut self, x: f32, y: f32) -> &mut Self {
        self.ensure_open();
        self.pb.line_to(x, y);
        self.last = (x, y);
        self
    }

    /// First point is a Move, the rest are Lines.
    pub fn lines(&mut self, points: &[(f32, f32)]) -> &mut Self {
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                self.move_to(x, y);
            } else {
                self.line(x, y);
            }
        }
        self
    }

    /// Quadratic: end point first, then control (characters Q5).
    pub fn quad(&mut self, x: f32, y: f32, cx: f32, cy: f32) -> &mut Self {
        self.ensure_open();
        self.pb.quad_to(cx, cy, x, y);
        self.last = (x, y);
        self
    }

    /// Cubic: end point, then the two controls.
    pub fn curve(&mut self, x: f32, y: f32, c1x: f32, c1y: f32, c2x: f32, c2y: f32) -> &mut Self {
        self.ensure_open();
        self.pb.cubic_to(c1x, c1y, c2x, c2y, x, y);
        self.last = (x, y);
        self
    }

    /// Circular arc from angle `start` to `end` (radians) around `(cx, cy)`.
    /// The first point is Move (no open sub-path) or Line (open). The sweep is
    /// split into `max(1, ceil(|end-start| / 90deg))` clockwise small arcs
    /// between evenly spaced angles; a negative sweep therefore bulges the
    /// other way (core-app 2.18 quirk).
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, start: f32, end: f32) -> &mut Self {
        let (cx, cy, r) = (f64::from(cx), f64::from(cy), f64::from(r));
        let (start, end) = (f64::from(start), f64::from(end));
        let at = |a: f64| (cx + r * a.cos(), cy + r * a.sin());
        let (x0, y0) = at(start);
        if self.open {
            self.line(x0 as f32, y0 as f32);
        } else {
            self.move_to(x0 as f32, y0 as f32);
        }
        // The epsilon keeps a full circle given as an f32 2*PI at 4 segments.
        let n = ((end - start).abs() / std::f64::consts::FRAC_PI_2 - 1e-5).ceil().max(1.0);
        // A non-finite sweep draws only the start point; absurd sweeps are
        // capped so a bad angle cannot stall the caller.
        if !n.is_finite() {
            return self;
        }
        let n = n.min(MAX_ARC_SEGMENTS);
        let d = (end - start) / n;
        let sweep = d.abs();
        let kappa = 4.0 / 3.0 * (sweep / 4.0).tan();
        for k in 0..n as u32 {
            let a0 = start + f64::from(k) * d;
            let (p0x, p0y) = at(a0);
            let (p1x, p1y) = at(a0 + d);
            // Clockwise small arc: for a negative sweep the centre is the
            // point reflection of the true centre through the chord midpoint.
            let (ox, oy) = if d >= 0.0 { (cx, cy) } else { (p0x + p1x - cx, p0y + p1y - cy) };
            let t0 = (p0y - oy).atan2(p0x - ox);
            let t1 = t0 + sweep;
            let c1 = (p0x - kappa * r * t0.sin(), p0y + kappa * r * t0.cos());
            let c2 = (p1x + kappa * r * t1.sin(), p1y - kappa * r * t1.cos());
            self.curve(p1x as f32, p1y as f32, c1.0 as f32, c1.1 as f32, c2.0 as f32, c2.1 as f32);
        }
        self
    }

    /// Closes the current sub-path; no-op if none.
    pub fn close(&mut self) -> &mut Self {
        if self.open {
            self.pb.close();
            self.open = false;
            self.last = self.start;
        }
        self
    }

    /// Frozen geometry; `None` when nothing was drawn.
    pub fn finish(self) -> Option<Path> {
        self.pb.finish()
    }
}

#[derive(Clone)]
struct State {
    ctm: Transform,
    clip: Option<Mask>,
}

/// Immediate-mode drawing on a pixmap in Y-up points with a transform stack
/// (characters 6 `Sketch`). Strokes use round caps and round joins; fills are
/// non-zero winding.
pub struct Sketch<'a> {
    pixmap: &'a mut Pixmap,
    base: Transform,
    state: State,
    stack: Vec<State>,
}

impl<'a> Sketch<'a> {
    /// `scale` = monitor scale; the pixmap is `ceil(box * scale)` pixels.
    pub fn new(pixmap: &'a mut Pixmap, scale: f32) -> Self {
        let base = Transform::from_row(scale, 0.0, 0.0, -scale, 0.0, pixmap.height() as f32);
        Self { pixmap, base, state: State { ctm: Transform::identity(), clip: None }, stack: Vec::new() }
    }

    pub fn save(&mut self) {
        self.stack.push(self.state.clone());
    }

    /// Pops the last [`save`](Self::save); a restore without a save is a no-op.
    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.state.ctm = self.state.ctm.pre_translate(x, y);
    }

    /// Counter-clockwise in Y-up space.
    pub fn rotate(&mut self, radians: f32) {
        let (s, c) = radians.sin_cos();
        self.state.ctm = self.state.ctm.pre_concat(Transform::from_row(c, s, -s, c, 0.0, 0.0));
    }

    pub fn scale_by(&mut self, sx: f32, sy: f32) {
        self.state.ctm = self.state.ctm.pre_scale(sx, sy);
    }

    /// Applies `t` (e.g. from [`pose`]) on top of the current transform.
    pub fn concat(&mut self, t: Transform) {
        self.state.ctm = self.state.ctm.pre_concat(t);
    }

    /// Save, translate, rotate when `turn != 0`, scale when `scale != 1`,
    /// draw, restore.
    pub fn at(&mut self, x: f32, y: f32, turn: f32, scale: f32, draw: impl FnOnce(&mut Self)) {
        self.enter(x, y, turn, scale);
        draw(self);
        self.restore();
    }

    fn enter(&mut self, x: f32, y: f32, turn: f32, scale: f32) {
        self.save();
        self.translate(x, y);
        if turn != 0.0 {
            self.rotate(turn);
        }
        if scale != 1.0 {
            self.scale_by(scale, scale);
        }
    }

    /// Intersects the clip with `path` (until the matching restore).
    pub fn clip(&mut self, path: &Path) {
        let t = self.xf();
        let (w, h) = (self.pixmap.width(), self.pixmap.height());
        let mask = match self.state.clip.take() {
            Some(mut m) => {
                m.intersect_path(path, FillRule::Winding, true, t);
                m
            }
            None => {
                let mut m = Mask::new(w, h).expect("pixmap has a size");
                m.fill_path(path, FillRule::Winding, true, t);
                m
            }
        };
        self.state.clip = Some(mask);
    }

    /// Runs `draw` with `path` as an extra clip.
    pub fn inside(&mut self, path: &Path, draw: impl FnOnce(&mut Self)) {
        self.save();
        self.clip(path);
        draw(self);
        self.restore();
    }

    fn xf(&self) -> Transform {
        self.base.pre_concat(self.state.ctm)
    }

    fn paint_with(&mut self, path: &Path, shader: Shader<'_>) {
        let paint = Paint { shader, anti_alias: true, ..Paint::default() };
        let t = self.xf();
        self.pixmap.fill_path(path, &paint, FillRule::Winding, t, self.state.clip.as_ref());
    }

    pub fn fill(&mut self, path: &Path, color: Color) {
        self.paint_with(path, Shader::SolidColor(color));
    }

    pub fn stroke(&mut self, path: &Path, color: Color, width: f32) {
        let mut paint = Paint::default();
        paint.set_color(color);
        let stroke = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        let t = self.xf();
        self.pixmap.stroke_path(path, &paint, &stroke, t, self.state.clip.as_ref());
    }

    pub fn circle(&mut self, x: f32, y: f32, r: f32, color: Color) {
        if let Some(p) = PathBuilder::from_circle(x, y, r) {
            self.fill(&p, color);
        }
    }

    /// Bounding-box ellipse fill.
    pub fn ellipse(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if let Some(p) = oval_path(x, y, w, h) {
            self.fill(&p, color);
        }
    }

    /// Bounding-box ellipse outline.
    pub fn ring(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color, line_width: f32) {
        if let Some(p) = oval_path(x, y, w, h) {
            self.stroke(&p, color, line_width);
        }
    }

    /// Rounded rectangle fill; `radius` is clamped to half the short side.
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        if let Some(p) = rounded_rect(x, y, w, h, radius) {
            self.fill(&p, color);
        }
    }

    /// Rounded rectangle outline.
    pub fn rect_outline(&mut self, (x, y, w, h): (f32, f32, f32, f32), radius: f32, color: Color, line_width: f32) {
        if let Some(p) = rounded_rect(x, y, w, h, radius) {
            self.stroke(&p, color, line_width);
        }
    }

    /// Stroked segment.
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Color, width: f32) {
        self.polyline(&[(x0, y0), (x1, y1)], color, width);
    }

    /// Stroked open polyline.
    pub fn polyline(&mut self, points: &[(f32, f32)], color: Color, width: f32) {
        let mut t = Trace::new();
        t.lines(points);
        if let Some(p) = t.finish() {
            self.stroke(&p, color, width);
        }
    }

    /// Filled closed polygon.
    pub fn polygon(&mut self, points: &[(f32, f32)], color: Color) {
        let mut t = Trace::new();
        t.lines(points).close();
        if let Some(p) = t.finish() {
            self.fill(&p, color);
        }
    }

    /// Linear gradient fill in the current user space; `stops` default to even
    /// spacing. Nothing is drawn for an empty colour list.
    pub fn across(&mut self, path: &Path, colors: &[Color], stops: Option<&[f32]>, from: (f32, f32), to: (f32, f32)) {
        let g = LinearGradient::new(
            Point::from_xy(from.0, from.1),
            Point::from_xy(to.0, to.1),
            gradient_stops(colors, stops),
            SpreadMode::Pad,
            Transform::identity(),
        );
        if let Some(shader) = g {
            self.paint_with(path, shader);
        }
    }

    /// Two-colour vertical gradient over the path's bounds (core-app 2.9
    /// `Vertical`): `top` at the highest Y (Y-up), `bottom` at the lowest.
    pub fn vertical(&mut self, path: &Path, top: Color, bottom: Color) {
        let b = path.bounds();
        let x = (b.left() + b.right()) / 2.0;
        self.across(path, &[top, bottom], None, (x, b.bottom()), (x, b.top()));
    }

    /// Radial gradient with a distinct highlight origin `light`.
    pub fn round(
        &mut self,
        path: &Path,
        colors: &[Color],
        stops: Option<&[f32]>,
        light: (f32, f32),
        center: (f32, f32),
        radius: f32,
    ) {
        let g = RadialGradient::new(
            Point::from_xy(light.0, light.1),
            0.0,
            Point::from_xy(center.0, center.1),
            radius,
            gradient_stops(colors, stops),
            SpreadMode::Pad,
            Transform::identity(),
        );
        if let Some(shader) = g {
            self.paint_with(path, shader);
        }
    }

    /// Soft glow: colour at the centre fading to the same RGB at alpha 0.
    pub fn halo(&mut self, x: f32, y: f32, radius: f32, color: Color) {
        let mut clear = color;
        clear.set_alpha(0.0);
        if let Some(p) = PathBuilder::from_circle(x, y, radius) {
            self.round(&p, &[color, clear], None, (x, y), (x, y), radius);
        }
    }
}

/// Layer matrix (characters 6 `Pose`): translate by `-anchor`, scale,
/// rotate by `angle` (counter-clockwise in Y-up), then translate to `at`.
pub fn pose(anchor: (f32, f32), scale: (f32, f32), angle: f32, at: (f32, f32)) -> Transform {
    let (s, c) = angle.sin_cos();
    Transform::from_translate(at.0, at.1)
        .pre_concat(Transform::from_row(c, s, -s, c, 0.0, 0.0))
        .pre_scale(scale.0, scale.1)
        .pre_translate(-anchor.0, -anchor.1)
}

fn gradient_stops(colors: &[Color], stops: Option<&[f32]>) -> Vec<GradientStop> {
    let last = colors.len().saturating_sub(1).max(1) as f32;
    colors
        .iter()
        .enumerate()
        .map(|(i, &c)| GradientStop::new(stops.and_then(|s| s.get(i).copied()).unwrap_or(i as f32 / last), c))
        .collect()
}

fn oval_path(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    tiny_skia::Rect::from_xywh(x, y, w, h).and_then(PathBuilder::from_oval)
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    let r = radius.clamp(0.0, (w.min(h) / 2.0).max(0.0));
    let mut t = Trace::new();
    t.move_to(x + r, y)
        .line(x + w - r, y)
        .arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0)
        .line(x + w, y + h - r)
        .arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2)
        .line(x + r, y + h)
        .arc(x + r, y + h - r, r, FRAC_PI_2, PI)
        .line(x, y + r)
        .arc(x + r, y + r, r, PI, PI + FRAC_PI_2)
        .close();
    t.finish()
}

/// String-lights rope (decorations 4, trees 2). Coordinates are the art box:
/// origin bottom-left, Y up, size `(length, dip + 46)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strand {
    pub length: f64,
    pub dip: f64,
}

impl Strand {
    pub const LIGHTS: Strand = Strand { length: 260.0, dip: 18.0 };

    /// Bulb tones, cycled by bulb index (decorations 4).
    pub const BULB_TONES: [(f64, f64, f64); 5] =
        [(1.00, 0.25, 0.30), (1.00, 0.78, 0.22), (0.30, 0.85, 0.45), (0.35, 0.62, 1.00), (1.00, 0.50, 0.82)];

    /// Clamps length to `[110, 4000]` and dip to `[0, 260]`; a NaN field
    /// (corrupt save) falls back to the [`LIGHTS`](Self::LIGHTS) value.
    pub fn fitted(self) -> Strand {
        let pick = |v: f64, d: f64, lo: f64, hi: f64| if v.is_nan() { d } else { v.clamp(lo, hi) };
        Strand {
            length: pick(self.length, Self::LIGHTS.length, 110.0, 4000.0),
            dip: pick(self.dip, Self::LIGHTS.dip, 0.0, 260.0),
        }
    }

    /// Bulb `i` tone: [`BULB_TONES`](Self::BULB_TONES)`[i mod 5]`.
    pub fn bulb_tone(i: usize) -> (f64, f64, f64) {
        Self::BULB_TONES[i % Self::BULB_TONES.len()]
    }

    /// Art box size: `(length, dip + 46)`.
    pub fn art_size(&self) -> (f64, f64) {
        (self.length, self.dip + 46.0)
    }

    /// Rope ends and quadratic control: `(start, control, end)`.
    pub fn anchors(&self) -> ((f64, f64), (f64, f64), (f64, f64)) {
        ((6.0, self.dip + 38.0), (self.length / 2.0, 38.0 - self.dip), (self.length - 6.0, self.dip + 38.0))
    }

    /// Point on the rope at `t` in `[0, 1]` (art box coordinates, Y up); the
    /// lowest point at `t = 0.5` is `(length / 2, 38)`.
    pub fn point(&self, t: f64) -> (f64, f64) {
        let (p0, c, p2) = self.anchors();
        let u = 1.0 - t;
        (u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p2.0, u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p2.1)
    }

    /// Tangent angle of the rope at `t` (radians, `atan2(dy, dx)`).
    pub fn tangent(&self, t: f64) -> f64 {
        let (p0, c, p2) = self.anchors();
        let u = 1.0 - t;
        let dx = 2.0 * u * (c.0 - p0.0) + 2.0 * t * (p2.0 - c.0);
        let dy = 2.0 * u * (c.1 - p0.1) + 2.0 * t * (p2.1 - c.1);
        dy.atan2(dx)
    }

    /// Bulb count `max(3, round_half_away((length - 12) / 25))`.
    pub fn bulb_count(&self) -> usize {
        (((self.length - 12.0) / 25.0).round() as usize).max(3)
    }

    /// Bulb `i`: position at `t = (i + 0.5) / n` and rotation `0.6 * tangent`.
    pub fn bulb(&self, i: usize) -> ((f64, f64), f64) {
        let t = (i as f64 + 0.5) / self.bulb_count() as f64;
        (self.point(t), 0.6 * self.tangent(t))
    }
}

/// Named menu/tray icons (core-app 4 list).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconName {
    Snow,
    Rain,
    Storm,
    Sakura,
    Leaves,
    Fireflies,
    Starfall,
    Plough,
    Snowman,
    Yeti,
    Santa,
    Train,
    Wind,
    Outside,
    Random,
    MinusCircle,
    Home,
    Logout,
    Bauble,
    Tree,
}

impl IconName {
    /// Icon for a factory name (core-app 4 list plus `bauble`, `tree`);
    /// weather raw names map directly (`snow`, `rain`, ...).
    pub fn from_name(name: &str) -> Option<IconName> {
        Some(match name {
            "snow" => IconName::Snow,
            "rain" => IconName::Rain,
            "storm" => IconName::Storm,
            "sakura" => IconName::Sakura,
            "leaves" => IconName::Leaves,
            "fireflies" => IconName::Fireflies,
            "starfall" => IconName::Starfall,
            "plough" => IconName::Plough,
            "snowman" => IconName::Snowman,
            "yeti" => IconName::Yeti,
            "santa" => IconName::Santa,
            "train" => IconName::Train,
            "wind" => IconName::Wind,
            "outside" => IconName::Outside,
            "random" => IconName::Random,
            "minus-circle" => IconName::MinusCircle,
            "home" => IconName::Home,
            "logout" => IconName::Logout,
            "bauble" => IconName::Bauble,
            "tree" => IconName::Tree,
            _ => return None,
        })
    }
}

/// Icon grid edge in points; art is drawn Y-up on a 24x24 grid.
const GRID: f32 = 24.0;
const ICON_STROKE: f32 = 1.8;

/// Renders an icon as straight (non-premultiplied) RGBA bytes, `size`x`size`.
pub fn icon_rgba(name: IconName, color: u32, size: u32) -> Vec<u8> {
    render_icon(size, color, |sk, c| draw_icon(sk, name, c))
}

/// Wind-setting icon (snow-sim 3.4): five streaks, tilt by strength, mirrored
/// by direction; `Changing` flips direction on streaks 1 and 3.
pub fn wind_icon_rgba(wind: fp_core::wind::Wind, color: u32, size: u32) -> Vec<u8> {
    let segs = wind_segments(wind);
    render_icon(size, color, |sk, c| {
        for ((x0, y0), (x1, y1)) in segs {
            // Segments are Y-down; flip into the Y-up grid.
            sk.line(x0, GRID - y0, x1, GRID - y1, c, ICON_STROKE);
        }
    })
}

/// The five wind streaks as `(start, end)` in Y-down 24-grid coordinates
/// (snow-sim 3.4): centre `spot * 30 - 3`, direction
/// `(tilt * sign, 1) / sqrt(tilt^2 + 1)`, centred length `len`.
pub fn wind_segments(wind: fp_core::wind::Wind) -> [((f32, f32), (f32, f32)); 5] {
    use fp_core::wind::Wind;
    const SPOTS: [(f32, f32); 5] = [(0.3, 0.24), (0.7, 0.3), (0.5, 0.52), (0.26, 0.76), (0.72, 0.74)];
    let (tilt, sign, len) = match wind {
        Wind::None => (0.0, 1.0, 7.2),
        Wind::GentleLeft => (0.45, -1.0, 7.2),
        Wind::StrongLeft => (0.95, -1.0, 9.0),
        Wind::GentleRight => (0.45, 1.0, 7.2),
        Wind::StrongRight => (0.95, 1.0, 9.0),
        Wind::Changing => (0.7, 1.0, 7.2),
    };
    let changing = wind == Wind::Changing;
    let norm = (tilt * tilt + 1.0f32).sqrt();
    let mut out = [((0.0, 0.0), (0.0, 0.0)); 5];
    for (i, (sx, sy)) in SPOTS.into_iter().enumerate() {
        let sg = if changing && i % 2 == 1 { -sign } else { sign };
        let (dx, dy) = (tilt * sg / norm, 1.0 / norm);
        let (cx, cy) = (sx * 30.0 - 3.0, sy * 30.0 - 3.0);
        let h = len / 2.0;
        out[i] = ((cx - dx * h, cy - dy * h), (cx + dx * h, cy + dy * h));
    }
    out
}

fn render_icon(size: u32, color: u32, draw: impl FnOnce(&mut Sketch<'_>, Color)) -> Vec<u8> {
    let Some(mut pm) = Pixmap::new(size, size) else {
        return Vec::new();
    };
    let mut sk = Sketch::new(&mut pm, size as f32 / GRID);
    draw(&mut sk, hex(color));
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

fn draw_icon(sk: &mut Sketch<'_>, name: IconName, c: Color) {
    match name {
        IconName::Snow => snow(sk, c),
        IconName::Rain => {
            cloud(sk, c);
            for x in [7.0, 12.0, 17.0] {
                sk.line(x, 6.5, x - 1.5, 2.5, c, ICON_STROKE);
            }
        }
        IconName::Storm => {
            cloud(sk, c);
            sk.polyline(&[(13.5, 8.0), (9.5, 4.8), (13.0, 4.2), (10.0, 0.8)], c, ICON_STROKE);
        }
        IconName::Sakura => {
            for k in 0..5 {
                let a = FRAC_PI_2 + k as f32 * 2.0 * PI / 5.0;
                let (px, py) = (12.0 + 5.4 * a.cos(), 12.0 + 5.4 * a.sin());
                sk.ring(px - 3.3, py - 3.3, 6.6, 6.6, c, 1.6);
            }
            sk.circle(12.0, 12.0, 1.4, c);
        }
        IconName::Leaves => {
            let mut t = Trace::new();
            t.move_to(5.0, 5.0).quad(19.0, 19.0, 5.0, 19.0).quad(5.0, 5.0, 19.0, 5.0).close();
            fill_trace(sk, t, c);
            sk.line(5.0, 5.0, 2.5, 2.5, c, ICON_STROKE);
        }
        IconName::Fireflies => {
            sk.halo(12.0, 12.0, 9.0, solid(c, 0.45));
            sk.circle(12.0, 12.0, 3.0, c);
            sk.circle(4.5, 18.5, 1.4, c);
            sk.circle(19.5, 5.5, 1.4, c);
        }
        IconName::Starfall => {
            let pts: Vec<(f32, f32)> = (0..10)
                .map(|k| {
                    let r = if k % 2 == 0 { 6.5 } else { 2.9 };
                    let a = FRAC_PI_2 + k as f32 * PI / 5.0;
                    (14.0 + r * a.cos(), 14.5 + r * a.sin())
                })
                .collect();
            sk.polygon(&pts, c);
            sk.line(8.0, 9.0, 2.5, 3.5, c, ICON_STROKE);
            sk.line(11.0, 6.5, 7.5, 2.5, c, ICON_STROKE);
        }
        IconName::Plough => {
            sk.polygon(&[(2.5, 6.0), (8.0, 6.0), (8.0, 16.0), (2.5, 18.5)], c);
            let body = [(10.0, 7.5), (10.0, 14.5), (15.0, 14.5), (17.0, 11.0), (21.5, 11.0), (21.5, 7.5), (10.0, 7.5)];
            sk.polyline(&body, c, ICON_STROKE);
            sk.circle(12.5, 5.5, 2.0, c);
            sk.circle(19.0, 5.5, 2.0, c);
        }
        IconName::Snowman => {
            sk.ring(7.0, 1.7, 10.0, 10.0, c, 1.7);
            sk.ring(8.4, 11.2, 7.2, 7.2, c, 1.7);
            sk.ring(9.6, 18.4, 4.8, 4.8, c, 1.5);
            sk.circle(12.0, 6.5, 0.9, c);
            sk.circle(10.9, 15.0, 0.7, c);
            sk.circle(13.1, 15.0, 0.7, c);
        }
        IconName::Yeti => {
            sk.ring(4.0, 3.5, 16.0, 16.0, c, ICON_STROKE);
            sk.circle(9.0, 12.5, 1.3, c);
            sk.circle(15.0, 12.5, 1.3, c);
            sk.polyline(&[(8.5, 8.5), (10.3, 6.5), (12.0, 8.5), (13.7, 6.5), (15.5, 8.5)], c, 1.5);
            sk.polyline(&[(8.0, 20.0), (9.5, 22.5), (11.5, 20.5), (13.5, 22.5), (15.5, 20.0)], c, 1.5);
        }
        IconName::Santa => {
            let mut t = Trace::new();
            t.move_to(5.5, 8.0).quad(17.0, 18.0, 6.5, 17.5).line(18.5, 8.0).close();
            fill_trace(sk, t, c);
            sk.rect(3.5, 4.5, 17.0, 3.6, 1.8, c);
            sk.circle(18.0, 18.5, 2.4, c);
        }
        IconName::Train => {
            let body = [(3.0, 8.0), (3.0, 15.0), (9.0, 15.0), (9.0, 19.5), (18.0, 19.5), (18.0, 8.0), (3.0, 8.0)];
            sk.polyline(&body, c, ICON_STROKE);
            sk.rect(10.5, 9.5, 5.0, 4.0, 1.0, c);
            sk.circle(7.0, 5.8, 2.0, c);
            sk.circle(14.0, 5.8, 2.0, c);
            sk.line(2.0, 2.5, 22.0, 2.5, c, ICON_STROKE);
            sk.line(18.0, 11.5, 21.5, 11.5, c, ICON_STROKE);
        }
        IconName::Wind => {
            let mut t = Trace::new();
            t.move_to(3.0, 16.0).line(14.0, 16.0).arc(14.0, 19.0, 3.0, -FRAC_PI_2, FRAC_PI_2);
            t.move_to(3.0, 10.0).line(12.0, 10.0).arc(12.0, 12.5, 2.5, -FRAC_PI_2, FRAC_PI_2);
            t.move_to(6.0, 5.5).line(17.0, 5.5);
            stroke_trace(sk, t, c, ICON_STROKE);
        }
        IconName::Outside => {
            sk.ring(7.5, 7.5, 9.0, 9.0, c, ICON_STROKE);
            for k in 0..8 {
                let (sa, ca) = (k as f32 * PI / 4.0).sin_cos();
                sk.line(12.0 + 7.0 * ca, 12.0 + 7.0 * sa, 12.0 + 10.0 * ca, 12.0 + 10.0 * sa, c, ICON_STROKE);
            }
        }
        IconName::Random => {
            sk.rect_outline((4.0, 4.0, 16.0, 16.0), 3.5, c, ICON_STROKE);
            for (x, y) in [(8.5, 8.5), (15.5, 15.5), (12.0, 12.0), (8.5, 15.5), (15.5, 8.5)] {
                sk.circle(x, y, 1.3, c);
            }
        }
        IconName::MinusCircle => {
            sk.ring(3.0, 3.0, 18.0, 18.0, c, ICON_STROKE);
            sk.line(7.5, 12.0, 16.5, 12.0, c, ICON_STROKE);
        }
        IconName::Home => {
            sk.polyline(&[(2.5, 11.0), (12.0, 20.0), (21.5, 11.0)], c, ICON_STROKE);
            sk.polyline(&[(5.5, 10.0), (5.5, 3.5), (18.5, 3.5), (18.5, 10.0)], c, ICON_STROKE);
            sk.polyline(&[(10.0, 3.5), (10.0, 9.0), (14.0, 9.0), (14.0, 3.5)], c, ICON_STROKE);
        }
        IconName::Logout => {
            sk.polyline(&[(10.0, 4.0), (4.0, 4.0), (4.0, 20.0), (10.0, 20.0)], c, ICON_STROKE);
            sk.line(9.0, 12.0, 20.0, 12.0, c, ICON_STROKE);
            sk.polyline(&[(16.0, 16.0), (20.0, 12.0), (16.0, 8.0)], c, ICON_STROKE);
        }
        IconName::Bauble => {
            sk.ring(5.0, 3.0, 14.0, 14.0, c, ICON_STROKE);
            sk.rect(10.0, 17.0, 4.0, 2.5, 0.8, c);
            sk.ring(10.5, 19.7, 3.0, 3.0, c, 1.4);
            let mut t = Trace::new();
            t.arc(12.0, 10.0, 4.2, PI * 0.8, PI * 1.05);
            stroke_trace(sk, t, c, 1.4);
        }
        IconName::Tree => {
            sk.polygon(&[(12.0, 22.0), (7.5, 15.5), (16.5, 15.5)], c);
            sk.polygon(&[(12.0, 18.0), (5.5, 10.0), (18.5, 10.0)], c);
            sk.polygon(&[(12.0, 13.5), (3.5, 5.5), (20.5, 5.5)], c);
            sk.rect(10.5, 1.5, 3.0, 4.0, 0.5, c);
        }
    }
}

fn fill_trace(sk: &mut Sketch<'_>, t: Trace, c: Color) {
    if let Some(p) = t.finish() {
        sk.fill(&p, c);
    }
}

fn stroke_trace(sk: &mut Sketch<'_>, t: Trace, c: Color, width: f32) {
    if let Some(p) = t.finish() {
        sk.stroke(&p, c, width);
    }
}

/// Six-spoke flake with a chevron on each spoke.
fn snow(sk: &mut Sketch<'_>, c: Color) {
    for k in 0..6 {
        let a = FRAC_PI_2 + k as f32 * PI / 3.0;
        let (sa, ca) = a.sin_cos();
        sk.line(12.0, 12.0, 12.0 + 9.5 * ca, 12.0 + 9.5 * sa, c, ICON_STROKE);
        let (bx, by) = (12.0 + 6.0 * ca, 12.0 + 6.0 * sa);
        let (l, r) = (a + 2.4, a - 2.4);
        let tips = [(bx + 2.6 * l.cos(), by + 2.6 * l.sin()), (bx, by), (bx + 2.6 * r.cos(), by + 2.6 * r.sin())];
        sk.polyline(&tips, c, 1.4);
    }
}

/// Filled cloud, base at y 8.7, top near y 20.
fn cloud(sk: &mut Sketch<'_>, c: Color) {
    sk.circle(8.0, 12.5, 3.8, c);
    sk.circle(12.5, 15.0, 5.0, c);
    sk.circle(17.0, 12.5, 3.8, c);
    sk.rect(8.0, 8.7, 9.0, 7.6, 0.0, c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::wind::Wind;

    const ALL: [IconName; 20] = [
        IconName::Snow,
        IconName::Rain,
        IconName::Storm,
        IconName::Sakura,
        IconName::Leaves,
        IconName::Fireflies,
        IconName::Starfall,
        IconName::Plough,
        IconName::Snowman,
        IconName::Yeti,
        IconName::Santa,
        IconName::Train,
        IconName::Wind,
        IconName::Outside,
        IconName::Random,
        IconName::MinusCircle,
        IconName::Home,
        IconName::Logout,
        IconName::Bauble,
        IconName::Tree,
    ];

    fn bytes(c: Color) -> [u8; 4] {
        let u = c.to_color_u8();
        [u.red(), u.green(), u.blue(), u.alpha()]
    }

    fn px(pm: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let p = pm.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    }

    fn square(x: f32, y: f32, w: f32, h: f32) -> Path {
        let mut t = Trace::new();
        t.lines(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)]).close();
        t.finish().unwrap()
    }

    fn arc_path(start: f32, end: f32) -> Path {
        let mut t = Trace::new();
        t.arc(0.0, 0.0, 10.0, start, end);
        t.finish().unwrap()
    }

    #[test]
    fn colours() {
        assert_eq!(bytes(rgb(0.7, 0.3, 0.5)), [0xB2, 0x4C, 0x80, 255]);
        assert_eq!(bytes(rgba(2.0, -1.0, 0.0, 0.5)), [255, 0, 0, 0x80]);
        assert_eq!(bytes(hex(0x1F70E0)), [0x1F, 0x70, 0xE0, 255]);
        assert_eq!(bytes(solid(hex(menu::SNOW), 0.5)), [0x59, 0xB2, 0xFF, 128]);
        assert_eq!(bytes(solid(hex(0), 7.0)), [0, 0, 0, 255]);
        assert_eq!(bytes(rgb(0.35, 0.7, 1.0)), [0x59, 0xB2, 0xFF, 255]);
        assert_eq!(brand::SNOW_DAY.0, 0xDBEDFF);
        assert_eq!(brand::RADIUS, 12.0);
        assert_eq!((menu::RAIN, menu::SAKURA, menu::LEAVES), (0x4080FA, 0xFF73A8, 0xF28C26));
        assert_eq!(
            (menu::FIREFLIES, menu::STARFALL, menu::GRAY, menu::HEAVY_BLUE),
            (0xF2C71A, 0x9E73FF, 0x8E8E93, 0x2973F2)
        );
    }

    #[test]
    fn trace_basics() {
        assert!(Trace::default().finish().is_none());
        let mut t = Trace::new();
        // Line with no sub-path starts at (0,0).
        t.line(10.0, 0.0).line(10.0, 10.0).close();
        let b = t.finish().unwrap().bounds();
        assert_eq!((b.left(), b.top(), b.right(), b.bottom()), (0.0, 0.0, 10.0, 10.0));

        // After close, the next segment restarts at the sub-path start.
        let mut t = Trace::new();
        t.move_to(5.0, 5.0).line(8.0, 5.0).close().line(5.0, 9.0).close().close();
        let b = t.finish().unwrap().bounds();
        assert_eq!((b.left(), b.top(), b.right(), b.bottom()), (5.0, 5.0, 8.0, 9.0));

        let mut t = Trace::new();
        t.lines(&[]).lines(&[(1.0, 1.0), (4.0, 1.0), (4.0, 3.0)]).quad(1.0, 3.0, 4.0, 5.0);
        let b = t.finish().unwrap().bounds();
        assert_eq!(b.bottom(), 5.0);

        let mut t = Trace::new();
        t.move_to(0.0, 0.0).curve(10.0, 0.0, 0.0, 10.0, 10.0, 10.0);
        let b = t.finish().unwrap().bounds();
        assert!(b.bottom() > 0.0 && b.bottom() <= 10.0);
    }

    #[test]
    fn arc_segments_and_quirk() {
        // Quarter circle: one segment; bounds hug the circle.
        let b = arc_path(0.0, FRAC_PI_2).bounds();
        assert!((b.right() - 10.0).abs() < 1e-3 && (b.bottom() - 10.0).abs() < 1e-3 && b.left().abs() < 1e-3);

        // f32 2*PI stays at four segments, so the circle is tight.
        let b = arc_path(0.0, 2.0 * PI).bounds();
        assert!((b.left() + 10.0).abs() < 0.02 && (b.right() - 10.0).abs() < 0.02);
        assert!((b.top() + 10.0).abs() < 0.02 && (b.bottom() - 10.0).abs() < 0.02);

        // Open sub-path: arc joins with a line from the current point.
        let mut t = Trace::new();
        t.move_to(-5.0, -5.0).arc(0.0, 0.0, 10.0, 0.0, 1.0);
        assert_eq!(t.finish().unwrap().bounds().left(), -5.0);

        // Negative sweep bulges the other way: the reflected arc hugs the
        // origin side of the chord instead of the outside.
        let diag = |p: &Path| p.points().iter().map(|q| q.x + q.y).fold(f32::MIN, f32::max);
        let outward = diag(&arc_path(0.0, FRAC_PI_2));
        let inward = diag(&arc_path(FRAC_PI_2, 0.0));
        assert!(outward > 14.0 && inward < 11.0);

        // Zero sweep is a single degenerate segment.
        let mut t = Trace::new();
        t.arc(0.0, 0.0, 10.0, 1.0, 1.0).line(0.0, 0.0);
        assert!(t.finish().is_some());
    }

    fn canvas(w: u32, scale: f32, f: impl FnOnce(&mut Sketch<'_>)) -> Pixmap {
        let mut pm = Pixmap::new(w, w).unwrap();
        f(&mut Sketch::new(&mut pm, scale));
        pm
    }

    fn red() -> Color {
        hex(0xFF0000)
    }
    fn green() -> Color {
        hex(0x00FF00)
    }
    fn blue() -> Color {
        hex(0x0000FF)
    }
    const WHITE: Color = Color::WHITE;

    #[test]
    fn sketch_flip_and_scale() {
        // 2x2 pt box at the bottom-left, Y up.
        let pm = canvas(20, 2.0, |sk| sk.fill(&square(0.0, 0.0, 2.0, 2.0), red()));
        assert_eq!(px(&pm, 1, 18), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 3, 16), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 1, 5)[3], 0);
        assert_eq!(px(&pm, 5, 18)[3], 0);
    }

    #[test]
    fn sketch_transforms_and_stack() {
        let pm = canvas(20, 1.0, |sk| {
            sk.restore(); // no-op without save
            sk.save();
            sk.translate(10.0, 10.0);
            sk.scale_by(2.0, 2.0);
            sk.rect(0.0, 0.0, 2.0, 2.0, 0.0, green());
            sk.restore();
            sk.circle(3.0, 3.0, 2.5, blue());
            sk.circle(2.0, 2.0, -1.0, blue());
        });
        assert_eq!(px(&pm, 12, 8), [0, 255, 0, 255]);
        assert_eq!(px(&pm, 3, 16), [0, 0, 255, 255]);
        assert_eq!(px(&pm, 2, 5)[3], 0);

        // Rotation is counter-clockwise in Y-up: a bar along +X turns to +Y.
        let pm = canvas(20, 1.0, |sk| {
            sk.at(10.0, 5.0, FRAC_PI_2, 1.0, |s| s.rect(0.0, -1.0, 8.0, 2.0, 0.0, red()));
        });
        assert_eq!(px(&pm, 10, 10), [255, 0, 0, 255]); // y up 10 -> pixel row 10
        assert_eq!(px(&pm, 14, 15)[3], 0);

        // at() with turn 0 and scale 3 leaves rotation alone.
        let pm = canvas(20, 1.0, |sk| sk.at(0.0, 0.0, 0.0, 3.0, |s| s.rect(0.0, 0.0, 2.0, 2.0, 0.0, red())));
        assert_eq!(px(&pm, 5, 15), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 7, 15)[3], 0);
    }

    #[test]
    fn sketch_shapes() {
        let pm = canvas(40, 1.0, |sk| {
            sk.rect(2.0, 2.0, 10.0, 10.0, 4.0, red());
            sk.rect(20.0, 2.0, 4.0, 4.0, 99.0, red()); // radius clamps
            sk.ellipse(2.0, 20.0, 10.0, 6.0, green());
            sk.ellipse(0.0, 0.0, -1.0, 0.0, green()); // invalid: ignored
            sk.ring(20.0, 20.0, 10.0, 10.0, blue(), 2.0);
            sk.ring(0.0, 0.0, -1.0, 0.0, blue(), 2.0);
            sk.rect(f32::NAN, 0.0, 1.0, 1.0, 0.0, blue());
            sk.rect_outline((f32::NAN, 0.0, 1.0, 1.0), 0.0, blue(), 1.0);
            sk.halo(5.0, 5.0, -1.0, blue());
            fill_trace(sk, Trace::new(), blue());
            stroke_trace(sk, Trace::new(), blue(), 1.0);
            sk.line(30.0, 30.0, 38.0, 30.0, WHITE, 2.0);
            sk.polyline(&[], WHITE, 2.0);
            sk.polygon(&[], WHITE);
            sk.polygon(&[(30.0, 20.0), (38.0, 20.0), (34.0, 26.0)], hex(0xFFFF00));
            sk.stroke(&square(0.0, 0.0, 1.0, 1.0), WHITE, 1.0);
        });
        assert_eq!(px(&pm, 7, 33), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 2, 37)[3], 0); // rounded corner stays clear
        assert_eq!(px(&pm, 7, 17), [0, 255, 0, 255]);
        assert_eq!(px(&pm, 20, 15)[2], 255);
        assert_eq!(px(&pm, 25, 15)[3], 0);
        assert_eq!(px(&pm, 34, 10), [255, 255, 255, 255]);
        assert_eq!(px(&pm, 34, 16), [255, 255, 0, 255]);
    }

    #[test]
    fn sketch_gradients() {
        let sq = square(0.0, 0.0, 10.0, 10.0);
        let pm = canvas(10, 1.0, |sk| sk.across(&sq, &[red(), blue()], None, (0.0, 0.0), (10.0, 0.0)));
        assert!(px(&pm, 0, 5)[0] > 200 && px(&pm, 9, 5)[2] > 200);

        let pm = canvas(10, 1.0, |sk| {
            sk.across(&sq, &[red(), green(), blue()], Some(&[0.0, 0.5, 1.0]), (0.0, 0.0), (10.0, 0.0));
        });
        assert!(px(&pm, 5, 5)[1] > 150);

        // Single colour is solid; empty lists draw nothing.
        let pm = canvas(10, 1.0, |sk| {
            sk.across(&sq, &[hex(0x808080)], None, (0.0, 0.0), (10.0, 0.0));
            sk.across(&sq, &[], None, (0.0, 0.0), (10.0, 0.0));
            sk.round(&sq, &[], None, (5.0, 5.0), (5.0, 5.0), 5.0);
        });
        assert_eq!(px(&pm, 5, 5), [0x80, 0x80, 0x80, 255]);
        assert_eq!(gradient_stops(&[], None).len(), 0);

        let pm = canvas(10, 1.0, |sk| {
            sk.round(&sq, &[WHITE, hex(0)], None, (5.0, 5.0), (5.0, 5.0), 5.0);
        });
        assert!(px(&pm, 5, 5)[0] > 200 && px(&pm, 0, 9)[0] < 60);

        let pm = canvas(20, 1.0, |sk| {
            sk.halo(10.0, 10.0, 8.0, red());
            sk.halo(10.0, 10.0, 0.0, red());
        });
        let (c, e) = (px(&pm, 10, 10), px(&pm, 10, 3));
        assert!(c[3] > 200 && e[3] < c[3]);
        assert_eq!(px(&pm, 0, 0)[3], 0);
    }

    #[test]
    fn sketch_clip() {
        let a = square(0.0, 0.0, 10.0, 10.0);
        let b = square(5.0, 5.0, 10.0, 10.0);
        let all = square(0.0, 0.0, 20.0, 20.0);
        let pm = canvas(20, 1.0, |sk| sk.inside(&a, |s| s.inside(&b, |s| s.fill(&all, red()))));
        assert_eq!(px(&pm, 7, 12), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 2, 17)[3], 0);
        assert_eq!(px(&pm, 12, 7)[3], 0);
        // Clip is gone after the scope; a bare clip stays until restore.
        let pm = canvas(20, 1.0, |sk| {
            sk.inside(&a, |_| {});
            sk.fill(&all, green());
            sk.save();
            sk.clip(&b);
            sk.stroke(&all, blue(), 2.0);
            sk.restore();
        });
        assert_eq!(px(&pm, 2, 2), [0, 255, 0, 255]);
        assert_eq!(px(&pm, 19, 0)[1], 255);
    }

    #[test]
    fn strand_geometry() {
        let s = Strand::LIGHTS;
        assert_eq!(s.art_size(), (260.0, 64.0));
        assert_eq!(s.point(0.0), (6.0, 56.0));
        assert_eq!(s.point(1.0), (254.0, 56.0));
        assert_eq!(s.point(0.5), (130.0, 38.0));
        assert_eq!(s.anchors().1, (130.0, 20.0));
        assert!((s.tangent(0.5)).abs() < 1e-12);
        assert!(s.tangent(0.0) < 0.0 && s.tangent(1.0) > 0.0);
        assert_eq!(s.bulb_count(), 10);
        let (p, turn) = s.bulb(0);
        assert!(p.0 > 6.0 && p.1 < 56.0 && turn < 0.0);
        assert_eq!(Strand { length: 110.0, dip: 0.0 }.bulb_count(), 4);
        assert_eq!(Strand { length: 50.0, dip: 0.0 }.bulb_count(), 3);
        assert_eq!(Strand { length: 112.5, dip: 0.0 }.bulb_count(), 4);
        let f = Strand { length: 5.0, dip: 999.0 }.fitted();
        assert_eq!((f.length, f.dip), (110.0, 260.0));
        let f = Strand { length: 9999.0, dip: -4.0 }.fitted();
        assert_eq!((f.length, f.dip), (4000.0, 0.0));
        assert_eq!(Strand::BULB_TONES.len(), 5);
    }

    #[test]
    fn icons_render_in_tint() {
        for name in ALL {
            let data = icon_rgba(name, 0x59B2FF, 24);
            assert_eq!(data.len(), 24 * 24 * 4, "{name:?}");
            let mut solid_px = 0;
            for p in data.chunks(4) {
                if p[3] == 255 {
                    solid_px += 1;
                    assert_eq!(&p[..3], &[0x59, 0xB2, 0xFF], "{name:?}");
                }
            }
            assert!(solid_px > 8, "{name:?} too faint");
            // Partial coverage keeps the straight tint.
            for p in data.chunks(4).filter(|p| p[3] > 100 && p[3] < 255) {
                assert!((i32::from(p[0]) - 0x59).abs() <= 3, "{name:?}");
            }
            let big = icon_rgba(name, 0xFFFFFF, 64);
            assert_eq!(big.len(), 64 * 64 * 4);
        }
        assert!(icon_rgba(IconName::Snow, 0, 0).is_empty());
    }

    #[test]
    fn wind_icons() {
        let winds =
            [Wind::None, Wind::GentleLeft, Wind::StrongLeft, Wind::GentleRight, Wind::StrongRight, Wind::Changing];
        let icons: Vec<Vec<u8>> = winds.iter().map(|&w| wind_icon_rgba(w, 0x73B8D9, 30)).collect();
        for (i, a) in icons.iter().enumerate() {
            assert_eq!(a.len(), 30 * 30 * 4);
            assert!(a.chunks(4).any(|p| p[3] > 200));
            for b in &icons[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(wind_icon_rgba(Wind::None, 0, 0).is_empty());
    }

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-4 && (a.1 - b.1).abs() < 1e-4
    }

    #[test]
    fn wind_segments_follow_spec() {
        // None: vertical streaks of length 7.2 centred on spot*30-3.
        let s = wind_segments(Wind::None);
        assert!(near(s[2].0, (12.0, 9.0)) && near(s[2].1, (12.0, 16.2)));
        assert!(near(s[0].0, (6.0, 0.6)) && near(s[0].1, (6.0, 7.8)));
        for (w, len) in [(Wind::GentleLeft, 7.2), (Wind::StrongRight, 9.0), (Wind::Changing, 7.2)] {
            for ((x0, y0), (x1, y1)) in wind_segments(w) {
                assert!(((x1 - x0).hypot(y1 - y0) - len).abs() < 1e-4, "{w:?}");
                assert!(y1 > y0, "streaks run downward in Y-down coords");
            }
        }
        // Strong right: dx/dy = 0.95 with positive sign; left mirrors it.
        let (a, b) = wind_segments(Wind::StrongRight)[2];
        assert!((((b.0 - a.0) / (b.1 - a.1)) - 0.95).abs() < 1e-5);
        let (a, b) = wind_segments(Wind::StrongLeft)[2];
        assert!((((b.0 - a.0) / (b.1 - a.1)) + 0.95).abs() < 1e-5);
        let (a, b) = wind_segments(Wind::GentleRight)[4];
        assert!((((b.0 - a.0) / (b.1 - a.1)) - 0.45).abs() < 1e-5);
        // Changing: odd streaks (1, 3) lean left, even ones right, tilt 0.7.
        for (i, (a, b)) in wind_segments(Wind::Changing).into_iter().enumerate() {
            let slope = (b.0 - a.0) / (b.1 - a.1);
            let want = if i % 2 == 1 { -0.7 } else { 0.7 };
            assert!((slope - want).abs() < 1e-5, "streak {i}");
        }
    }

    #[test]
    fn solid_rounds_half_to_even() {
        assert_eq!(solid(Color::from_rgba8(1, 2, 3, 1), 0.5).to_color_u8().alpha(), 0);
        assert_eq!(solid(Color::from_rgba8(1, 2, 3, 3), 0.5).to_color_u8().alpha(), 2);
        assert_eq!(bytes(solid(Color::from_rgba8(1, 2, 3, 200), f64::NAN)), [1, 2, 3, 0]);
        assert_eq!(solid(hex(0), -1.0).to_color_u8().alpha(), 0);
    }

    #[test]
    fn arc_bad_sweeps_terminate() {
        for end in [f32::INFINITY, f32::NAN, f32::NEG_INFINITY] {
            let mut t = Trace::new();
            t.arc(0.0, 0.0, 10.0, 0.0, end);
            // Only the start Move exists, so there is no geometry.
            assert!(t.finish().is_none(), "{end}");
        }
        let mut t = Trace::new();
        t.arc(0.0, 0.0, 10.0, 0.0, 1.0e7);
        let p = t.finish().unwrap();
        assert_eq!(p.verbs().len(), 1 + MAX_ARC_SEGMENTS as usize);
    }

    #[test]
    fn strand_fitted_and_bulbs() {
        let f = Strand { length: f64::NAN, dip: f64::NAN }.fitted();
        assert_eq!(f, Strand::LIGHTS);
        assert_eq!(Strand::bulb_tone(7), Strand::BULB_TONES[2]);
        assert_eq!(Strand::bulb_tone(0), (1.0, 0.25, 0.30));
        // Bulbs are symmetric about the rope's midpoint.
        let s = Strand::LIGHTS;
        let n = s.bulb_count();
        let ((x0, y0), t0) = s.bulb(0);
        let ((x1, y1), t1) = s.bulb(n - 1);
        assert!((x0 + x1 - s.length).abs() < 1e-9 && (y0 - y1).abs() < 1e-9 && (t0 + t1).abs() < 1e-12);
        // Rotation is 0.6 * tangent angle.
        assert!((t0 - 0.6 * s.tangent(0.5 / n as f64)).abs() < 1e-12);
        // Odd strands put the middle bulb on the lowest point, level.
        let s = Strand { length: 137.0, dip: 30.0 };
        assert_eq!(s.bulb_count(), 5);
        let ((x, y), turn) = s.bulb(2);
        assert_eq!((x, y), (68.5, 38.0));
        assert!(turn.abs() < 1e-12);
        assert_eq!(s.art_size(), (137.0, 76.0));
    }

    #[test]
    fn pose_and_concat() {
        let t = pose((1.0, 0.0), (2.0, 2.0), FRAC_PI_2, (10.0, 10.0));
        let mut pts = [Point::from_xy(1.0, 0.0), Point::from_xy(2.0, 0.0)];
        t.map_points(&mut pts);
        assert!((pts[0].x - 10.0).abs() < 1e-5 && (pts[0].y - 10.0).abs() < 1e-5);
        assert!((pts[1].x - 10.0).abs() < 1e-5 && (pts[1].y - 12.0).abs() < 1e-5);

        // concat uses the same Y-up frame as the other transforms.
        let pm = canvas(20, 1.0, |sk| {
            sk.concat(Transform::from_translate(10.0, 10.0));
            sk.rect(0.0, 0.0, 2.0, 2.0, 0.0, red());
        });
        assert_eq!(px(&pm, 11, 8), [255, 0, 0, 255]);
        assert_eq!(px(&pm, 11, 11)[3], 0);
    }

    #[test]
    fn vertical_gradient_top_is_high_y() {
        let sq = square(0.0, 0.0, 10.0, 10.0);
        let pm = canvas(10, 1.0, |sk| sk.vertical(&sq, red(), blue()));
        let (top, bottom) = (px(&pm, 5, 0), px(&pm, 5, 9));
        assert!(top[0] > 200 && top[2] < 60, "{top:?}");
        assert!(bottom[2] > 200 && bottom[0] < 60, "{bottom:?}");
    }

    #[test]
    fn degenerate_draws_do_not_panic() {
        let pm = canvas(10, 1.0, |sk| {
            sk.stroke(&square(1.0, 1.0, 5.0, 5.0), red(), -3.0);
            sk.save();
            sk.scale_by(0.0, 0.0);
            sk.circle(5.0, 5.0, 3.0, red());
            sk.restore();
            sk.circle(f32::NAN, 5.0, 3.0, red());
        });
        assert!(pm.pixels().iter().all(|p| p.alpha() == 0));
    }

    #[test]
    fn icon_names_and_orientation() {
        let names = [
            "snow",
            "rain",
            "storm",
            "sakura",
            "leaves",
            "fireflies",
            "starfall",
            "plough",
            "snowman",
            "yeti",
            "santa",
            "train",
            "wind",
            "outside",
            "random",
            "minus-circle",
            "home",
            "logout",
            "bauble",
            "tree",
        ];
        for (n, want) in names.iter().zip(ALL) {
            assert_eq!(IconName::from_name(n), Some(want));
        }
        assert_eq!(IconName::from_name("none"), None);

        // Tree: tip at the top of the bitmap, wide bottom tier near the base.
        let tree = icon_rgba(IconName::Tree, 0xFFFFFF, 24);
        let a = |x: usize, y: usize| tree[(y * 24 + x) * 4 + 3];
        assert!(a(12, 3) > 200, "tip near the top row");
        assert_eq!(a(4, 3), 0);
        assert!(a(6, 17) > 200, "bottom tier spans wide near the base");
        assert!(a(12, 21) > 200, "trunk below the tiers");
    }
}
