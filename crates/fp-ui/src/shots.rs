//! Headless screenshot harness and the widget id clash gate.
//!
//! Every screen and meaningful state of the UI is a [`Scene`]. Two tests
//! drive them through `egui_kittest` (no window, no real input):
//!
//! * `id_clash_gate` (always on, no GPU): runs every scene in English and
//!   fails when egui reports a widget id clash (debug builds paint
//!   "First/Second/Double use of ... ID" with a red box; we read those
//!   texts back from the frame's shapes).
//! * `ui_shots` (only with `FROSTPANE_SHOTS=1`): renders every scene with
//!   wgpu in en, de, ja, ru, tr and kk to
//!   `target/ui-shots/<lang>/<area>-<name>.png`, writes
//!   `target/ui-shots/clashes.txt` and `target/ui-shots/problems.txt`
//!   (missing glyphs and other panics), and fails when either is not empty.
//!
//! ```text
//! FROSTPANE_SHOTS=1 cargo test -p fp-ui --lib shots::ui_shots -- --nocapture
//! ```
//!
//! `FROSTPANE_SHOTS_DIR` overrides the output folder, `FROSTPANE_SHOTS_LANGS`
//! (comma separated codes) the languages and `FROSTPANE_SHOTS_FILTER` keeps
//! only scenes whose `<area>-<name>` contains the given text.

use crate::cardmodel::CityState;
use crate::model::{DisplayInfo, Pane, Tab};
use crate::shell::{HostInfo, LoginNote, Notices, Shell, UiAction, UpdateLine};
use crate::{CardCtx, Cards, UpdateFail, UpdatePhase};
use eframe::egui::{self, Color32, Pos2, Rect, Shape, Vec2, vec2};
use egui_kittest::Harness;
use fp_core::clock::WallTime;
use fp_core::desktop::{Layout, MonitorInfo};
use fp_core::effect::{EffectKind, Lang};
use fp_core::geom::{Frame, PixelRect, Vec2 as GVec2};
use fp_core::settings::{City, Settings, Weathers};
use fp_text::{Key, ManualReason};
use fp_world::CardRequest;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Simulated seconds per frame.
const DT: f32 = 0.05;
/// Frames before the picture (1.2 s: every 0.35 s animation has settled).
const FRAMES: usize = 24;
/// Frames per scene in the clash gate.
const GATE_FRAMES: usize = 8;
/// Extra frames after a hover (tooltips wait 0.5 s).
const HOVER_FRAMES: usize = 16;
/// Wall clock of every scene (2023-11-14 22:13 UTC).
const WALL_UNIX: i64 = 1_700_000_000;
/// Languages of the screenshot run: Latin, long words, CJK, Cyrillic,
/// Turkish (dotted/dotless i) and Kazakh (extended Cyrillic).
const SHOT_LANGS: [Lang; 6] = [Lang::En, Lang::De, Lang::Ja, Lang::Ru, Lang::Tr, Lang::Kk];

/// What a scene draws.
struct World {
    lang: Lang,
    weathers: Weathers,
    shell: Option<Shell>,
    cards: Option<Cards>,
    /// Keeps a toast fully shown every frame (a toast fades out by itself, and
    /// the harness runs until its repaints stop, so a one-off toast is gone).
    pin_toast: Option<String>,
}

/// A pointer action at the centre of the first painted text that equals
/// the needle, moved by `offset`.
struct Poke {
    click: bool,
    /// Mouse wheel points to scroll there (0: none; negative scrolls down).
    wheel: f32,
    needle: Rc<dyn Fn(Lang) -> String>,
    offset: Vec2,
}

struct Scene {
    area: &'static str,
    name: String,
    size: Vec2,
    frames: usize,
    make: Rc<dyn Fn(Lang) -> World>,
    pokes: Vec<Poke>,
}

impl Scene {
    fn new(area: &'static str, name: impl Into<String>, size: Vec2, make: impl Fn(Lang) -> World + 'static) -> Self {
        Self { area, name: name.into(), size, frames: FRAMES, make: Rc::new(make), pokes: Vec::new() }
    }

    /// Move the pointer over a text (the picture keeps the cursor).
    fn hover(mut self, needle: impl Fn(Lang) -> String + 'static, offset: Vec2) -> Self {
        self.pokes.push(Poke { click: false, wheel: 0.0, needle: Rc::new(needle), offset });
        self
    }

