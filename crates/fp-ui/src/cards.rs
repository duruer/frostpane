//! Cards shown over the desktop as egui viewports (transparent, undecorated,
//! always on top): snowman dress-up, tree style, Santa quote, limit snowman,
//! train station bubble and the decoration picker.
//!
//! The host opens a card from a `fp_world::CardRequest` ([`Cards::open`]),
//! calls [`Cards::show`] every frame from `eframe::App::ui` and applies the
//! returned [`CardAction`]s to the world.

use crate::art::{self, Textures};
use crate::cardmodel::{self, Fade, GUTTER, QUOTE_W};
use crate::model::Glyph;
use crate::theme::{self, Tokens};
use crate::widgets;
use eframe::egui::{
    self, Align2, Color32, Context, Rect, Response, Sense, TextureHandle, Ui, UiBuilder, ViewportBuilder, ViewportId,
    pos2, vec2,
};
use fp_characters::Pick;
use fp_core::desktop::Layout;
use fp_core::effect::Lang;
use fp_core::geom::{Frame, Vec2};
use fp_core::rng::{Lcg64, Rng};
use fp_core::scene::Sprite;
use fp_decor::Kind;
use fp_snowman::card::{self as dress, Tab};
use fp_snowman::puppet::{self, Puppet};
use fp_snowman::{Arms, Face, Hat, Look, Outfit};
use fp_text::Key;
use fp_trees::{Topper, TreeLook, Trim};
use fp_world::CardRequest;

/// What a card asks the host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum CardAction {
    /// `World::restyle_snowman(bottom, look)`.
    RestyleSnowman { bottom: u64, look: Option<Look> },
    /// `World::restyle_tree(tree, look)`.
    RestyleTree { tree: u64, look: TreeLook },
    /// The chop button: fell this tree (`Grove::doom`).
    DoomTree(u64),
    /// The user closed the tree card: `World::close_tree_card()`.
    TreeCardClosed,
    /// `Command::AddDecoration(kind)`.
    AddDecoration(Kind),
    /// `Command::RemoveAllDecorations`.
    RemoveAllDecorations,
    /// `Command::PlantTree`.
    PlantTree,
    /// `Command::TrainFetch(pick)`.
    TrainFetch(Pick),
    /// `Command::TrainDismiss`.
    TrainDismiss,
    /// The update notice was dismissed: write `updateAskedAt = now`.
    UpdateLater,
    /// Open this https page (release page of the notice).
    OpenUrl(String),
}

/// Facts the host supplies when a card opens.
pub struct CardCtx<'a> {
    pub lang: Lang,
    pub layout: &'a Layout,
    /// The latest `WorldOutput::sprites` (to find the snowman stack).
    pub sprites: &'a [Sprite],
    /// Global point (Y up) of the mouse cursor, the fallback anchor.
    pub cursor: Vec2,
    /// `World::tree_card_origin(id, work)` for a tree style card.
    pub tree_origin: Option<Vec2>,
}

const DRESS_WINDOW: (f32, f32) =
    ((dress::CARD_W + 2.0 * dress::ROOM) as f32, (dress::CARD_H + 2.0 * dress::ROOM) as f32);
const TREE_WINDOW: (f32, f32) =
    ((fp_trees::CARD_SIZE.0 + 2.0 * dress::ROOM) as f32, (fp_trees::CARD_SIZE.1 + 2.0 * dress::ROOM) as f32);
const LIMIT_WINDOW: (f32, f32) = (cardmodel::LIMIT_SIZE.0 as f32, cardmodel::LIMIT_SIZE.1 as f32);
const PICKER_WINDOW: (f32, f32) = (400.0, 560.0);
const UPDATE_WINDOW: (f32, f32) = (468.0, 300.0);

struct Placed {
    /// Window top-left in egui logical points.
    pos: (f32, f32),
    size: (f32, f32),
    fade: Fade,
    had_focus: bool,
}

struct DressCard {
    at: Placed,
    bottom: u64,
    look: Look,
    tab: Tab,
    quip: usize,
}

struct TreeCardState {
    at: Placed,
    tree: u64,
    look: TreeLook,
    header: Header,
}

#[derive(Clone, Copy)]
enum Header {
    Quip(usize),
    Refuse(u8),
}

struct QuoteCard {
    at: Placed,
    text: String,
    by: Option<String>,
    thanks: usize,
    born: f64,
    auto: f64,
}

struct LimitCard {
    at: Placed,
    puppet: Puppet,
    born: f64,
    last: f64,
    /// The bubble has popped (and the hop started).
    popped: bool,
}

struct StationCard {
    at: Placed,
    choices: Vec<Pick>,
}

struct UpdateCard {
    at: Placed,
    version: String,
    url: String,
}

struct PointerCard {
    at: Placed,
    plan: cardmodel::PointerPlan,
    born: f64,
}

struct PickerCard {
    at: Placed,
    /// The window rect in global points (an obstacle for decoration spots).
    rect: Frame,
    count: usize,
    max: usize,
    confirm_all: bool,
}

/// All card windows. Each kind is a singleton.
pub struct Cards {
    dress: Option<DressCard>,
    tree: Option<TreeCardState>,
    quote: Option<QuoteCard>,
    limit: Option<LimitCard>,
    station: Option<StationCard>,
    picker: Option<PickerCard>,
    update: Option<UpdateCard>,
    pointer: Option<PointerCard>,
    tex: Textures,
    rng: Lcg64,
    actions: Vec<CardAction>,
}

/// Screenshot harness hooks: states a real run reaches only by clicking.
#[cfg(test)]
impl Cards {
    /// Show dress-up tab `tab` (index into `Tab::ALL`) and/or the picker's
    /// "take all down" confirmation.
    #[allow(dead_code)] // kept for scene setup in tests
    pub(crate) fn test_view(&mut self, tab: Option<usize>, confirm_all: bool) {
        if let (Some(d), Some(t)) = (&mut self.dress, tab.and_then(|i| Tab::ALL.get(i))) {
            d.tab = *t;
        }
        if let Some(p) = &mut self.picker {
            p.confirm_all = confirm_all;
        }
    }
}

/// Estimated height of the quote card for `text` and an optional author:
/// wrapped 21 pt text at 296 pt wide (CJK counts a full em, others 0.52).
pub fn quote_height(text: &str, by: Option<&str>) -> f32 {
    let em: f32 = text.chars().map(|c| if (c as u32) >= 0x2E80 { 1.0 } else { 0.52 }).sum();
    let lines = ((em * 21.0) / 296.0).ceil().max(1.0);
    let mut h = 22.0 + lines * 26.0;
    if by.is_some() {
        h += 8.0 + 17.0;
    }
    h + 68.0 - 22.0 + 22.0
}

/// Size of the station bubble for `n` choices.
pub fn station_size(n: usize) -> (f32, f32) {
    (260.0 + 2.0 * GUTTER as f32, 96.0 + 38.0 * (n as f32 + 1.0) + 2.0 * GUTTER as f32)
}

/// Rows of the decoration grid (4 columns).
pub const PICKER_COLUMNS: usize = 4;

impl Cards {
    pub fn new(seed: u64) -> Self {
        Self {
            dress: None,
            tree: None,
            quote: None,
            limit: None,
            station: None,
            picker: None,
            update: None,
            pointer: None,
            tex: Textures::new(),
            rng: Lcg64::new(seed),
            actions: Vec::new(),
        }
    }

