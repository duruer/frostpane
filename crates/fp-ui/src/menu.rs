//! Tray menu layout (core-app 2.6) as plain data, so it can be tested
//! without a tray. `tray::Tray` turns a plan into muda items.

use crate::TrayAction;
use fp_core::effect::{EffectKind, Lang};
use fp_core::settings::Weathers;
use fp_core::wind::Wind;
use fp_paint::{IconName, menu as tint};
use fp_text::Key;

/// Icon of a menu entry: a named icon with a tint, or a wind icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuIcon {
    Named(IconName, u32),
    Wind(Wind, u32),
}

/// One leaf entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub icon: Option<MenuIcon>,
    /// `None` = no click action of its own (the primary snow/rain items).
    pub action: Option<TrayAction>,
    pub enabled: bool,
    /// Shows `Ctrl+Q` next to the label.
    pub ctrl_q: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Item(Item),
    Submenu { label: String, icon: Option<MenuIcon>, items: Vec<Entry> },
    Separator,
}

/// Cyan of the snowmen item, purple of the yeti, green of the train.
const SNOWMEN_TINT: u32 = 0x1AA8C2;
const YETI_TINT: u32 = 0x8C5CEB;
const PLOUGH_TINT: u32 = 0xF28C26;
const SANTA_TINT: u32 = 0xD92633;
const TRAIN_TINT: u32 = 0x29A85C;
const WIND_TINT: u32 = 0x73B8D9;
const OUTSIDE_TINT: u32 = 0xF2AD1A;
const RANDOM_TINT: u32 = 0x9966F2;
const SEASON_TINT: u32 = 0xF273A6;
const BAUBLE_TINT: u32 = 0xD92633;

pub fn icon_for(e: EffectKind) -> IconName {
    match e {
        EffectKind::Snow => IconName::Snow,
        EffectKind::Rain => IconName::Rain,
        EffectKind::Sakura => IconName::Sakura,
        EffectKind::Leaves => IconName::Leaves,
        EffectKind::Fireflies => IconName::Fireflies,
        EffectKind::Starfall => IconName::Starfall,
    }
}

fn item(label: impl Into<String>, icon: Option<MenuIcon>, action: Option<TrayAction>) -> Entry {
    Entry::Item(Item { label: label.into(), icon, action, enabled: true, ctrl_q: false })
}

fn label(lang: Lang, text: &str, on: bool) -> String {
    if on { fp_text::checked(lang, text) } else { text.to_string() }
}

/// Click variants of one effect (core-app 2.6 "Variants"): snow and rain have
/// two entries, the others one.
fn variants(w: &Weathers, lang: Lang, e: EffectKind) -> Vec<Entry> {
    let s = &w.settings;
    let on = s.weather == Some(e);
    let icon = |tintc| Some(MenuIcon::Named(icon_for(e), tintc));
    match e {
        EffectKind::Snow => vec![
            item(
                label(lang, fp_text::effect(lang, e), on && !s.heavy_snow),
                icon(tint::SNOW),
                Some(TrayAction::SnowVariant { heavy: false }),
            ),
            item(
                label(lang, fp_text::t(lang, Key::HeavySnow), on && s.heavy_snow),
                icon(tint::HEAVY_BLUE),
                Some(TrayAction::SnowVariant { heavy: true }),
            ),
        ],
        EffectKind::Rain => vec![
            item(
                label(lang, fp_text::effect(lang, e), on && !s.lightning),
                icon(tint::RAIN),
                Some(TrayAction::RainVariant { storm: false }),
            ),
            item(
                label(lang, fp_text::t(lang, Key::Thunderstorm), on && s.lightning),
                Some(MenuIcon::Named(IconName::Storm, tint::RAIN)),
                Some(TrayAction::RainVariant { storm: true }),
            ),
        ],
        _ => vec![item(label(lang, fp_text::effect(lang, e), on), icon(e.menu_color()), Some(TrayAction::Toggle(e)))],
    }
}