    /// Click a text.
    fn click(mut self, needle: impl Fn(Lang) -> String + 'static) -> Self {
        self.pokes.push(Poke { click: true, wheel: 0.0, needle: Rc::new(needle), offset: Vec2::ZERO });
        self
    }

    /// Scroll the mouse wheel by `dy` points over a text.
    fn wheel(mut self, needle: impl Fn(Lang) -> String + 'static, dy: f32) -> Self {
        self.pokes.push(Poke { click: false, wheel: dy, needle: Rc::new(needle), offset: Vec2::ZERO });
        self
    }

    fn hovers(&self) -> bool {
        self.pokes.iter().any(|p| !p.click && p.wheel == 0.0)
    }

    fn frames(mut self, n: usize) -> Self {
        self.frames = n;
        self
    }

    fn file(&self) -> String {
        format!("{}-{}", self.area, self.name)
    }
}

// ---- worlds ----

const HOME: Vec2 = Vec2 { x: crate::model::HOME_WIDTH, y: 735.0 };
const ONBOARD: Vec2 = Vec2 { x: crate::onboarding::WINDOW.0, y: crate::onboarding::WINDOW.1 };
/// Harness size for card scenes; the picture is cropped to the card window.
const CARD: Vec2 = Vec2 { x: 640.0, y: 720.0 };
/// Where a card window is drawn inside the harness. Origin (0,0): the quote card paints at absolute GUTTER coordinates, as in its own native window.
const CARD_AT: Pos2 = Pos2 { x: 0.0, y: 0.0 };

thread_local! {
    /// Rect of the last card window drawn by [`card_viewports`].
    static CARD_RECT: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
}

/// Draw immediate viewports (the cards) in the main viewport at their own
/// inner size, at [`CARD_AT`], without the frame an embedded `Window`
/// would add. The real app gives each card a native window of that size.
fn card_viewports() {
    egui::Context::set_immediate_viewport_renderer(|ctx, mut vp| {
        let size = vp.builder.inner_size.unwrap_or(vec2(300.0, 200.0));
        let rect = Rect::from_min_size(CARD_AT, size);
        CARD_RECT.with(|c| c.set(Some(rect)));
        egui::Area::new(egui::Id::unique(("fp_shot_viewport", vp.ids.this)))
            .order(egui::Order::Middle)
            .fixed_pos(CARD_AT)
            .show(ctx, |ui| {
                ui.set_clip_rect(rect);
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    ui.set_min_size(size);
                    (vp.viewport_ui_cb)(ui);
                });
            });
    });
}

/// A folder holding an empty file for every wallpaper pack (main picture
/// and dawn still, so every pack shows its day-and-night pill).
fn walls_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fp-ui-shots-walls-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    for e in crate::wallpapers::ORDER {
        let id = e.wallpaper_id();
        let _ = std::fs::write(dir.join(format!("{id}.jpg")), b"");
        let _ = std::fs::write(dir.join(fp_sources::daynight::file_name(id, fp_sources::daynight::Slot::Dawn)), b"");
    }
    dir
}

fn host(walls: bool) -> HostInfo {
    HostInfo {
        version: crate::changelog::latest().to_string(),
        has_battery: true,
        displays: vec![
            DisplayInfo { id: "\\\\.\\DISPLAY1".to_string(), name: "DELL U2720Q".to_string() },
            DisplayInfo { id: "\\\\.\\DISPLAY2".to_string(), name: String::new() },
        ],
        wallpapers_dir: if walls { walls_dir() } else { PathBuf::new() },
        day_pack: None,
        login_on: true,
        seen_changelog: Some(crate::changelog::latest().to_string()),
        custom_chrome: true,
        decoder: None,
        work_areas: vec![Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1040.0))],
        ..HostInfo::default()
    }
}

fn settings(lang: Lang, seen_welcome: bool) -> Settings {
    Settings { seen_welcome, language: Some(lang), ..Settings::default() }
}

fn home_world(lang: Lang, tab: Tab, edit: impl FnOnce(&mut Shell, &mut Settings)) -> World {
    let mut shell = Shell::new(host(true));
    shell.show_tab(tab);
    let mut s = settings(lang, true);
    edit(&mut shell, &mut s);
    World { lang, weathers: Weathers::new(s, false), shell: Some(shell), cards: None, pin_toast: None }
}