    /// Any card window open (the host keeps repainting while true).
    pub fn any_open(&self) -> bool {
        self.dress.is_some()
            || self.tree.is_some()
            || self.quote.is_some()
            || self.limit.is_some()
            || self.station.is_some()
            || self.picker.is_some()
            || self.update.is_some()
            || self.pointer.is_some()
    }

    /// Bottom ball id of the open dress-up card.
    pub fn dress_open(&self) -> Option<u64> {
        self.dress.as_ref().map(|c| c.bottom)
    }

    /// Close the dress-up card (its snowman became invalid).
    pub fn close_dress(&mut self) {
        self.dress = None;
    }

    pub fn tree_open(&self) -> Option<u64> {
        self.tree.as_ref().map(|t| t.tree)
    }

    pub fn picker_open(&self) -> bool {
        self.picker.is_some()
    }

    /// The open decoration picker's window in global points.
    pub fn picker_rect(&self) -> Option<Frame> {
        self.picker.as_ref().map(|p| p.rect)
    }

    /// The decoration picker's current (count, max), while it is open.
    pub fn picker_counts(&self) -> Option<(usize, usize)> {
        self.picker.as_ref().map(|p| (p.count, p.max))
    }

    pub fn limit_open(&self) -> bool {
        self.limit.is_some()
    }

    /// Open the card for a world request.
    pub fn open(&mut self, req: CardRequest, cx: &CardCtx<'_>, now: f64) {
        match req {
            CardRequest::DressUp { bottom, look } => self.open_dress(bottom, look, cx, now),
            CardRequest::TreeStyle { tree, look, quip } => self.open_tree(tree, look, quip, cx, now),
            CardRequest::Quote { at, quote } => self.open_quote(at, quote, cx, now),
            CardRequest::LimitReached { screen } => self.open_limit(screen, cx, now),
            CardRequest::TrainStation { at, choices } => {
                self.open_station(at, choices.into_iter().map(|c| c.0).collect(), cx, now)
            }
        }
    }

    fn placed(&self, cx: &CardCtx<'_>, top_left: Vec2, size: (f32, f32), now: f64) -> Placed {
        Placed { pos: cardmodel::window_pos(cx.layout, top_left), size, fade: Fade::card(now), had_focus: false }
    }

    fn open_dress(&mut self, bottom: u64, look: Option<Look>, cx: &CardCtx<'_>, now: f64) {
        let b = cardmodel::stack_box(cx.sprites, bottom).unwrap_or([
            cx.cursor.x - 30.0,
            cx.cursor.y - 30.0,
            cx.cursor.x + 30.0,
            cx.cursor.y + 30.0,
        ]);
        let mid = Vec2::new((b[0] + b[2]) / 2.0, b[1]);
        let work = cardmodel::work_at(cx.layout, mid).unwrap_or(Frame::new(0.0, 0.0, 1920.0, 1080.0));
        let (x, y) = dress::placement(b, [work.min_x(), work.min_y(), work.max_x(), work.max_y()]);
        // Window origin is ROOM down-left of the card; top-left Y-up:
        let top_left = Vec2::new(x - dress::ROOM, y + dress::CARD_H + dress::ROOM);
        let quip = self.rng.next_u64() as usize;
        let at = self.placed(cx, top_left, DRESS_WINDOW, now);
        self.dress = Some(DressCard { at, bottom, look: look.unwrap_or_else(Look::classic), tab: Tab::Hat, quip });
    }

    fn open_tree(&mut self, tree: u64, look: TreeLook, quip: Option<u8>, cx: &CardCtx<'_>, now: f64) {
        let origin = cx.tree_origin.unwrap_or(Vec2::new(cx.cursor.x, cx.cursor.y - 200.0));
        // `origin` is the window's bottom-left (Y up).
        let top_left = Vec2::new(origin.x, origin.y + f64::from(TREE_WINDOW.1));
        let header = match quip {
            Some(q) => Header::Refuse(q),
            None => Header::Quip(self.rng.next_u64() as usize),
        };
        let at = self.placed(cx, top_left, TREE_WINDOW, now);
        self.tree = Some(TreeCardState { at, tree, look, header });
    }

    fn open_quote(&mut self, point: Vec2, index: usize, cx: &CardCtx<'_>, now: f64) {
        let list = fp_text::quotes(cx.lang);
        let Some(q) = list.get(index).or_else(|| list.first()) else { return };
        let work = cardmodel::work_at(cx.layout, point).unwrap_or(Frame::new(0.0, 0.0, 1920.0, 1080.0));
        let left = cardmodel::quote_left(point.x, work.min_x(), work.max_x());
        let top = cardmodel::quote_top(point.y);
        let h = quote_height(q.text, q.by);
        let size = (QUOTE_W as f32 + 2.0 * GUTTER as f32, h + 2.0 * GUTTER as f32);
        let top_left = Vec2::new(left - GUTTER, top + GUTTER);
        let mut at = self.placed(cx, top_left, size, now);
        at.fade = Fade::quote(now);
        let thanks = self.rng.next_u64() as usize;
        self.quote = Some(QuoteCard {
            at,
            text: q.text.to_string(),
            by: q.by.map(str::to_string),
            thanks,
            born: now,
            auto: cardmodel::quote_auto_fade(q.text.chars().count()),
        });
    }

    fn open_limit(&mut self, screen: Frame, cx: &CardCtx<'_>, now: f64) {
        if self.limit.is_some() {
            return;
        }
        let top_left = cardmodel::limit_top_left(screen, cardmodel::LIMIT_SIZE);
        let at = self.placed(cx, top_left, LIMIT_WINDOW, now);
        // Walks in from just outside the right edge (snowman F8/F9).
        let puppet = Puppet::at(cardmodel::LIMIT_SIZE.0 - cardmodel::LIMIT_EXIT);
        self.limit = Some(LimitCard { at, puppet, born: now, last: now, popped: false });
    }

    fn open_station(&mut self, at_pt: Vec2, choices: Vec<Pick>, cx: &CardCtx<'_>, now: f64) {
        let size = station_size(choices.len());
        let work = cardmodel::work_at(cx.layout, at_pt).unwrap_or(Frame::new(0.0, 0.0, 1920.0, 1080.0));
        let tl = cardmodel::station_top_left(at_pt, (f64::from(size.0), f64::from(size.1)), work);
        let at = self.placed(cx, tl, size, now);
        self.station = Some(StationCard { at, choices });
    }

    /// Point at the tray icon (core-app 2.7). `icon_px` = icon rect in
    /// physical pixels `[left, top, w, h]` (Y down). Returns false when the
    /// icon is not on the taskbar itself (overflow flyout, or no monitor),
    /// so the caller shows the overflow hint instead. A new call replaces a
    /// pointer that is still showing.
    pub fn open_pointer(&mut self, icon_px: [f64; 4], cx: &CardCtx<'_>, now: f64) -> bool {
        let [x, y, w, h] = icon_px;
        let a = cx.layout.to_points(x, y);
        let b = cx.layout.to_points(x + w, y + h);
        let icon = Frame::new(a.x.min(b.x), a.y.min(b.y), (b.x - a.x).abs(), (a.y - b.y).abs());
        let Some(work) = cardmodel::work_at(cx.layout, icon.center()) else { return false };
        let screens = cx.layout.screens();
        let min_x = screens.iter().map(|s| s.points.min_x()).fold(f64::INFINITY, f64::min);
        let max_x = screens.iter().map(|s| s.points.max_x()).fold(f64::NEG_INFINITY, f64::max);
        let all = Frame::new(min_x, work.min_y(), max_x - min_x, work.h);
        let plan = cardmodel::pointer_plan(icon, work, all);
        let size = (cardmodel::POINTER_SIZE.0 as f32, cardmodel::POINTER_SIZE.1 as f32);
        let at = self.placed(cx, plan.top_left, size, now);
        self.pointer = Some(PointerCard { at, plan, born: now });
        true
    }