/// The primary weather entry for `W = weather ?? snow`.
fn primary(w: &Weathers, lang: Lang) -> Entry {
    let s = &w.settings;
    let e = s.weather.unwrap_or(EffectKind::Snow);
    match e {
        EffectKind::Snow => {
            let on = s.weather == Some(EffectKind::Snow);
            let (text, tintc) = if on && s.heavy_snow {
                (label(lang, fp_text::t(lang, Key::HeavySnow), true), tint::HEAVY_BLUE)
            } else {
                (label(lang, fp_text::effect(lang, e), on), tint::SNOW)
            };
            Entry::Submenu {
                label: text,
                icon: Some(MenuIcon::Named(IconName::Snow, tintc)),
                items: variants(w, lang, e),
            }
        }
        EffectKind::Rain => {
            let name = if s.lightning { fp_text::t(lang, Key::Thunderstorm) } else { fp_text::effect(lang, e) };
            Entry::Submenu {
                label: label(lang, name, true),
                icon: Some(MenuIcon::Named(IconName::Rain, tint::RAIN)),
                items: variants(w, lang, e),
            }
        }
        _ => item(
            label(lang, fp_text::effect(lang, e), true),
            Some(MenuIcon::Named(icon_for(e), e.menu_color())),
            Some(TrayAction::Toggle(e)),
        ),
    }
}

/// The whole menu, top to bottom (core-app 2.6 items 1-12).
pub fn plan(w: &Weathers, lang: Lang) -> Vec<Entry> {
    let s = &w.settings;
    let t = |k| fp_text::t(lang, k);
    let named = |n, c| Some(MenuIcon::Named(n, c));
    let mut out = vec![primary(w, lang)];

    if w.shown_weather() == Some(EffectKind::Snow) {
        out.push(Entry::Separator);
        out.push(item(t(Key::TraySendPlough), named(IconName::Plough, PLOUGH_TINT), Some(TrayAction::SendPlough)));
        out.push(item(t(Key::TraySendSnowmen), named(IconName::Snowman, SNOWMEN_TINT), Some(TrayAction::SendSnowmen)));
        out.push(item(t(Key::TraySendYeti), named(IconName::Yeti, YETI_TINT), Some(TrayAction::SendYeti)));
        out.push(item(t(Key::TraySendSanta), named(IconName::Santa, SANTA_TINT), Some(TrayAction::SendSanta)));
        out.push(item(t(Key::TraySendTrain), named(IconName::Train, TRAIN_TINT), Some(TrayAction::SendTrain)));
    }

    out.push(Entry::Separator);
    out.push(item(t(Key::TrayDecorate), named(IconName::Bauble, BAUBLE_TINT), Some(TrayAction::Decorate)));
    out.push(Entry::Separator);

    let current = s.weather.unwrap_or(EffectKind::Snow);
    let mut other = Vec::new();
    for e in EffectKind::ALL {
        if e != current {
            other.extend(variants(w, lang, e));
        }
    }
    out.push(Entry::Submenu {
        label: t(Key::TrayOtherWeather).to_string(),
        icon: named(IconName::Rain, tint::GRAY),
        items: other,
    });

    let winds = Wind::ALL
        .iter()
        .map(|&wv| {
            item(
                label(lang, fp_text::wind(lang, wv), s.wind == wv),
                Some(MenuIcon::Wind(wv, WIND_TINT)),
                Some(TrayAction::Wind(wv)),
            )
        })
        .collect();
    out.push(Entry::Submenu {
        label: t(Key::TrayWind).to_string(),
        icon: named(IconName::Wind, WIND_TINT),
        items: winds,
    });

    out.push(item(
        label(lang, t(Key::TrayMatchWeather), s.match_weather),
        named(IconName::Outside, OUTSIDE_TINT),
        Some(TrayAction::ToggleMatchWeather),
    ));
    out.push(item(
        label(lang, t(Key::TrayRandomWeather), s.random_weather),
        named(IconName::Random, RANDOM_TINT),
        Some(TrayAction::ToggleRandom),
    ));
    out.push(item(
        label(lang, t(Key::TrayMatchSeason), s.match_season),
        named(IconName::Sakura, SEASON_TINT),
        Some(TrayAction::ToggleSeason),
    ));
    out.push(Entry::Item(Item {
        label: t(Key::TrayOff).to_string(),
        icon: named(IconName::MinusCircle, tint::GRAY),
        action: Some(TrayAction::Off),
        enabled: w.weather_on(),
        ctrl_q: false,
    }));
    out.push(Entry::Separator);
    out.push(item(t(Key::TrayOpen), named(IconName::Home, tint::GRAY), Some(TrayAction::Open)));
    out.push(Entry::Separator);
    out.push(Entry::Item(Item {
        label: t(Key::TrayQuit).to_string(),
        icon: named(IconName::Logout, tint::GRAY),
        action: Some(TrayAction::Quit),
        enabled: true,
        ctrl_q: true,
    }));
    out
}