fn onboarding_world(lang: Lang, step: usize) -> World {
    let mut shell = Shell::new(host(true));
    shell.step = step;
    World {
        lang,
        weathers: Weathers::new(settings(lang, false), false),
        shell: Some(shell),
        cards: None,
        pin_toast: None,
    }
}

fn layout() -> Layout {
    Layout::new(vec![MonitorInfo {
        id: "shots".into(),
        name: "Shots".into(),
        pixels: PixelRect { left: 0, top: 0, right: 1920, bottom: 1080 },
        work: PixelRect { left: 0, top: 0, right: 1920, bottom: 1040 },
        scale: 1.0,
        primary: true,
    }])
}

fn card_world(lang: Lang, open: impl FnOnce(&mut Cards, &CardCtx<'_>)) -> World {
    let l = layout();
    let cx = CardCtx {
        lang,
        layout: &l,
        sprites: &[],
        cursor: GVec2::new(900.0, 500.0),
        tree_origin: Some(GVec2::new(700.0, 300.0)),
    };
    let mut cards = Cards::new(7);
    open(&mut cards, &cx);
    World {
        lang,
        weathers: Weathers::new(settings(lang, true), false),
        shell: None,
        cards: Some(cards),
        pin_toast: None,
    }
}

fn city(name: &str, admin: &str, country: &str) -> City {
    City { name: name.into(), admin: admin.into(), country: country.into(), latitude: 52.5, longitude: 13.4 }
}

/// Put the city search into `state` the way the host replies would.
fn city_state(shell: &mut Shell, reply: Option<Result<Vec<City>, ()>>) {
    shell.city.query = "Berlin".to_string();
    let q = shell.city.begin().expect("query long enough");
    if let Some(r) = reply {
        shell.city_results(&q, r);
    }
}

// ---- scenes ----

fn effect_slug(e: EffectKind) -> String {
    format!("{e:?}").to_lowercase()
}

fn scenes() -> Vec<Scene> {
    let mut v = Vec::new();

    // Effects tab (ui 3.1): nothing on, each effect on, snow and rain variants.
    v.push(Scene::new("effects", "none", HOME, |l| home_world(l, Tab::Effects, |_, _| {})));
    for e in EffectKind::ALL {
        v.push(Scene::new("effects", effect_slug(e), HOME, move |l| {
            home_world(l, Tab::Effects, |_, s| s.weather = Some(e))
        }));
    }
    v.push(Scene::new("effects", "snow-heavy", HOME, |l| {
        home_world(l, Tab::Effects, |_, s| {
            s.weather = Some(EffectKind::Snow);
            s.heavy_snow = true;
        })
    }));
    v.push(Scene::new("effects", "rain-no-lightning", HOME, |l| {
        home_world(l, Tab::Effects, |_, s| {
            s.weather = Some(EffectKind::Rain);
            s.lightning = false;
        })
    }));
    // Hover: the heavy snow segment, the snow tile, a tab, the update button.
    v.push(
        Scene::new("effects", "hover-heavy-snow", HOME, |l| home_world(l, Tab::Effects, |_, _| {}))
            .hover(|l| fp_text::t(l, Key::HeavySnow).to_string(), Vec2::ZERO),
    );
    v.push(
        Scene::new("effects", "hover-snow-tile", HOME, |l| home_world(l, Tab::Effects, |_, _| {}))
            .hover(|l| fp_text::effect(l, EffectKind::Snow).to_string(), vec2(60.0, -110.0)),
    );
    v.push(
        Scene::new("effects", "hover-tab", HOME, |l| home_world(l, Tab::Effects, |_, _| {}))
            .hover(|l| fp_text::t(l, Tab::Wallpapers.title()).to_string(), Vec2::ZERO),
    );
    v.push(
        Scene::new("effects", "hover-check-updates", HOME, |l| home_world(l, Tab::Effects, |_, _| {}))
            .hover(|l| fp_text::t(l, Key::SetCheckUpdates).to_string(), Vec2::ZERO),
    );

    // Header update line states (ui 2.3) and the changelog badge.
    let updates: [(&str, UpdateLine); 4] = [
        ("checking", UpdateLine::Checking),
        ("up-to-date", UpdateLine::UpToDate { at: 0.0 }),
        ("offline", UpdateLine::Offline { at: 0.0 }),
        ("available", UpdateLine::Available { version: "9.9.9".into(), url: "https://example.org/".into() }),
    ];
    for (name, line) in updates {
        v.push(Scene::new("update", name, HOME, move |l| {
            let line = line.clone();
            home_world(l, Tab::Effects, move |sh, _| sh.update_result(line))
        }));
    }
    v.push(Scene::new("header", "changelog-badge", HOME, |l| {
        let mut w = home_world(l, Tab::Effects, |_, _| {});
        if let Some(sh) = &mut w.shell {
            sh.badge = true;
        }
        w
    }));

    // Platform notices (porting design 7).
    let notes: [(&str, Notices); 3] = [
        ("notice-no-compositor", Notices { no_overlay: true, no_compositor: true, ..Notices::default() }),
        (
            "notice-wayland-ground-only",
            Notices { no_window_geometry: true, no_global_cursor: true, ..Notices::default() },
        ),
        ("notice-xwayland", Notices { partial_windows: true, ..Notices::default() }),
    ];
    for (name, n) in notes {
        v.push(Scene::new("effects", name, HOME, move |l| {
            let mut w = home_world(l, Tab::Effects, |_, _| {});
            if let Some(sh) = &mut w.shell {
                sh.set_host(HostInfo { notices: n, ..host(true) });
            }
            w
        }));
    }

    // Wallpapers tab (ui 3.2).
    v.push(Scene::new("wallpapers", "packs", HOME, |l| home_world(l, Tab::Wallpapers, |_, _| {})));
    v.push(Scene::new("wallpapers", "unsupported", HOME, |l| {
        let mut w = home_world(l, Tab::Wallpapers, |_, _| {});
        if let Some(sh) = &mut w.shell {
            sh.set_host(HostInfo { wallpaper_supported: false, ..host(true) });
        }
        w
    }));
    v.push(Scene::new("wallpapers", "picked-cycle", HOME, |l| {
        home_world(l, Tab::Wallpapers, |sh, _| {
            sh.picked.set(crate::wallpapers::cycle_id(EffectKind::Snow.wallpaper_id()))
        })
    }));
    v.push(Scene::new("wallpapers", "empty", HOME, |l| {
        let mut w = home_world(l, Tab::Wallpapers, |_, _| {});
        if let Some(sh) = &mut w.shell {
            sh.set_host(host(false));
        }
        w
    }));
    v.push(
        Scene::new("wallpapers", "time-card", HOME, |l| home_world(l, Tab::Wallpapers, |_, _| {}))
            .click(|l| fp_text::t(l, Key::DayNightLabel).to_string()),
    );
    v.push(
        Scene::new("wallpapers", "hover-day-pill", HOME, |l| home_world(l, Tab::Wallpapers, |_, _| {}))
            .hover(|l| fp_text::t(l, Key::DayNightLabel).to_string(), Vec2::ZERO),
    );

    // How-to and changelog.
    v.push(Scene::new("howto", "page", HOME, |l| home_world(l, Tab::HowTo, |_, _| {})));
    v.push(Scene::new("changelog", "page", HOME, |l| home_world(l, Tab::Changelog, |_, _| {})));

    // Settings panes (ui 3.5).
    let panes = [("general", Pane::General), ("screens", Pane::Screens), ("about", Pane::About), ("help", Pane::Help)];
    for (name, p) in panes {
        v.push(Scene::new("settings", name, HOME, move |l| home_world(l, Tab::Settings, |sh, _| sh.show_pane(p))));
    }
    // The end of the General pane: the automatic-updates switch.
    v.push(
        Scene::new("settings", "general-bottom", HOME, |l| home_world(l, Tab::Settings, |_, s| s.auto_update = true))
            .wheel(|l| fp_text::t(l, Key::SetSnowballs).to_string(), -3000.0),
    );
    v.push(
        Scene::new("settings", "general-language-popup", HOME, |l| home_world(l, Tab::Settings, |_, _| {}))
            .click(|l| l.native_name().to_string()),
    );
    v.push(Scene::new("settings", "general-login-blocked", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| sh.login_failed(true))
    }));
    v.push(Scene::new("settings", "general-login-failed", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| {
            sh.login_failed(false);
            debug_assert_eq!(sh.login_note, Some(LoginNote::Failed));
        })
    }));
    v.push(Scene::new("settings", "general-match-needs-city", HOME, |l| {
        home_world(l, Tab::Settings, |_, s| s.match_weather = true)
    }));
    v.push(Scene::new("settings", "city-searching", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| city_state(sh, None))
    }));
    v.push(Scene::new("settings", "city-results", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| {
            city_state(
                sh,
                Some(Ok(vec![
                    city("Berlin", "Land Berlin", "Germany"),
                    city("Berlin", "New Hampshire", "United States"),
                    city("Berlin", "Wisconsin", "United States"),
                ])),
            )
        })
    }));
    v.push(Scene::new("settings", "city-no-match", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| city_state(sh, Some(Ok(Vec::new()))))
    }));
    v.push(Scene::new("settings", "city-offline", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| {
            city_state(sh, Some(Err(())));
            debug_assert_eq!(sh.city.state, CityState::Offline);
        })
    }));
    v.push(Scene::new("settings", "city-chosen", HOME, |l| {
        home_world(l, Tab::Settings, |_, s| s.city = Some(city("Tokyo", "Tokyo", "Japan")))
    }));
    v.push(
        Scene::new("settings", "hover-nav", HOME, |l| home_world(l, Tab::Settings, |_, _| {}))
            .hover(|l| fp_text::t(l, Pane::About.title()).to_string(), Vec2::ZERO),
    );

    // Toast (ui 3.7) and sheet (ui 3.8).
    v.push(Scene::new("overlay", "toast", HOME, |l| {
        let mut w = home_world(l, Tab::Wallpapers, |_, _| {});
        w.pin_toast = Some(fp_text::t(l, Key::WallpaperSet).to_string());
        w
    }));
    v.push(Scene::new("overlay", "sheet", HOME, |l| {
        home_world(l, Tab::Settings, |sh, _| {
            sh.show_pane(Pane::Help);
            sh.confirm(
                fp_text::t(l, Key::SetQuit),
                fp_text::t(l, Key::SetQuitDetail),
                fp_text::t(l, Key::SetQuitButton),
                UiAction::Quit,
            );
        })
    }));

    // Onboarding (ui 3.6): every step.
    for step in 0..=crate::onboarding::DONE {
        let name = match step {
            crate::onboarding::LANGUAGE => "0-language".to_string(),
            crate::onboarding::WELCOME => "1-welcome".to_string(),
            crate::onboarding::WALLPAPERS => format!("{step}-wallpapers"),
            crate::onboarding::DONE => format!("{step}-done"),
            s => format!("{s}-{}", crate::onboarding::effect_of(s).map(effect_slug).unwrap_or_default()),
        };
        v.push(Scene::new("onboarding", name, ONBOARD, move |l| onboarding_world(l, step)));
    }
    v.push(
        Scene::new("onboarding", "hover-next", ONBOARD, |l| onboarding_world(l, crate::onboarding::WELCOME))
            .hover(|l| fp_text::t(l, Key::ObStart).to_string(), Vec2::ZERO),
    );

    // Cards.
    let tabs = [("hat", Key::TabHat), ("face", Key::TabFace), ("body", Key::TabBody), ("clothes", Key::TabClothes)];
    for (name, key) in tabs {
        let mut s = Scene::new("card", format!("dress-{name}"), CARD, |l| {
            card_world(l, |c, cx| {
                c.open(
                    CardRequest::DressUp {
                        bottom: 1,
                        look: Some(fp_snowman::Look {
                            hat: Some(fp_snowman::Hat::Cowboy),
                            ..fp_snowman::Look::classic()
                        }),
                    },
                    cx,
                    0.0,
                )
            })
        });
        if key != Key::TabHat {
            s = s.click(move |l| fp_text::t(l, key).to_string());
        }
        v.push(s);
    }
    v.push(Scene::new("card", "tree-style", CARD, |l| {
        card_world(l, |c, cx| {
            c.open(CardRequest::TreeStyle { tree: 3, look: fp_trees::TreeLook::default(), quip: None }, cx, 0.0)
        })
    }));
    v.push(Scene::new("card", "tree-refuse", CARD, |l| {
        card_world(l, |c, cx| {
            c.open(CardRequest::TreeStyle { tree: 3, look: fp_trees::TreeLook::default(), quip: Some(1) }, cx, 0.0)
        })
    }));
    v.push(Scene::new("card", "quote", CARD, |l| {
        card_world(l, |c, cx| c.open(CardRequest::Quote { at: GVec2::new(900.0, 600.0), quote: 0 }, cx, 0.0))
    }));
    v.push(Scene::new("card", "quote-by", CARD, |l| {
        // Shortest quote with an author line: attribution on a minimal card.
        let with_by = fp_text::quotes(l)
            .iter()
            .enumerate()
            .filter(|(_, q)| q.by.is_some())
            .min_by_key(|(_, q)| q.text.chars().count())
            .map_or(0, |(i, _)| i);
        card_world(l, move |c, cx| c.open(CardRequest::Quote { at: GVec2::new(900.0, 600.0), quote: with_by }, cx, 0.0))
    }));
    v.push(Scene::new("card", "quote-longest", CARD, |l| {
        // Longest quote that also carries an author line: covers wrapping plus
        // attribution and the tallest card height (falls back to the longest overall).
        let quotes = fp_text::quotes(l);
        let pick = |need_by: bool| {
            quotes
                .iter()
                .enumerate()
                .filter(|(_, q)| !need_by || q.by.is_some())
                .max_by_key(|(_, q)| q.text.chars().count())
                .map(|(i, _)| i)
        };
        let longest = pick(true).or_else(|| pick(false)).unwrap_or(0);
        card_world(l, move |c, cx| c.open(CardRequest::Quote { at: GVec2::new(900.0, 600.0), quote: longest }, cx, 0.0))
    }));
    for n in [1u64, 4] {
        v.push(Scene::new("card", format!("station-{n}"), CARD, move |l| {
            card_world(l, |c, cx| {
                let choices = (0..n)
                    .map(|i| {
                        let p = if i % 2 == 0 {
                            fp_characters::Pick::Snowman(i + 1)
                        } else {
                            fp_characters::Pick::Tree(i + 1)
                        };
                        (p, GVec2::new(800.0 + 40.0 * i as f64, 200.0))
                    })
                    .collect();
                c.open(CardRequest::TrainStation { at: GVec2::new(900.0, 200.0), choices }, cx, 0.0)
            })
        }));
    }
    v.push(
        Scene::new("card", "limit-snowman", CARD, |l| {
            card_world(l, |c, cx| {
                c.open(CardRequest::LimitReached { screen: Frame::new(0.0, 0.0, 1920.0, 1040.0) }, cx, 0.0)
            })
        })
        .frames(44),
    );
    for (name, count, confirm) in
        [("empty", 0usize, false), ("some", 7, false), ("full", 100, false), ("confirm", 7, true)]
    {
        let mut s = Scene::new("card", format!("decor-picker-{name}"), CARD, move |l| {
            card_world(l, |c, cx| c.toggle_picker(cx, count, 100, 0.0))
        });
        if confirm {
            s = s.click(|l| fp_text::t(l, Key::DecorTakeAllDown).to_string());
        }
        v.push(s);
    }
    v.push(Scene::new("card", "update", CARD, |l| {
        card_world(l, |c, cx| c.open_update("9.9.9", "https://example.org/release", cx, 0.0))
    }));
    // The in-app install: progress, restart, failure, and a copy that can
    // only point at the download page.
    let phases: [(&str, UpdatePhase, Option<ManualReason>); 5] = [
        ("downloading", UpdatePhase::Downloading { done: 4_200_000, total: 7_000_000 }, None),
        ("restarting", UpdatePhase::Restarting, None),
        ("failed", UpdatePhase::Failed(UpdateFail::Verify), None),
        ("manual-package", UpdatePhase::Offer, Some(ManualReason::Package)),
        ("manual-failed", UpdatePhase::Failed(UpdateFail::Network), Some(ManualReason::Folder)),
    ];
    for (name, phase, manual) in phases {
        v.push(Scene::new("card", format!("update-{name}"), CARD, move |l| {
            card_world(l, |c, cx| {
                c.set_update_manual(manual);
                c.set_update_phase(phase);
                c.open_update("9.9.9", "https://example.org/release", cx, 0.0);
            })
        }));
    }
    v.push(Scene::new("card", "decor-menu", CARD, |l| {
        card_world(l, |c, cx| c.open_menu(3, fp_text::t(l, Key::DecorTakeDown), cx, 0.0))
    }));
    v.push(Scene::new("card", "tray-pointer", CARD, |l| {
        card_world(l, |c, cx| {
            assert!(c.open_pointer([1700.0, 1048.0, 24.0, 24.0], cx, 0.0), "icon on the taskbar");
        })
    }));
    v
}