    /// Toggle the decoration picker (tray "Decorate").
    pub fn toggle_picker(&mut self, cx: &CardCtx<'_>, count: usize, max: usize, now: f64) {
        if self.picker.is_some() {
            self.picker = None;
            return;
        }
        let fallback = Frame::new(0.0, 0.0, 1920.0, 1080.0);
        let (screen, work) = cx
            .layout
            .at_point(cx.cursor)
            .or_else(|| cx.layout.primary())
            .map_or((fallback, fallback), |s| (s.points, s.work_points));
        let tl = cardmodel::picker_top_left(
            cx.cursor,
            screen,
            work,
            (f64::from(PICKER_WINDOW.0), f64::from(PICKER_WINDOW.1)),
        );
        let at = self.placed(cx, tl, PICKER_WINDOW, now);
        let (w, h) = (f64::from(PICKER_WINDOW.0), f64::from(PICKER_WINDOW.1));
        let rect = Frame::new(tl.x, tl.y - h, w, h);
        self.picker = Some(PickerCard { at, rect, count, max, confirm_all: false });
    }

    /// Open the update notice (core-app 2.11, notice only, nothing is
    /// downloaded). A second call while one is open keeps the first.
    pub fn open_update(&mut self, version: &str, url: &str, cx: &CardCtx<'_>, now: f64) {
        if self.update.is_some() {
            return;
        }
        let work = cardmodel::work_at(cx.layout, cx.cursor).unwrap_or(Frame::new(0.0, 0.0, 1920.0, 1080.0));
        let size = UPDATE_WINDOW;
        let tl = Vec2::new(work.mid_x() - f64::from(size.0) / 2.0, work.center().y + f64::from(size.1) / 2.0);
        let at = self.placed(cx, tl, size, now);
        self.update = Some(UpdateCard { at, version: version.to_string(), url: url.to_string() });
    }

    /// Keep the picker's count line current.
    pub fn set_decor_counts(&mut self, count: usize, max: usize) {
        if let Some(p) = &mut self.picker {
            p.count = count;
            p.max = max;
        }
    }

    /// True while a dress or tree card is open and not already fading: the
    /// host then watches for a press outside the app (desktop-native 3.11,
    /// snowman 104/304). Those cards are non-activating, so focus is no cue.
    pub fn outside_watch(&self) -> bool {
        self.dress.as_ref().is_some_and(|c| !c.at.fade.is_fading())
            || self.tree.as_ref().is_some_and(|c| !c.at.fade.is_fading())
    }

    /// A press landed outside the app: fade the dress card and close the tree
    /// card (its fade ends in `TreeCardClosed`).
    pub fn close_outside(&mut self, now: f64) {
        if let Some(c) = &mut self.dress {
            c.at.fade.fade(now);
        }
        if let Some(c) = &mut self.tree {
            c.at.fade.fade(now);
        }
    }

    /// The world no longer has this tree card open: close the window.
    pub fn close_tree(&mut self) {
        self.tree = None;
    }

    /// The train left: close the station bubble.
    pub fn close_station(&mut self) {
        self.station = None;
    }

    pub fn station_open(&self) -> bool {
        self.station.is_some()
    }

    /// Snow went off: close the dress, quote and station cards.
    pub fn close_snow_cards(&mut self) {
        self.dress = None;
        self.quote = None;
        self.station = None;
    }

    /// Close everything (window hidden, app quitting).
    pub fn close_all(&mut self) {
        self.dress = None;
        self.tree = None;
        self.quote = None;
        self.limit = None;
        self.station = None;
        self.picker = None;
        self.update = None;
        self.pointer = None;
    }

    // ---- drawing ----

    /// Show every open card; returns what the user asked for.
    pub fn show(&mut self, ctx: &Context, lang: Lang) -> Vec<CardAction> {
        let now = ctx.input(|i| i.time);
        self.show_dress(ctx, lang, now);
        self.show_tree(ctx, lang, now);
        self.show_quote(ctx, lang, now);
        self.show_limit(ctx, lang, now);
        self.show_station(ctx, lang, now);
        self.show_picker(ctx, lang, now);
        self.show_update(ctx, lang, now);
        self.show_pointer(ctx, lang, now);
        if self.any_open() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        std::mem::take(&mut self.actions)
    }

    fn builder(p: &Placed) -> ViewportBuilder {
        ViewportBuilder::default()
            .with_title("Frostpane")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_inner_size([p.size.0, p.size.1])
            .with_position([p.pos.0, p.pos.1])
    }