/// Tray icon name: `storm` when rain with lightning is shown, else the
/// shown (or manual) weather's name, `snow` when nothing is set.
pub fn tray_icon_name(w: &Weathers) -> IconName {
    let shown = w.shown_weather().or(w.settings.weather);
    match shown {
        Some(EffectKind::Rain) if w.lightning_shown() => IconName::Storm,
        Some(e) => icon_for(e),
        None => IconName::Snow,
    }
}

/// Icon colour: black on a light taskbar, white on a dark one.
pub fn tray_icon_color(light_taskbar: bool) -> u32 {
    if light_taskbar { 0x000000 } else { 0xFFFFFF }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_core::settings::Settings;

    fn weathers() -> Weathers {
        Weathers::new(Settings::default(), false)
    }

    fn leaves(entries: &[Entry]) -> Vec<&Item> {
        let mut v = Vec::new();
        for e in entries {
            match e {
                Entry::Item(i) => v.push(i),
                Entry::Submenu { items, .. } => v.extend(leaves(items)),
                Entry::Separator => {}
            }
        }
        v
    }

    #[test]
    fn default_menu_has_the_spec_shape() {
        let w = weathers();
        let p = plan(&w, Lang::En);
        // No visitors without snow: primary, sep, decorate, sep, other, wind,
        // 3 toggles, off, sep, open, sep, quit.
        assert_eq!(p.len(), 14);
        assert!(matches!(&p[0], Entry::Submenu { items, .. } if items.len() == 2));
        assert!(matches!(p[1], Entry::Separator));
        assert!(matches!(&p[2], Entry::Item(i) if i.action == Some(TrayAction::Decorate)));
        let off = leaves(&p).into_iter().find(|i| i.action == Some(TrayAction::Off)).unwrap();
        assert!(!off.enabled);
        let quit = leaves(&p).into_iter().find(|i| i.action == Some(TrayAction::Quit)).unwrap();
        assert!(quit.ctrl_q);
        assert!(matches!(p.last(), Some(Entry::Item(i)) if i.action == Some(TrayAction::Quit)));
    }

    #[test]
    fn other_weather_excludes_the_primary() {
        let w = weathers();
        let p = plan(&w, Lang::En);
        let Entry::Submenu { items, .. } = &p[4] else { panic!("other submenu") };
        // Rain (2) + sakura, leaves, fireflies, starfall.
        assert_eq!(items.len(), 6);
        assert!(!leaves(items).iter().any(|i| i.action == Some(TrayAction::SnowVariant { heavy: false })));
        // With sakura as the manual weather the snow variants move into "other".
        let mut w2 = weathers();
        w2.set_weather(Some(EffectKind::Sakura));
        let p2 = plan(&w2, Lang::En);
        let Entry::Submenu { items, .. } = &p2[4] else { panic!() };
        assert!(leaves(items).iter().any(|i| i.action == Some(TrayAction::SnowVariant { heavy: true })));
        assert!(!leaves(items).iter().any(|i| i.action == Some(TrayAction::Toggle(EffectKind::Sakura))));
        // The primary is the checked sakura entry with a click action.
        let Entry::Item(i) = &p2[0] else { panic!("plain primary") };
        assert_eq!(i.action, Some(TrayAction::Toggle(EffectKind::Sakura)));
        assert_eq!(i.label, fp_text::checked(Lang::En, fp_text::effect(Lang::En, EffectKind::Sakura)));
    }

    #[test]
    fn snow_adds_the_send_actions_and_checks_variants() {
        let mut w = weathers();
        w.tray_snow(true);
        let p = plan(&w, Lang::En);
        let acts: Vec<_> = leaves(&p).into_iter().filter_map(|i| i.action).collect();
        for a in [
            TrayAction::SendPlough,
            TrayAction::SendSnowmen,
            TrayAction::SendYeti,
            TrayAction::SendSanta,
            TrayAction::SendTrain,
        ] {
            assert!(acts.contains(&a), "{a:?}");
        }
        let Entry::Submenu { label, icon, items } = &p[0] else { panic!() };
        assert_eq!(*label, fp_text::checked(Lang::En, fp_text::t(Lang::En, Key::HeavySnow)));
        assert_eq!(*icon, Some(MenuIcon::Named(IconName::Snow, tint::HEAVY_BLUE)));
        let Entry::Item(normal) = &items[0] else { panic!() };
        let Entry::Item(heavy) = &items[1] else { panic!() };
        assert_ne!(normal.label, heavy.label);
        assert_eq!(heavy.label, fp_text::checked(Lang::En, fp_text::t(Lang::En, Key::HeavySnow)));
        assert_eq!(normal.label, fp_text::effect(Lang::En, EffectKind::Snow));
    }

    #[test]
    fn plain_snow_checks_the_normal_variant() {
        let mut w = weathers();
        w.tray_snow(false);
        let p = plan(&w, Lang::En);
        let Entry::Submenu { label, icon, items } = &p[0] else { panic!() };
        assert_eq!(*label, fp_text::checked(Lang::En, fp_text::effect(Lang::En, EffectKind::Snow)));
        assert_eq!(*icon, Some(MenuIcon::Named(IconName::Snow, tint::SNOW)));
        let Entry::Item(normal) = &items[0] else { panic!() };
        assert_eq!(normal.label, *label);
    }

    #[test]
    fn rain_primary_is_always_checked() {
        let mut w = weathers();
        w.tray_rain(true);
        let p = plan(&w, Lang::En);
        let Entry::Submenu { label, items, .. } = &p[0] else { panic!() };
        assert_eq!(*label, fp_text::checked(Lang::En, fp_text::t(Lang::En, Key::Thunderstorm)));
        assert_eq!(items.len(), 2);
        let mut w = weathers();
        w.tray_rain(false);
        let p = plan(&w, Lang::En);
        let Entry::Submenu { label, .. } = &p[0] else { panic!() };
        assert_eq!(*label, fp_text::checked(Lang::En, fp_text::effect(Lang::En, EffectKind::Rain)));
    }

    #[test]
    fn wind_submenu_checks_the_current_wind() {
        let mut w = weathers();
        w.settings.wind = Wind::StrongLeft;
        let p = plan(&w, Lang::En);
        let Entry::Submenu { items, .. } = &p[5] else { panic!("wind submenu") };
        assert_eq!(items.len(), 6);
        let checked: Vec<_> = leaves(items)
            .into_iter()
            .filter(|i| i.label == fp_text::checked(Lang::En, fp_text::wind(Lang::En, Wind::StrongLeft)))
            .collect();
        assert_eq!(checked.len(), 1);
        assert_eq!(checked[0].action, Some(TrayAction::Wind(Wind::StrongLeft)));
    }

    #[test]
    fn modes_check_their_toggles_and_enable_off() {
        let mut w = weathers();
        w.set_match_weather(true);
        let p = plan(&w, Lang::En);
        let m = leaves(&p).into_iter().find(|i| i.action == Some(TrayAction::ToggleMatchWeather)).unwrap();
        assert_eq!(m.label, fp_text::checked(Lang::En, fp_text::t(Lang::En, Key::TrayMatchWeather)));
        let off = leaves(&p).into_iter().find(|i| i.action == Some(TrayAction::Off)).unwrap();
        assert!(off.enabled);
        let r = leaves(&p).into_iter().find(|i| i.action == Some(TrayAction::ToggleRandom)).unwrap();
        assert_eq!(r.label, fp_text::t(Lang::En, Key::TrayRandomWeather));
    }

    #[test]
    fn tray_icon_follows_the_shown_weather() {
        let mut w = weathers();
        assert_eq!(tray_icon_name(&w), IconName::Snow);
        w.tray_rain(true);
        assert_eq!(tray_icon_name(&w), IconName::Storm);
        w.tray_rain(false);
        assert_eq!(tray_icon_name(&w), IconName::Rain);
        w.set_weather(Some(EffectKind::Leaves));
        assert_eq!(tray_icon_name(&w), IconName::Leaves);
        assert_eq!(tray_icon_color(true), 0);
        assert_eq!(tray_icon_color(false), 0xFFFFFF);
        for e in EffectKind::ALL {
            let _ = icon_for(e);
        }
    }
}