// ---- running ----

/// One frame of a scene.
fn frame(ui: &mut egui::Ui, w: &mut World) {
    if w.shell.is_none() {
        // Cards are transparent: draw a plain desktop under them.
        crate::art::gradient(
            ui.painter(),
            ui.max_rect(),
            Color32::from_rgb(58, 82, 112),
            Color32::from_rgb(24, 34, 52),
        );
    }
    if let Some(sh) = &mut w.shell {
        // The harness wraps the app in an 8 pt margin; the window has none.
        let full = ui.ctx().content_rect();
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(full));
        ui.set_clip_rect(full);
        if let Some(text) = &w.pin_toast {
            // Started a second ago: the slide-in is done, the 2 s hold has time left.
            sh.toast(text, ui.ctx().input(|i| i.time) - 1.0);
        }
        for a in sh.ui(&mut ui, &w.weathers, w.lang, WallTime::new(WALL_UNIX, 0)) {
            if let UiAction::SetWeather(x) = a {
                w.weathers.set_weather(x);
            }
        }
    }
    if let Some(c) = &mut w.cards {
        let ctx = ui.ctx().clone();
        let _ = c.show(&ctx, w.lang);
    }
}

/// Texts painted this frame with their screen rects.
fn texts(out: &egui::FullOutput) -> Vec<(String, Rect)> {
    fn walk(s: &Shape, out: &mut Vec<(String, Rect)>) {
        match s {
            Shape::Text(t) => out.push((t.galley.text().to_string(), t.galley.rect.translate(t.pos.to_vec2()))),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut v = Vec::new();
    for c in &out.shapes {
        walk(&c.shape, &mut v);
    }
    v
}

/// egui's id clash warnings ("🔥 First use of widget ID 1A2B", ...).
fn clashes(out: &egui::FullOutput) -> Vec<String> {
    texts(out).into_iter().map(|(t, _)| t).filter(|t| is_clash_text(t)).collect()
}

fn is_clash_text(t: &str) -> bool {
    ["Double use of ", "First use of ", "Second use of "].iter().any(|p| t.contains(p)) && t.contains(" ID ")
}

fn harness(scene: &Scene, lang: Lang, gpu: bool) -> Harness<'static, World> {
    let code = lang.code();
    let b = Harness::<World>::builder()
        .with_size(scene.size)
        .with_step_dt(DT)
        .with_max_steps(200)
        .with_accessibility_check(false)
        .with_render_cursor(scene.hovers())
        .with_os(egui::os::OperatingSystem::Windows)
        .with_context_setup(move |ctx| {
            crate::install(ctx);
            ctx.options_mut(|o| o.warn_on_id_clash = true);
            // eframe's default `system_fonts` fallback (CJK glyphs).
            ctx.add_font_provider(Arc::new(
                egui_system_font_provider::SystemFontProvider::new().with_locale(Some(code)),
            ));
            // Cards go through `card_viewports`, like separate windows.
            ctx.set_embed_viewports(false);
        });
    card_viewports();
    CARD_RECT.with(|c| c.set(None));
    let b = if gpu { b.wgpu() } else { b };
    b.build_ui_state(frame, (scene.make)(lang))
}

/// Run a scene; returns the harness and the clash texts it produced.
fn run(scene: &Scene, lang: Lang, gpu: bool) -> (Harness<'static, World>, BTreeSet<String>) {
    let mut h = harness(scene, lang, gpu);
    let mut found = BTreeSet::new();
    // The gate only needs the layout to settle; pictures need animations done.
    let frames = if gpu { scene.frames } else { scene.frames.min(GATE_FRAMES) };
    for _ in 0..frames {
        h.step();
        found.extend(clashes(h.output()));
    }
    for poke in &scene.pokes {
        let needle = (poke.needle)(lang);
        let at = texts(h.output()).into_iter().find(|(t, _)| *t == needle).map(|(_, r)| r.center() + poke.offset);
        let Some(pos) = at else {
            found.insert(format!("(harness) target {needle:?} not found"));
            continue;
        };
        h.hover_at(pos);
        if poke.wheel != 0.0 {
            h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, poke.wheel),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
            h.event(egui::Event::PointerGone);
        }
        if poke.click {
            h.step();
            let press = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            h.event(press(true));
            h.step();
            h.event(press(false));
            h.step();
            if !scene.hovers() {
                h.event(egui::Event::PointerGone);
            }
        }
        for _ in 0..HOVER_FRAMES {
            h.step();
            found.extend(clashes(h.output()));
        }
    }
    (h, found)
}

fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_string()))
        .unwrap_or_default()
}

/// Always on: no scene may report a widget id clash (English, no GPU).
#[test]
fn id_clash_gate() {
    let mut bad = Vec::new();
    for scene in scenes() {
        let (_, found) = run(&scene, Lang::En, false);
        for c in found {
            bad.push(format!("{}: {c}", scene.file()));
        }
    }
    assert!(bad.is_empty(), "egui id clashes:\n{}", bad.join("\n"));
}

/// The clash detector sees an unsalted two-way pill pair, and the salted
/// pair (the effects grid shape: sibling `vertical` children) is clean.
#[test]
fn two_ways_clash_detected_and_fixed() {
    let pair = |salts: [u8; 2]| {
        let mut h = Harness::builder().with_accessibility_check(false).allow_missing_glyphs().build_ui(move |ui| {
            ui.ctx().options_mut(|o| o.warn_on_id_clash = true);
            let tk = crate::theme::Tokens::new(true);
            ui.horizontal(|ui| {
                for salt in salts {
                    ui.vertical(|ui| {
                        crate::widgets::two_ways(ui, &tk, salt, ["a", "b"], false);
                    });
                }
            });
        });
        h.step();
        clashes(h.output()).len()
    };
    assert!(pair([0, 0]) > 0, "detector must see a shared id");
    assert_eq!(pair([0, 1]), 0);
}