    fn show_dress(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.dress.take() else { return };
        let tk = Tokens::new(true);
        let mut close = false;
        let mut out = Vec::new();
        let op = c.at.fade.opacity(now);
        let builder = Self::builder(&c.at).with_active(false);
        let tex = &mut self.tex;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_dress"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op);
                let card = glass(ui, &tk);
                let mut inner = ui.new_child(UiBuilder::new().max_rect(card.shrink(16.0)));
                inner.horizontal(|ui| {
                    widgets::label(ui, fp_text::dress_quip(lang, c.quip), 13.0, Color32::WHITE);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if round_button(ui, Glyph::Close, true).clicked() {
                            close = true;
                        }
                    });
                });
                inner.add_space(8.0);
                inner.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for t in Tab::ALL {
                        let key = match t {
                            Tab::Hat => Key::TabHat,
                            Tab::Face => Key::TabFace,
                            Tab::Body => Key::TabBody,
                            Tab::Clothes => Key::TabClothes,
                        };
                        if tab_pill(ui, fp_text::t(lang, key), t == c.tab, 72.0).clicked() {
                            c.tab = t;
                        }
                    }
                });
                inner.add_space(8.0);
                let selected = dress::selected(&c.look, c.tab);
                let scale = ui.ctx().pixels_per_point();
                for row in 0..2 {
                    inner.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for col in 0..4 {
                            let i = row * 4 + col;
                            let icon = tex
                                .pixmap(ui.ctx(), &format!("dress:{:?}:{i}", c.tab), |s| dress::chip_icon(c.tab, i, s));
                            let name = chip_name(lang, c.tab, i);
                            if chip(ui, vec2(76.0, 66.0), icon.as_ref(), 44.0, &name, selected == Some(i), true)
                                .clicked()
                            {
                                c.look = dress::apply(c.look, c.tab, i);
                                out.push(CardAction::RestyleSnowman { bottom: c.bottom, look: c.look.stored() });
                            }
                        }
                    });
                    inner.add_space(4.0);
                }
                let _ = scale;
                inner.add_space(2.0);
                if widgets::quiet_button(
                    &mut inner,
                    &tk,
                    fp_text::t(lang, Key::DressReset),
                    30.0,
                    dress::can_reset(&c.look),
                )
                .clicked()
                {
                    c.look = dress::reset();
                    out.push(CardAction::RestyleSnowman { bottom: c.bottom, look: None });
                }
                escape_close(ui, &mut close);
            });
        });
        self.actions.extend(out);
        if close {
            c.at.fade.fade(now);
        }
        if !c.at.fade.finished(now) {
            self.dress = Some(c);
        }
    }

    fn show_tree(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.tree.take() else { return };
        let tk = Tokens::new(true);
        let mut close = false;
        let mut out = Vec::new();
        let op = c.at.fade.opacity(now);
        let builder = Self::builder(&c.at).with_active(false);
        let tex = &mut self.tex;
        let rng = &mut self.rng;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_tree"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op);
                let card = glass(ui, &tk);
                let mut inner = ui.new_child(UiBuilder::new().max_rect(card.shrink(16.0)));
                let ui = &mut inner;
                let header = match c.header {
                    Header::Quip(n) => fp_text::tree_quip(lang, n),
                    Header::Refuse(1) => fp_text::t(lang, Key::TreeRefuse1),
                    Header::Refuse(_) => fp_text::t(lang, Key::TreeRefuse2),
                };
                ui.horizontal(|ui| {
                    widgets::wrapped(ui, header, 13.0, Color32::WHITE);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if round_button(ui, Glyph::Close, true).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(10.0);
                // Height.
                widgets::label(ui, fp_text::t(lang, Key::TreeHeight), 11.0, Color32::from_white_alpha(150));
                ui.horizontal(|ui| {
                    if round_text_button(ui, "\u{2212}", c.look.level > 0).clicked() {
                        c.look = c.look.stepped(-1);
                        out.push(CardAction::RestyleTree { tree: c.tree, look: c.look.clone() });
                    }
                    ui.add_space(6.0);
                    widgets::label(ui, fp_text::tree_size(lang, c.look.level), 13.0, Color32::WHITE);
                    ui.add_space(6.0);
                    if round_text_button(ui, "+", c.look.level < fp_trees::MAX_LEVEL).clicked() {
                        c.look = c.look.stepped(1);
                        out.push(CardAction::RestyleTree { tree: c.tree, look: c.look.clone() });
                    }
                });
                ui.add_space(8.0);
                widgets::label(ui, fp_text::t(lang, Key::TreeTopper), 11.0, Color32::from_white_alpha(150));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for t in Topper::ALL {
                        let icon = tex.pixmap(ui.ctx(), &format!("topper:{t:?}"), |s| fp_trees::topper_icon(t, s));
                        let name = fp_text::topper_name(lang, &format!("{t:?}")).unwrap_or("");
                        if chip(ui, vec2(50.0, 60.0), icon.as_ref(), 30.0, name, c.look.topper == Some(t), true)
                            .clicked()
                        {
                            c.look = c.look.topper_clicked(t);
                            out.push(CardAction::RestyleTree { tree: c.tree, look: c.look.clone() });
                        }
                    }
                });
                ui.add_space(8.0);
                widgets::label(ui, fp_text::t(lang, Key::TreeTrims), 11.0, Color32::from_white_alpha(150));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for t in Trim::ALL {
                        let icon = tex.pixmap(ui.ctx(), &format!("trim:{t:?}"), |s| fp_trees::trim_icon(t, s));
                        let name = fp_text::trim_name(lang, &format!("{t:?}")).unwrap_or("");
                        if chip(ui, vec2(74.0, 64.0), icon.as_ref(), 32.0, name, c.look.has(t), true).clicked() {
                            c.look = c.look.toggled(t);
                            out.push(CardAction::RestyleTree { tree: c.tree, look: c.look.clone() });
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if widgets::quiet_button(ui, &tk, fp_text::t(lang, Key::TreeRandomise), 30.0, true).clicked() {
                        c.look = c.look.randomised(rng);
                        out.push(CardAction::RestyleTree { tree: c.tree, look: c.look.clone() });
                    }
                    if widgets::quiet_button(ui, &tk, fp_text::t(lang, Key::TreeChop), 30.0, true).clicked() {
                        out.push(CardAction::DoomTree(c.tree));
                        close = true;
                    }
                });
                escape_close(ui, &mut close);
            });
        });
        self.actions.extend(out);
        if close {
            c.at.fade.fade(now);
        }
        if c.at.fade.finished(now) {
            self.actions.push(CardAction::TreeCardClosed);
        } else {
            self.tree = Some(c);
        }
    }

    fn show_quote(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.quote.take() else { return };
        let _ = lang;
        let op = c.at.fade.opacity(now);
        let mut close = false;
        let builder = Self::builder(&c.at).with_mouse_passthrough(false);
        let size = c.at.size;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_quote"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op);
                let card = Rect::from_min_size(
                    pos2(GUTTER as f32, GUTTER as f32),
                    vec2(QUOTE_W as f32, size.1 - 2.0 * GUTTER as f32),
                );
                let navy = theme::rgb(theme::NAVY);
                let p = ui.painter();
                p.rect_filled(
                    card.translate(vec2(0.0, 6.0)).expand(4.0),
                    26.0,
                    Color32::from_black_alpha(theme::alpha_byte(0.18 * f64::from(op))),
                );
                let (top, bottom) = theme::snow_day();
                p.rect_filled(card, 22.0, bottom);
                art::gradient(
                    &p.with_clip_rect(card.shrink2(vec2(0.0, 22.0))),
                    card.shrink2(vec2(0.0, 22.0)),
                    top,
                    bottom,
                );
                p.rect_filled(
                    Rect::from_min_size(card.min, vec2(card.width(), 22.0)),
                    egui::CornerRadius { nw: 22, ne: 22, sw: 0, se: 0 },
                    top,
                );
                for d in fp_snow::decor::dots(18, f64::from(card.width()), f64::from(card.height()), now) {
                    p.circle_filled(
                        card.min + vec2(d.x as f32, d.y as f32),
                        d.radius as f32,
                        Color32::from_white_alpha((230.0 * d.alpha) as u8),
                    );
                }
                crate::home::paint_bank(ui, card, 34.0, 0.9);
                let mut inner = ui.new_child(
                    UiBuilder::new()
                        .max_rect(Rect::from_min_max(card.min + vec2(22.0, 22.0), card.max - vec2(22.0, 68.0))),
                );
                widgets::wrapped(&mut inner, &c.text, 21.0, navy);
                if let Some(by) = &c.by {
                    inner.add_space(8.0);
                    widgets::label(&mut inner, by, 14.0, navy.gamma_multiply(0.65));
                }
                // Red badge, bottom right.
                let badge_text = fp_text::gift_thanks(lang, c.thanks);
                let g = ui.painter().layout_no_wrap(badge_text.to_string(), widgets::font(14.0), Color32::WHITE);
                let bsize = vec2(g.size().x + 30.0 + 20.0, g.size().y + 16.0);
                let b = Rect::from_min_size(pos2(card.right() - 14.0 - bsize.x, card.bottom() - 14.0 - bsize.y), bsize);
                let red = theme::rgb(theme::BADGE_RED);
                ui.painter().rect_filled(b.translate(vec2(0.0, 2.0)).expand(2.0), 100.0, red.gamma_multiply(0.4));
                ui.painter().rect_filled(b, 100.0, red);
                ui.painter().rect_stroke(
                    b,
                    100.0,
                    egui::Stroke::new(1.0, Color32::from_white_alpha(theme::alpha_byte(0.55))),
                    egui::StrokeKind::Inside,
                );
                widgets::glyph(
                    ui.painter(),
                    Glyph::SnowOnly,
                    pos2(b.left() + 15.0 + 7.0, b.center().y),
                    14.0,
                    Color32::WHITE,
                );
                ui.painter().galley(pos2(b.left() + 15.0 + 20.0, b.center().y - g.size().y / 2.0), g, Color32::WHITE);
                ui.painter().rect_stroke(
                    card,
                    22.0,
                    egui::Stroke::new(1.5, Color32::from_white_alpha(theme::alpha_byte(0.8))),
                    egui::StrokeKind::Inside,
                );
                // A left press on the card fades it.
                let resp = ui.interact(card, egui::Id::unique("fp_quote_card"), Sense::click());
                if resp.is_pointer_button_down_on() || resp.clicked() {
                    close = true;
                }
            });
        });
        if close || now - c.born >= c.auto {
            c.at.fade.fade(now);
        }
        if !c.at.fade.finished(now) {
            self.quote = Some(c);
        }
    }

    fn show_limit(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.limit.take() else { return };
        let age = now - c.born;
        if !age.is_finite() || age >= cardmodel::limit_total() {
            // Timeline over: the window closes and the slot frees.
            return;
        }
        let dt = (now - c.last).clamp(0.0, puppet::MAX_DT);
        c.last = now;
        if !c.popped && age >= cardmodel::LIMIT_BUBBLE_AT {
            c.popped = true;
            c.puppet.start_hop();
        }
        let goal = cardmodel::limit_goal_from_right(age);
        c.puppet.step(dt, cardmodel::LIMIT_SIZE.0 - goal);
        let leaving = goal == cardmodel::LIMIT_EXIT;
        let bubble_a = cardmodel::limit_bubble_alpha(age) as f32;
        // Click-through and never activated: the window takes no mouse input.
        let builder = Self::builder(&c.at).with_mouse_passthrough(true).with_active(false);
        let puppet_state = c.puppet;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_limit"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                let full = ui.max_rect();
                // Bubble: 240 wide, anchored bottom-right above the puppet.
                if bubble_a > 0.0 {
                    let pop = 0.85 + 0.15 * bubble_a;
                    let size = vec2(240.0 * pop, 104.0 * pop);
                    let br = pos2(full.right() - 66.0, full.bottom() - 40.0);
                    let bubble = Rect::from_min_max(br - size, br);
                    let p = ui.painter();
                    let fade = |c: Color32| c.gamma_multiply(bubble_a);
                    p.rect_filled(
                        bubble.translate(vec2(0.0, 4.0)).expand(2.0),
                        18.0,
                        fade(theme::rgb_a(0x1A3373, 0.2)),
                    );
                    p.rect_filled(bubble, 16.0, fade(Color32::WHITE));
                    // Red header strip with a vertical gradient.
                    let head = Rect::from_min_size(bubble.min, vec2(bubble.width(), 28.0 * pop));
                    let mut mesh = egui::Mesh::default();
                    let (top, bottom) = (fade(theme::rgb(0xE83340)), fade(theme::rgb(0xC21A2B)));
                    mesh.colored_vertex(head.left_top() + vec2(8.0, 0.0), top);
                    mesh.colored_vertex(head.right_top() - vec2(8.0, 0.0), top);
                    mesh.colored_vertex(head.right_bottom(), bottom);
                    mesh.colored_vertex(head.left_bottom(), bottom);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    p.rect_filled(
                        Rect::from_min_size(head.min, vec2(head.width(), 16.0)),
                        egui::CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 },
                        top,
                    );
                    p.add(egui::Shape::mesh(mesh));
                    let title = format!("\u{2744}  {}  \u{2744}", fp_text::t(lang, Key::TreeLimitTitle));
                    p.text(
                        head.center(),
                        egui::Align2::CENTER_CENTER,
                        title,
                        widgets::font(11.0),
                        fade(Color32::WHITE),
                    );
                    // Tail pointing down-right at the puppet.
                    let tail = [
                        pos2(bubble.right() - 30.0, bubble.bottom() - 1.0),
                        pos2(bubble.right() - 12.0, bubble.bottom() - 1.0),
                        pos2(bubble.right() + 4.0, bubble.bottom() + 14.0),
                    ];
                    p.add(egui::Shape::convex_polygon(tail.to_vec(), fade(Color32::WHITE), egui::Stroke::NONE));
                    let body = Rect::from_min_max(pos2(bubble.left(), head.bottom()), bubble.max).shrink(14.0 * pop);
                    let mut inner = ui.new_child(UiBuilder::new().max_rect(body));
                    let navy = fade(theme::rgb(theme::NAVY));
                    let text = egui::RichText::new(fp_text::t(lang, Key::TreeLimitLine))
                        .font(widgets::font(13.0 * pop))
                        .strong()
                        .color(navy);
                    inner.add(egui::Label::new(text).wrap().selectable(false));
                }
                // Puppet on top of the bubble (F9 layering).
                let scale = ui.ctx().pixels_per_point();
                let (pw, ph) = (80.0f32, 64.0f32);
                let (w, h) = ((pw * scale).ceil() as u32, (ph * scale).ceil() as u32);
                if let Some(mut pm) = fp_paint::tiny_skia::Pixmap::new(w, h) {
                    puppet::paint(&mut pm, scale, puppet::POPUP_SIZE, &puppet_state, &Look::classic(), !leaving);
                    let img = art::pixmap_image(&pm);
                    let tex: TextureHandle = ui.ctx().load_texture("limit_puppet", img, egui::TextureOptions::LINEAR);
                    let x = full.left() + puppet_state.x as f32;
                    let r = Rect::from_min_size(pos2(x - pw / 2.0, full.bottom() - ph - 4.0), vec2(pw, ph));
                    ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            });
        });
        self.limit = Some(c);
    }

    /// Tray pointer (core-app 2.7): pulse rings around the icon, an arrow and
    /// a hint pill that bob towards it. Click-through, never activated.
    fn show_pointer(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(c) = self.pointer.take() else { return };
        let age = now - c.born;
        let Some(op) = cardmodel::pointer_opacity(age) else { return };
        let builder = Self::builder(&c.at).with_mouse_passthrough(true).with_active(false);
        let plan = c.plan;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_pointer"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op as f32);
                let full = ui.max_rect();
                let accent = theme::rgb(theme::ACCENT);
                let p = ui.painter();
                let ring = full.min + vec2(plan.ring.0 as f32, plan.ring.1 as f32);
                let r = (plan.ring_side / 2.0) as f32;
                for i in 0..2 {
                    let (scale, a) = cardmodel::pointer_ring(i, age);
                    if a > 0.0 {
                        p.circle_stroke(
                            ring,
                            r * scale as f32,
                            egui::Stroke::new(3.0, accent.gamma_multiply(a as f32)),
                        );
                    }
                }
                p.circle_stroke(ring, r + 1.5, egui::Stroke::new(6.0, theme::rgb_a(0x1F70E0, 0.25)));
                p.circle_stroke(ring, r, egui::Stroke::new(3.0, accent));
                // Arrow then pill (below the icon) or pill then arrow, gap 6.
                let bob = cardmodel::pointer_bob(plan.below, age) as f32;
                let (arrow_h, pill_h, gap) = (18.0f32, 28.0f32, 6.0f32);
                let stack_h = arrow_h + gap + pill_h;
                let top = if plan.below { ring.y + r + 4.0 } else { ring.y - r - 4.0 - stack_h } + bob;
                let (arrow_top, pill_top) =
                    if plan.below { (top, top + arrow_h + gap) } else { (top + pill_h + gap, top) };
                let cx = ring.x;
                let tri = if plan.below {
                    vec![
                        pos2(cx, arrow_top),
                        pos2(cx + 11.0, arrow_top + arrow_h),
                        pos2(cx - 11.0, arrow_top + arrow_h),
                    ]
                } else {
                    vec![pos2(cx - 11.0, arrow_top), pos2(cx + 11.0, arrow_top), pos2(cx, arrow_top + arrow_h)]
                };
                let shadow: Vec<_> = tri.iter().map(|q| *q + vec2(0.0, 1.0)).collect();
                p.add(egui::Shape::convex_polygon(shadow, Color32::from_black_alpha(64), egui::Stroke::NONE));
                p.add(egui::Shape::convex_polygon(tri, accent, egui::Stroke::NONE));
                let font = widgets::font(13.0);
                let text = fp_text::t(lang, Key::TrayPointerHint);
                let galley = p.layout_no_wrap(text.to_string(), font, Color32::WHITE);
                let pill_w = (galley.size().x + 24.0).min(full.width());
                let pill_left = (cx - pill_w / 2.0).clamp(full.left(), full.right() - pill_w);
                let pill = Rect::from_min_size(pos2(pill_left, pill_top), vec2(pill_w, pill_h));
                p.rect_filled(pill.translate(vec2(0.0, 2.0)).expand(1.0), 15.0, Color32::from_black_alpha(48));
                p.rect_filled(pill, 14.0, accent);
                p.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
            });
        });
        self.pointer = Some(c);
    }

    fn show_station(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.station.take() else { return };
        let tk = Tokens::new(true);
        let op = c.at.fade.opacity(now);
        let mut out = Vec::new();
        let mut done = false;
        let builder = Self::builder(&c.at);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_station"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op);
                let card = glass(ui, &tk);
                let mut inner = ui.new_child(UiBuilder::new().max_rect(card.shrink(16.0)));
                let ui = &mut inner;
                widgets::label(ui, fp_text::t(lang, fp_text::Key::StationTitle), 13.0, Color32::WHITE);
                ui.add_space(8.0);
                let (mut snowmen, mut trees) = (0, 0);
                for p in &c.choices {
                    let label = match p {
                        Pick::Snowman(_) => {
                            snowmen += 1;
                            format!("{} {snowmen}", fp_text::t(lang, fp_text::Key::StationSnowman))
                        }
                        Pick::Tree(_) => {
                            trees += 1;
                            format!("{} {trees}", fp_text::t(lang, fp_text::Key::StationTree))
                        }
                    };
                    if red_gold(ui, &label).clicked() {
                        out.push(CardAction::TrainFetch(*p));
                        done = true;
                    }
                    ui.add_space(4.0);
                }
                if widgets::quiet_button(ui, &tk, fp_text::t(lang, fp_text::Key::StationWave), 30.0, true).clicked() {
                    out.push(CardAction::TrainDismiss);
                    done = true;
                }
            });
        });
        self.actions.extend(out);
        if done {
            c.at.fade.fade(now);
        }
        if !c.at.fade.finished(now) {
            self.station = Some(c);
        }
    }

    fn show_update(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.update.take() else { return };
        let tk = Tokens::new(true);
        let op = c.at.fade.opacity(now);
        let mut later = false;
        let mut open = false;
        let builder = Self::builder(&c.at);
        let tex = &mut self.tex;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_update"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op);
                let card = glass(ui, &tk);
                let mut inner = ui.new_child(UiBuilder::new().max_rect(card.shrink2(vec2(24.0, 22.0))));
                let ui = &mut inner;
                ui.horizontal(|ui| {
                    if let Some(t) = tex.pixmap(ui.ctx(), "mascot:48", |s| crate::home::mascot_pixmap(48.0, s, false)) {
                        let (r, _) = ui.allocate_exact_size(vec2(38.4, 48.0), Sense::hover());
                        ui.painter().image(
                            t.id(),
                            r,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        widgets::label(ui, fp_text::t(lang, Key::UpdPromptTitle), 17.0, Color32::WHITE);
                        ui.add_space(3.0);
                        widgets::label(
                            ui,
                            &fp_text::update_available(lang, &c.version),
                            12.0,
                            Color32::from_white_alpha(150),
                        );
                    });
                });
                ui.add_space(16.0);
                widgets::wrapped(
                    ui,
                    &fp_text::update_prompt_body(lang, &c.version),
                    13.0,
                    Color32::from_white_alpha(150),
                );
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary_button(ui, &tk, fp_text::t(lang, Key::UpdOpenPage), true).clicked() {
                        open = true;
                    }
                    ui.add_space(10.0);
                    if widgets::quiet_button(ui, &tk, fp_text::t(lang, Key::UpdLater), 36.0, true).clicked() {
                        later = true;
                    }
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    later = true;
                }
            });
        });
        if open {
            self.actions.push(CardAction::OpenUrl(c.url.clone()));
        }
        if later || open {
            // Closing without accepting counts as "later" again.
            self.actions.push(CardAction::UpdateLater);
            c.at.fade.fade(now);
        }
        if !c.at.fade.finished(now) {
            self.update = Some(c);
        }
    }

    fn show_picker(&mut self, ctx: &Context, lang: Lang, now: f64) {
        let Some(mut c) = self.picker.take() else { return };
        let tk = Tokens::new(true);
        let op = c.at.fade.opacity(now);
        let mut out = Vec::new();
        let mut close = false;
        let builder = Self::builder(&c.at);
        let tex = &mut self.tex;
        let full = c.count >= c.max;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("fp_card_picker"), builder, |ui, _| {
            transparent_panel(ui, |ui| {
                ui.set_opacity(op);
                let card = glass(ui, &tk);
                let mut inner = ui.new_child(UiBuilder::new().max_rect(card.shrink(16.0)));
                let ui = &mut inner;
                ui.horizontal(|ui| {
                    widgets::label(ui, fp_text::t(lang, Key::DecorTitle), 15.0, Color32::WHITE);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if round_button(ui, Glyph::Close, true).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(2.0);
                widgets::wrapped(ui, fp_text::t(lang, Key::DecorLine), 11.5, Color32::from_white_alpha(150));
                ui.add_space(6.0);
                let count_line = if full {
                    fp_text::t(lang, Key::DecorFull).to_string()
                } else {
                    fp_text::decor_count(lang, c.count, c.max)
                };
                widgets::label(ui, &count_line, 11.0, Color32::from_white_alpha(130));
                ui.add_space(8.0);
                for row in Kind::ALL.chunks(PICKER_COLUMNS) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for &k in row {
                            let icon =
                                tex.pixmap(ui.ctx(), &format!("decor:{k:?}"), |_| Some(fp_decor::icon_square(k, 96)));
                            let name = fp_text::decor_name(lang, k.raw()).unwrap_or("");
                            if chip(ui, vec2(84.0, 82.0), icon.as_ref(), 48.0, name, false, !full).clicked() {
                                out.push(CardAction::AddDecoration(k));
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if red_gold(ui, fp_text::t(lang, Key::DecorTree)).clicked() {
                        tree_tile_clicked(&mut out, &mut close);
                    }
                    widgets::wrapped(ui, fp_text::t(lang, Key::DecorTreeLine), 11.0, Color32::from_white_alpha(150));
                });
                ui.add_space(8.0);
                if c.confirm_all {
                    ui.horizontal(|ui| {
                        if widgets::primary_button(ui, &tk, fp_text::t(lang, Key::SetConfirm), true).clicked() {
                            out.push(CardAction::RemoveAllDecorations);
                            c.confirm_all = false;
                        }
                        if widgets::quiet_button(ui, &tk, fp_text::t(lang, Key::SetCancel), 36.0, true).clicked() {
                            c.confirm_all = false;
                        }
                    });
                } else if widgets::quiet_button(ui, &tk, fp_text::t(lang, Key::DecorTakeAllDown), 30.0, c.count > 0)
                    .clicked()
                {
                    c.confirm_all = true;
                }
                focus_close(ui, &mut c.at, &mut close);
            });
        });
        self.actions.extend(out);
        if close {
            c.at.fade.fade(now);
        }
        if !c.at.fade.finished(now) {
            self.picker = Some(c);
        }
    }
}