/// `FROSTPANE_SHOTS=1`: render every scene to PNG (see the module docs).
#[test]
fn ui_shots() {
    if std::env::var("FROSTPANE_SHOTS").ok().as_deref() != Some("1") {
        eprintln!("ui_shots: skipped (set FROSTPANE_SHOTS=1)");
        return;
    }
    let root = std::env::var_os("FROSTPANE_SHOTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-shots"));
    let langs: Vec<Lang> = match std::env::var("FROSTPANE_SHOTS_LANGS") {
        Ok(s) => s.split(',').filter_map(|c| Lang::parse(c.trim())).collect(),
        Err(_) => SHOT_LANGS.to_vec(),
    };
    let filter = std::env::var("FROSTPANE_SHOTS_FILTER").unwrap_or_default();
    let mut clash_lines = Vec::new();
    let mut problems = Vec::new();
    let mut written = 0usize;
    for lang in langs {
        let dir = root.join(lang.code());
        std::fs::create_dir_all(&dir).expect("shot folder");
        for scene in scenes().into_iter().filter(|s| s.file().contains(&filter)) {
            let path = dir.join(format!("{}.png", scene.file()));
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let (mut h, found) = run(&scene, lang, true);
                let mut img = h.render().map_err(|e| format!("render: {e}"))?;
                if let Some(r) = CARD_RECT.with(std::cell::Cell::get) {
                    let ppp = h.ctx.pixels_per_point();
                    let (x, y) = ((r.left() * ppp) as u32, (r.top() * ppp) as u32);
                    let (w, hh) = ((r.width() * ppp) as u32, (r.height() * ppp) as u32);
                    img = image::imageops::crop_imm(&img, x, y, w, hh).to_image();
                }
                img.save(&path).map_err(|e| format!("save {}: {e}", path.display()))?;
                Ok::<_, String>(found)
            }));
            match result {
                Ok(Ok(found)) => {
                    written += 1;
                    for c in found {
                        clash_lines.push(format!("{}/{}: {c}", lang.code(), scene.file()));
                    }
                }
                Ok(Err(e)) => problems.push(format!("{}/{}: {e}", lang.code(), scene.file())),
                Err(p) => problems.push(format!("{}/{}: panic: {}", lang.code(), scene.file(), panic_text(&*p))),
            }
        }
    }
    let write = |name: &str, lines: &[String]| {
        let body = if lines.is_empty() { "none\n".to_string() } else { lines.join("\n") + "\n" };
        std::fs::write(root.join(name), body).expect("report file");
    };
    write("clashes.txt", &clash_lines);
    write("problems.txt", &problems);
    eprintln!("ui_shots: {written} PNGs under {}", root.display());
    assert!(clash_lines.is_empty(), "id clashes (see clashes.txt):\n{}", clash_lines.join("\n"));
    assert!(problems.is_empty(), "problems (see problems.txt):\n{}", problems.join("\n"));
}