/// Item caption of chip `i` on a dress-up tab.
pub fn chip_name(lang: Lang, tab: Tab, i: usize) -> String {
    let id = match tab {
        Tab::Hat => Hat::ALL.get(i).map(|h| h.id()),
        Tab::Face => Face::ALL.get(i).map(|f| f.id()),
        Tab::Body => Arms::ALL.get(i).map(|a| a.id()),
        Tab::Clothes => Outfit::ALL.get(i).map(|o| o.id()),
    };
    id.map(|id| fp_text::item_name(lang, id)).unwrap_or_default()
}

/// Root panel without background or margin (the viewport is transparent).
fn transparent_panel(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, add);
}

/// Glass card (ui 3.9 CardParts.Card): fills the window minus the shadow
/// gutter and returns the card rect.
fn glass(ui: &mut Ui, _tk: &Tokens) -> Rect {
    let full = ui.max_rect();
    let card = full.shrink(dress::ROOM as f32);
    let p = ui.painter();
    for i in 0..6 {
        let g = i as f32 * 3.0;
        p.rect_filled(card.translate(vec2(0.0, 5.0)).expand(g), 14.0 + g, Color32::from_black_alpha(10));
    }
    p.rect_filled(card, 14.0, Color32::from_rgba_unmultiplied(0x29, 0x29, 0x2E, theme::alpha_byte(0.97)));
    p.rect_stroke(
        card,
        14.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(theme::alpha_byte(0.10))),
        egui::StrokeKind::Inside,
    );
    card
}

/// 22 px round button with a glyph (CardParts.Round).
fn round_button(ui: &mut Ui, g: Glyph, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let op = if enabled { 1.0 } else { 0.4 };
    ui.painter().circle_filled(rect.center(), 11.0, Color32::from_white_alpha(theme::alpha_byte(0.10 * op)));
    widgets::glyph(ui.painter(), g, rect.center(), 9.0, Color32::from_white_alpha(theme::alpha_byte(0.7 * op)));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn round_text_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let op = if enabled { 1.0 } else { 0.4 };
    ui.painter().circle_filled(rect.center(), 11.0, Color32::from_white_alpha(theme::alpha_byte(0.10 * op)));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        widgets::font(14.0),
        Color32::from_white_alpha(theme::alpha_byte(0.8 * op)),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn tab_pill(ui: &mut Ui, text: &str, on: bool, width: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let fill = if on {
        Color32::from_white_alpha(theme::alpha_byte(0.20))
    } else if resp.hovered() {
        Color32::from_white_alpha(theme::alpha_byte(0.10))
    } else {
        Color32::from_white_alpha(theme::alpha_byte(0.05))
    };
    ui.painter().rect_filled(rect, 13.0, fill);
    let g = shrink_to(
        ui,
        text,
        11.5,
        width - 8.0,
        Color32::from_white_alpha(theme::alpha_byte(if on { 1.0 } else { 0.8 })),
    );
    ui.painter().galley(rect.center() - g.size() / 2.0, g, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A single-line galley whose font shrinks (never grows) to fit `max_w`.
fn shrink_to(ui: &Ui, text: &str, size: f32, max_w: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let g = ui.painter().layout_no_wrap(text.to_string(), widgets::font(size), color);
    if g.size().x <= max_w || g.size().x <= 0.0 {
        return g;
    }
    ui.painter().layout_no_wrap(text.to_string(), widgets::font((size * max_w / g.size().x).max(6.0)), color)
}

/// Selectable tile with an icon and a caption (CardParts.Chip).
fn chip(
    ui: &mut Ui,
    size: egui::Vec2,
    icon: Option<&TextureHandle>,
    side: f32,
    caption: &str,
    on: bool,
    enabled: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let op = if enabled { 1.0 } else { 0.4 };
    let fill = if on {
        0.20
    } else if resp.hovered() && enabled {
        0.12
    } else {
        0.05
    };
    ui.painter().rect_filled(rect, 12.0, Color32::from_white_alpha(theme::alpha_byte(fill * op)));
    if on {
        ui.painter().rect_stroke(rect, 12.0, egui::Stroke::new(1.5, theme::rgb(0x66B2FF)), egui::StrokeKind::Inside);
    }
    let hover_grow = if resp.hovered() && enabled { 1.1 } else { 1.0 };
    if let Some(t) = icon {
        let s = side * hover_grow;
        let r = Rect::from_center_size(pos2(rect.center().x, rect.top() + 4.0 + side / 2.0), vec2(s, s));
        ui.painter().image(
            t.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE.gamma_multiply(op as f32),
        );
    }
    let g = shrink_to(
        ui,
        caption,
        10.5,
        size.x - 6.0,
        Color32::from_white_alpha(theme::alpha_byte((if on { 1.0 } else { 0.8 }) * op)),
    );
    ui.painter().galley(pos2(rect.center().x - g.size().x / 2.0, rect.bottom() - 4.0 - g.size().y), g, Color32::WHITE);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// The picker's tree tile: plant a tree, then dismiss the panel (decorations 3.1).
fn tree_tile_clicked(out: &mut Vec<CardAction>, close: &mut bool) {
    out.push(CardAction::PlantTree);
    *close = true;
}

/// Red pill with a gold outline (CardParts.RedGold).
fn red_gold(ui: &mut Ui, text: &str) -> Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), widgets::font(13.5), Color32::WHITE);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 44.0, 32.0), Sense::click());
    let scale = if resp.is_pointer_button_down_on() { 0.96 } else { 1.0 };
    let rect = Rect::from_center_size(rect.center(), rect.size() * scale);
    ui.painter().rect_filled(rect, 16.0, theme::rgb(0xC70F2E));
    ui.painter().rect_stroke(rect, 16.0, egui::Stroke::new(1.5, theme::rgb(0xE8B84A)), egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Close a card when it loses focus after having had it (the egui stand-in
/// for the original's click-anywhere watcher) or on Escape.
fn focus_close(ui: &Ui, at: &mut Placed, close: &mut bool) {
    escape_close(ui, close);
    let focused = ui.input(|i| i.viewport().focused).unwrap_or(false);
    if focused {
        at.had_focus = true;
    } else if at.had_focus {
        *close = true;
    }
}

/// Escape closes a card.
fn escape_close(ui: &Ui, close: &mut bool) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        *close = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::desktop::MonitorInfo;
    use fp_core::geom::PixelRect;

    fn layout() -> Layout {
        Layout::new(vec![MonitorInfo {
            id: "a".into(),
            name: "A".into(),
            pixels: PixelRect { left: 0, top: 0, right: 1920, bottom: 1080 },
            work: PixelRect { left: 0, top: 0, right: 1920, bottom: 1040 },
            scale: 1.0,
            primary: true,
        }])
    }

    #[test]
    fn quote_height_grows_with_text() {
        let short = quote_height("Hi", None);
        let long = quote_height(&"word ".repeat(60), None);
        assert!(long > short);
        assert!(quote_height("Hi", Some("Someone")) > short);
        // CJK counts wider than Latin for the same number of characters.
        assert!(quote_height(&"雪".repeat(40), None) > quote_height(&"a".repeat(40), None));
    }

    #[test]
    fn close_station_and_snow_cards() {
        let l = layout();
        let cxx = cx(&l, &[]);
        let st = || CardRequest::TrainStation {
            at: Vec2::new(300.0, 100.0),
            choices: vec![(Pick::Snowman(1), Vec2::new(1.0, 1.0))],
        };
        let mut c = Cards::new(1);
        c.open(st(), &cxx, 0.0);
        assert!(c.station_open());
        c.close_station();
        assert!(!c.station_open() && !c.any_open());
        c.open(st(), &cxx, 0.0);
        c.open(CardRequest::DressUp { bottom: 1, look: None }, &cxx, 0.0);
        c.open(CardRequest::Quote { at: Vec2::new(900.0, 600.0), quote: 0 }, &cxx, 0.0);
        c.close_snow_cards();
        assert!(!c.any_open());
    }

    #[test]
    fn station_size_grows_with_choices() {
        assert!(station_size(2).1 > station_size(1).1);
        assert_eq!(station_size(0).0, 260.0 + 48.0);
    }

    #[test]
    fn chip_names_exist_for_every_chip() {
        for t in Tab::ALL {
            for i in 0..dress::CHIPS {
                assert!(!chip_name(Lang::En, t, i).is_empty());
            }
            assert!(chip_name(Lang::En, t, 99).is_empty());
        }
    }

    fn cx<'a>(l: &'a Layout, sprites: &'a [Sprite]) -> CardCtx<'a> {
        CardCtx { lang: Lang::En, layout: l, sprites, cursor: Vec2::new(500.0, 500.0), tree_origin: None }
    }

    #[test]
    fn requests_open_their_cards() {
        let l = layout();
        let mut c = Cards::new(1);
        assert!(!c.any_open());
        let cxx = cx(&l, &[]);
        c.open(CardRequest::DressUp { bottom: 1, look: None }, &cxx, 0.0);
        c.open(CardRequest::TreeStyle { tree: 4, look: TreeLook::default(), quip: Some(1) }, &cxx, 0.0);
        c.open(CardRequest::Quote { at: Vec2::new(900.0, 600.0), quote: 0 }, &cxx, 0.0);
        c.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 1920.0, 1040.0) }, &cxx, 0.0);
        c.open(
            CardRequest::TrainStation {
                at: Vec2::new(300.0, 100.0),
                choices: vec![(Pick::Snowman(1), Vec2::new(1.0, 1.0)), (Pick::Tree(2), Vec2::new(2.0, 2.0))],
            },
            &cxx,
            0.0,
        );
        assert_eq!(c.picker_rect(), None);
        c.toggle_picker(&cxx, 3, 100, 0.0);
        assert!(c.any_open() && c.picker_open() && c.limit_open());
        let r = c.picker_rect().unwrap();
        assert_eq!((r.w, r.h), (400.0, 560.0));
        assert_eq!(c.tree_open(), Some(4));
        c.set_decor_counts(5, 100);
        assert_eq!(c.picker_counts(), Some((5, 100)));
        // A second limit request while one is open changes nothing.
        c.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 10.0, 10.0) }, &cxx, 1.0);
        c.toggle_picker(&cxx, 3, 100, 0.0);
        assert!(!c.picker_open());
        c.close_tree();
        assert_eq!(c.tree_open(), None);
        c.close_all();
        assert!(!c.any_open());
        c.set_decor_counts(1, 2);
        assert_eq!(c.picker_counts(), None);
    }

    #[test]
    fn limit_snowman_walks_in_pops_and_leaves_on_time() {
        let l = layout();
        let mut c = Cards::new(4);
        c.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 1920.0, 1040.0) }, &cx(&l, &[]), 0.0);
        let lim = c.limit.as_ref().unwrap();
        assert_eq!(lim.at.size, LIMIT_WINDOW);
        // Starts outside the right edge.
        assert_eq!(lim.puppet.x, cardmodel::LIMIT_SIZE.0 + 50.0);
        let ctx = Context::default();
        let mut t = 0.0;
        while t < 3.0 {
            t += 0.02;
            crate::testutil::frame(&ctx, |_| c.show_limit(&ctx, Lang::En, t));
        }
        let lim = c.limit.as_ref().unwrap();
        assert!(lim.popped);
        assert!((lim.puppet.x - (cardmodel::LIMIT_SIZE.0 - cardmodel::LIMIT_REST)).abs() <= 0.5);
        while t < 6.5 {
            t += 0.02;
            crate::testutil::frame(&ctx, |_| c.show_limit(&ctx, Lang::En, t));
        }
        // Walking back out to the right.
        assert!(c.limit.as_ref().unwrap().puppet.x > cardmodel::LIMIT_SIZE.0 - cardmodel::LIMIT_REST + 50.0);
        crate::testutil::frame(&ctx, |_| c.show_limit(&ctx, Lang::En, 6.61));
        assert!(!c.limit_open());
        // The slot is free again.
        c.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 1920.0, 1040.0) }, &cx(&l, &[]), 7.0);
        assert!(c.limit_open());
    }

    #[test]
    fn outside_press_fades_dress_and_tree_only_once() {
        let l = layout();
        let mut c = Cards::new(5);
        assert!(!c.outside_watch());
        let cxx = cx(&l, &[]);
        c.open(CardRequest::Quote { at: Vec2::new(900.0, 600.0), quote: 0 }, &cxx, 0.0);
        c.toggle_picker(&cxx, 0, 100, 0.0);
        // Quote and picker never arm the outside watcher.
        assert!(!c.outside_watch());
        c.open(CardRequest::DressUp { bottom: 1, look: None }, &cxx, 0.0);
        c.open(CardRequest::TreeStyle { tree: 4, look: TreeLook::default(), quip: None }, &cxx, 0.0);
        assert!(c.outside_watch());
        c.close_outside(1.0);
        assert!(!c.outside_watch());
        assert!(c.dress.as_ref().unwrap().at.fade.is_fading());
        assert!(c.quote.as_ref().is_some_and(|q| !q.at.fade.is_fading()));
        assert!(c.picker_open());
        // The tree card ends in TreeCardClosed once its fade is over.
        let ctx = Context::default();
        crate::testutil::frame(&ctx, |_| c.show_tree(&ctx, Lang::En, 2.0));
        assert_eq!(c.tree_open(), None);
        assert!(matches!(c.actions.last(), Some(CardAction::TreeCardClosed)));
        crate::testutil::frame(&ctx, |_| c.show_dress(&ctx, Lang::En, 2.0));
        assert!(c.dress.is_none());
    }

    #[test]
    fn quote_index_out_of_range_falls_back() {
        let l = layout();
        let mut c = Cards::new(2);
        c.open(CardRequest::Quote { at: Vec2::new(10.0, 10.0), quote: 10_000 }, &cx(&l, &[]), 0.0);
        assert!(c.quote.is_some());
    }

    /// Every card draws a frame headlessly (non-embedded viewports run their
    /// closure only under a real backend, so this exercises the open path).
    #[test]
    fn cards_show_in_a_frame() {
        let l = layout();
        let mut c = Cards::new(3);
        let cxx = cx(&l, &[]);
        c.open(CardRequest::DressUp { bottom: 1, look: None }, &cxx, 0.0);
        c.open(CardRequest::TreeStyle { tree: 4, look: TreeLook::default(), quip: None }, &cxx, 0.0);
        c.open(CardRequest::Quote { at: Vec2::new(900.0, 600.0), quote: 0 }, &cxx, 0.0);
        c.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 1920.0, 1040.0) }, &cxx, 0.0);
        c.toggle_picker(&cxx, 0, 100, 0.0);
        c.open_update("9.9.9", "https://example.org/r", &cxx, 0.0);
        c.open_update("1.0.0", "https://example.org/x", &cxx, 0.0);
        let ctx = Context::default();
        let mut acts = Vec::new();
        crate::testutil::frame(&ctx, |_| {
            acts.extend(c.show(&ctx, Lang::En));
        });
        assert!(acts.is_empty());
        let _ = Hat::ALL;
    }
}
