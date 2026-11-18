//! Frostpane UI strings.
//!
//! Spec: core-app 2.16 (string groups and purposes), misc 3 (visitors),
//! ui 7 (localisation keys), snowballs 6, snowman 5, trees 3.9, decorations 9.
//! All copy is written for Frostpane; nothing is taken from the original
//! program. Nine languages: en, ja, es, de, fr, ko, ru, tr, kk.
//!
//! Every string lives in one table (`strings.rs`), one row per [`Key`] with
//! one column per language, so a missing translation cannot compile. Text with
//! numbers or names uses `{0}`, `{1}` placeholders filled by [`fmt`] and the
//! typed helpers below. Other crates never build user text themselves.

mod quotes;
mod strings;

use fp_core::effect::{EffectKind, Lang, Visitor};
use fp_core::wind::Wind;

pub use quotes::Quote;
pub use strings::Key;

fn col(lang: Lang) -> usize {
    match lang {
        Lang::En => 0,
        Lang::Ja => 1,
        Lang::Es => 2,
        Lang::De => 3,
        Lang::Fr => 4,
        Lang::Ko => 5,
        Lang::Ru => 6,
        Lang::Tr => 7,
        Lang::Kk => 8,
    }
}

/// Text for `k` in `lang`.
pub fn t(lang: Lang, k: Key) -> &'static str {
    strings::TABLE[k as usize][col(lang)]
}

/// Text for `k` with `{0}`, `{1}`, ... replaced by `args`.
///
/// One pass over the template: text inside an argument is never expanded
/// again, so an argument such as `"{1}"` stays as it is. A placeholder with
/// no matching argument is kept as written.
pub fn fmt(lang: Lang, k: Key, args: &[&str]) -> String {
    fill(t(lang, k), args)
}

fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let arg = tail
            .find('}')
            .and_then(|close| tail[1..close].parse::<usize>().ok().map(|i| (i, close)))
            .and_then(|(i, close)| args.get(i).map(|a| (*a, close)));
        match arg {
            Some((a, close)) => {
                out.push_str(a);
                rest = &tail[close + 1..];
            }
            None => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Effect name.
pub fn effect(lang: Lang, e: EffectKind) -> &'static str {
    t(
        lang,
        match e {
            EffectKind::Snow => Key::EffSnow,
            EffectKind::Rain => Key::EffRain,
            EffectKind::Sakura => Key::EffSakura,
            EffectKind::Leaves => Key::EffLeaves,
            EffectKind::Fireflies => Key::EffFireflies,
            EffectKind::Starfall => Key::EffStarfall,
        },
    )
}

/// One-line description of an effect (Effects tab cell).
pub fn effect_detail(lang: Lang, e: EffectKind) -> &'static str {
    t(
        lang,
        match e {
            EffectKind::Snow => Key::DetSnow,
            EffectKind::Rain => Key::DetRain,
            EffectKind::Sakura => Key::DetSakura,
            EffectKind::Leaves => Key::DetLeaves,
            EffectKind::Fireflies => Key::DetFireflies,
            EffectKind::Starfall => Key::DetStarfall,
        },
    )
}

/// Onboarding pitch for an effect.
pub fn pitch(lang: Lang, e: EffectKind) -> &'static str {
    t(
        lang,
        match e {
            EffectKind::Snow => Key::PitchSnow,
            EffectKind::Rain => Key::PitchRain,
            EffectKind::Sakura => Key::PitchSakura,
            EffectKind::Leaves => Key::PitchLeaves,
            EffectKind::Fireflies => Key::PitchFireflies,
            EffectKind::Starfall => Key::PitchStarfall,
        },
    )
}

/// Onboarding "try it" line for an effect, wrapped ("On screen: ...").
pub fn try_it(lang: Lang, e: EffectKind) -> String {
    let what = t(
        lang,
        match e {
            EffectKind::Snow => Key::TrySnow,
            EffectKind::Rain => Key::TryRain,
            EffectKind::Sakura => Key::TrySakura,
            EffectKind::Leaves => Key::TryLeaves,
            EffectKind::Fireflies => Key::TryFireflies,
            EffectKind::Starfall => Key::TryStarfall,
        },
    );
    fmt(lang, Key::ObTryWrap, &[what])
}

/// Wallpaper display name for the pack paired with an effect.
pub fn wallpaper_name(lang: Lang, e: EffectKind) -> &'static str {
    t(
        lang,
        match e {
            EffectKind::Snow => Key::WallWinter,
            EffectKind::Rain => Key::WallRain,
            EffectKind::Sakura => Key::WallSpring,
            EffectKind::Leaves => Key::WallAutumn,
            EffectKind::Fireflies => Key::WallFireflies,
            EffectKind::Starfall => Key::WallStars,
        },
    )
}

/// Wind menu label.
pub fn wind(lang: Lang, w: Wind) -> &'static str {
    t(
        lang,
        match w {
            Wind::None => Key::WindNone,
            Wind::GentleLeft => Key::WindGentleLeft,
            Wind::StrongLeft => Key::WindStrongLeft,
            Wind::GentleRight => Key::WindGentleRight,
            Wind::StrongRight => Key::WindStrongRight,
            Wind::Changing => Key::WindChanging,
        },
    )
}

/// Settings label of a visitor switch.
pub fn visitor(lang: Lang, v: Visitor) -> &'static str {
    t(
        lang,
        match v {
            Visitor::Santa => Key::VisitSanta,
            Visitor::Snowmen => Key::VisitSnowmen,
            Visitor::Yeti => Key::VisitYeti,
            Visitor::Plough => Key::VisitPlough,
        },
    )
}

/// Upper-case `s` for headings, with the language's casing rules: Turkish
/// maps `i` to `İ` and `ı` to `I`; Kazakh `і` already maps to `І`.
pub fn upper(lang: Lang, s: &str) -> String {
    match lang {
        Lang::Tr => s
            .chars()
            .map(|c| match c {
                'i' => "İ".to_string(),
                'ı' => "I".to_string(),
                c => c.to_uppercase().collect(),
            })
            .collect(),
        _ => s.to_uppercase(),
    }
}

/// Menu label with a check marker.
pub fn checked(_lang: Lang, label: &str) -> String {
    format!("\u{2713} {label}")
}

/// Fixed, untranslated title of the language row: the word "language" in all
/// nine languages, so a user stuck in an unknown one can still find it.
pub fn language_label() -> &'static str {
    "Language / 言語 / Idioma / Sprache / Langue / 언어 / Язык / Dil / Тіл"
}

/// Names of the eight times of day, in order (dawn first).
const TIMES: [Key; 8] = [
    Key::TimeDawn,
    Key::TimeSunrise,
    Key::TimeMorning,
    Key::TimeMidday,
    Key::TimeGolden,
    Key::TimeSunset,
    Key::TimeDusk,
    Key::TimeNight,
];

/// Time-of-day name by index 0..=7 (larger indices give the last one).
pub fn time_name(lang: Lang, index: usize) -> &'static str {
    t(lang, TIMES[index.min(TIMES.len() - 1)])
}

/// The twelve guide cards (title key, line key), in display order.
const TIPS: [(Key, Key); 12] = [
    (Key::HowMenuTitle, Key::HowMenuLine),
    (Key::HowSnowmanTitle, Key::HowSnowmanLine),
    (Key::HowSnowballTitle, Key::HowSnowballLine),
    (Key::HowSaveTitle, Key::HowSaveLine),
    (Key::HowGiftTitle, Key::HowGiftLine),
    (Key::HowTreeTitle, Key::HowTreeLine),
    (Key::HowPloughTitle, Key::HowPloughLine),
    (Key::HowMatchTitle, Key::HowMatchLine),
    (Key::HowBehindTitle, Key::HowBehindLine),
    (Key::HowSnowOnlyTitle, Key::HowSnowOnlyLine),
    (Key::HowBatteryTitle, Key::HowBatteryLine),
    (Key::HowScreensTitle, Key::HowScreensLine),
];

/// Number of how-to guide cards.
pub const HOW_TIPS: usize = TIPS.len();

/// How-to card `(title, line)` by index (larger indices give the last card).
pub fn how_tip(lang: Lang, index: usize) -> (&'static str, &'static str) {
    let (a, b) = TIPS[index.min(TIPS.len() - 1)];
    (t(lang, a), t(lang, b))
}

const TREE_QUIPS: [Key; 4] = [Key::TreeQuip1, Key::TreeQuip2, Key::TreeQuip3, Key::TreeQuip4];
const DRESS_QUIPS: [Key; 4] = [Key::DressQuip1, Key::DressQuip2, Key::DressQuip3, Key::DressQuip4];
const GIFT_THANKS: [Key; 4] = [Key::GiftThanks1, Key::GiftThanks2, Key::GiftThanks3, Key::GiftThanks4];
const TREE_SIZES: [Key; 6] =
    [Key::TreeSize0, Key::TreeSize1, Key::TreeSize2, Key::TreeSize3, Key::TreeSize4, Key::TreeSize5];

/// Tree card header quip; `pick` is any random number (taken modulo 4).
pub fn tree_quip(lang: Lang, pick: usize) -> &'static str {
    t(lang, TREE_QUIPS[pick % 4])
}

/// Snowman dress-up quip; `pick` is any random number (taken modulo 4).
pub fn dress_quip(lang: Lang, pick: usize) -> &'static str {
    t(lang, DRESS_QUIPS[pick % 4])
}

/// Santa gift thank-you badge; `pick` is any random number (taken modulo 4).
pub fn gift_thanks(lang: Lang, pick: usize) -> &'static str {
    t(lang, GIFT_THANKS[pick % 4])
}

/// Tree height class name, level 0..=5 (larger values clamp to 5).
pub fn tree_size(lang: Lang, level: u8) -> &'static str {
    t(lang, TREE_SIZES[usize::from(level).min(5)])
}

/// Case-insensitive lookup of a persisted id in a table.
fn lookup(table: &[(&str, Key)], raw: &str) -> Option<Key> {
    table.iter().find(|(id, _)| id.eq_ignore_ascii_case(raw)).map(|(_, k)| *k)
}

const ITEMS: [(&str, Key); 32] = [
    ("topHat", Key::HatTopHat),
    ("bobble", Key::HatBobble),
    ("crown", Key::HatCrown),
    ("cowboy", Key::HatCowboy),
    ("party", Key::HatParty),
    ("earmuffs", Key::HatEarmuffs),
    ("viking", Key::HatViking),
    ("santa", Key::HatSanta),
    ("classic", Key::FaceClassic),
    ("happy", Key::FaceHappy),
    ("wink", Key::FaceWink),
    ("cool", Key::FaceCool),
    ("surprised", Key::FaceSurprised),
    ("sleepy", Key::FaceSleepy),
    ("grumpy", Key::FaceGrumpy),
    ("love", Key::FaceLove),
    ("sticks", Key::ArmsSticks),
    ("waving", Key::ArmsWaving),
    ("mittens", Key::ArmsMittens),
    ("broom", Key::ArmsBroom),
    ("candyCane", Key::ArmsCandyCane),
    ("cocoa", Key::ArmsCocoa),
    ("gift", Key::ArmsGift),
    ("flowers", Key::ArmsFlowers),
    ("scarf", Key::OutScarf),
    ("stripedScarf", Key::OutStripedScarf),
    ("bowTie", Key::OutBowTie),
    ("tie", Key::OutTie),
    ("sweater", Key::OutSweater),
    ("puffer", Key::OutPuffer),
    ("tuxedo", Key::OutTuxedo),
    ("goldChain", Key::OutGoldChain),
];

/// Display name of a snowman item id such as `candyCane` (no `arms:` prefix).
/// An unknown id is shown as it is (snowman 5).
pub fn item_name(lang: Lang, id: &str) -> String {
    match lookup(&ITEMS, id) {
        Some(k) => t(lang, k).to_string(),
        None => id.to_string(),
    }
}

const DECOR: [(&str, Key); 16] = [
    ("lights", Key::DecoLights),
    ("candle", Key::DecoCandle),
    ("redBauble", Key::DecoRedBauble),
    ("goldBauble", Key::DecoGoldBauble),
    ("star", Key::DecoStar),
    ("wreath", Key::DecoWreath),
    ("stocking", Key::DecoStocking),
    ("candyCane", Key::DecoCandyCane),
    ("gift", Key::DecoGift),
    ("bells", Key::DecoBells),
    ("gingerbread", Key::DecoGingerbread),
    ("holly", Key::DecoHolly),
    ("santaHat", Key::DecoSantaHat),
    ("snowGlobe", Key::DecoSnowGlobe),
    ("cocoa", Key::DecoCocoa),
    ("boat", Key::DecoBoat),
];

/// Display name of a decoration by its persisted key (decorations 9).
pub fn decor_name(lang: Lang, raw: &str) -> Option<&'static str> {
    lookup(&DECOR, raw).map(|k| t(lang, k))
}

const TOPPERS: [(&str, Key); 6] = [
    ("star", Key::TopperStar),
    ("snowflake", Key::TopperSnowflake),
    ("bow", Key::TopperBow),
    ("bell", Key::TopperBell),
    ("heart", Key::TopperHeart),
    ("spire", Key::TopperSpire),
];

/// Display name of a tree topper by id (`star`, `snowflake`, `bow`, `bell`, `heart`, `spire`).
pub fn topper_name(lang: Lang, raw: &str) -> Option<&'static str> {
    lookup(&TOPPERS, raw).map(|k| t(lang, k))
}

const TRIMS: [(&str, Key); 10] = [
    ("baubles", Key::TrimBaubles),
    ("lights", Key::TrimLights),
    ("tinsel", Key::TrimGarland),
    ("garland", Key::TrimGarland),
    ("candy", Key::TrimCandyCanes),
    ("candyCanes", Key::TrimCandyCanes),
    ("bells", Key::TrimBells),
    ("gingerbread", Key::TrimGingerbread),
    ("bows", Key::TrimBows),
    ("stars", Key::TrimStars),
];

/// Display name of a tree trim by id (`baubles`, `lights`, `tinsel`, `candy`,
/// `bells`, `gingerbread`, `bows`, `stars`).
pub fn trim_name(lang: Lang, raw: &str) -> Option<&'static str> {
    lookup(&TRIMS, raw).map(|k| t(lang, k))
}

/// Plural category (CLDR subset used by the UI languages).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plural {
    One,
    Few,
    Many,
    Other,
}

/// Plural category of `n` in `lang`.
pub fn plural_category(lang: Lang, n: u64) -> Plural {
    match lang {
        // Turkish and Kazakh keep the noun singular after a numeral.
        Lang::Ja | Lang::Ko | Lang::Tr | Lang::Kk => Plural::Other,
        Lang::En | Lang::De | Lang::Es => {
            if n == 1 {
                Plural::One
            } else {
                Plural::Other
            }
        }
        Lang::Fr => {
            if n < 2 {
                Plural::One
            } else {
                Plural::Other
            }
        }
        Lang::Ru => {
            let (m10, m100) = (n % 10, n % 100);
            if m10 == 1 && m100 != 11 {
                Plural::One
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                Plural::Few
            } else {
                Plural::Many
            }
        }
    }
}

/// `"{n} screen(s)"` templates by language, then by [`Plural`] (one, few, many, other).
const SCREEN_COUNT: [[&str; 4]; 9] = [
    ["{0} screen", "{0} screens", "{0} screens", "{0} screens"],
    ["{0}画面", "{0}画面", "{0}画面", "{0}画面"],
    ["{0} pantalla", "{0} pantallas", "{0} pantallas", "{0} pantallas"],
    ["{0} Bildschirm", "{0} Bildschirme", "{0} Bildschirme", "{0} Bildschirme"],
    ["{0} écran", "{0} écrans", "{0} écrans", "{0} écrans"],
    ["{0}개 화면", "{0}개 화면", "{0}개 화면", "{0}개 화면"],
    ["{0} экран", "{0} экрана", "{0} экранов", "{0} экрана"],
    ["{0} ekran", "{0} ekran", "{0} ekran", "{0} ekran"],
    ["{0} экран", "{0} экран", "{0} экран", "{0} экран"],
];

/// "N screens" with the right plural form.
pub fn screen_count(lang: Lang, n: u64) -> String {
    fill(SCREEN_COUNT[col(lang)][plural_category(lang, n) as usize], &[&n.to_string()])
}

/// Step counter "Effect i of n".
pub fn step(lang: Lang, i: usize, n: usize) -> String {
    fmt(lang, Key::ObStep, &[&i.to_string(), &n.to_string()])
}

/// "with <weather>" link label.
pub fn with_weather(lang: Lang, e: EffectKind) -> String {
    fmt(lang, Key::WithWeather, &[effect(lang, e)])
}

/// "Version x" header.
pub fn version(lang: Lang, v: &str) -> String {
    fmt(lang, Key::SetVersion, &[v])
}

/// "Screen n" label.
pub fn screen_number(lang: Lang, n: usize) -> String {
    fmt(lang, Key::ScreenNumber, &[&n.to_string()])
}

/// Decoration counter, word order per language.
pub fn decor_count(lang: Lang, count: usize, most: usize) -> String {
    fmt(lang, Key::DecorCount, &[&count.to_string(), &most.to_string()])
}

/// "Version x is available" status line.
pub fn update_available(lang: Lang, v: &str) -> String {
    fmt(lang, Key::UpdAvailable, &[v])
}

/// Body of the new-version notice.
pub fn update_prompt_body(lang: Lang, v: &str) -> String {
    fmt(lang, Key::UpdPromptBody, &[v])
}

/// "Downloading… n%" with `done` of `total` bytes (0 % while the size is
/// unknown, never above 100 %).
pub fn update_downloading(lang: Lang, done: u64, total: u64) -> String {
    let pct = (done.saturating_mul(100)).checked_div(total).unwrap_or(0).min(100);
    fmt(lang, Key::UpdDownloading, &[&pct.to_string()])
}

/// Toast after an update: "Updated to version x".
pub fn update_done(lang: Lang, v: &str) -> String {
    fmt(lang, Key::UpdDone, &[v])
}

/// Why this copy cannot update itself, for the notice body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualReason {
    /// Installed by a package manager.
    Package,
    /// The app's folder is not writable.
    Folder,
    /// Any other reason.
    Other,
}

/// Notice body when only the download page helps.
pub fn update_manual(lang: Lang, why: ManualReason, v: &str) -> String {
    let k = match why {
        ManualReason::Package => Key::UpdManualPackage,
        ManualReason::Folder => Key::UpdManualFolder,
        ManualReason::Other => Key::UpdManualOther,
    };
    fmt(lang, k, &[v])
}

/// Month names (en, es, de, fr, ru genitive, tr, kk) for [`format_date`].
const MONTHS: [[&str; 12]; 7] = [
    [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ],
    [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ],
    [
        "января",
        "февраля",
        "марта",
        "апреля",
        "мая",
        "июня",
        "июля",
        "августа",
        "сентября",
        "октября",
        "ноября",
        "декабря",
    ],
    ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"],
    [
        "қаңтар",
        "ақпан",
        "наурыз",
        "сәуір",
        "мамыр",
        "маусым",
        "шілде",
        "тамыз",
        "қыркүйек",
        "қазан",
        "қараша",
        "желтоқсан",
    ],
];

/// Days in month `m` (1..=12) of year `y` (proleptic Gregorian).
fn days_in_month(y: u32, m: usize) -> u32 {
    match m {
        2 if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Parse an exact `yyyy-MM-dd` calendar date (year 1..=9999, real day of
/// that month); anything else is not a date.
fn parse_iso(s: &str) -> Option<(u32, usize, u32)> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<u32> {
        let part = &s[r];
        if part.bytes().all(|c| c.is_ascii_digit()) { part.parse().ok() } else { None }
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let m = m as usize;
    let real = y >= 1 && (1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m);
    if real { Some((y, m, d)) } else { None }
}

/// Changelog date in the language's long format; the raw text when it is not
/// an exact `yyyy-MM-dd` date (ui 3.4).
pub fn format_date(lang: Lang, iso: &str) -> String {
    let Some((y, m, d)) = parse_iso(iso) else {
        return iso.to_string();
    };
    match lang {
        Lang::En => format!("{} {d}, {y}", MONTHS[0][m - 1]),
        Lang::Ja => format!("{y}年{m}月{d}日"),
        Lang::Es => format!("{d} de {} de {y}", MONTHS[1][m - 1]),
        Lang::De => format!("{d}. {} {y}", MONTHS[2][m - 1]),
        Lang::Fr => format!("{d} {} {y}", MONTHS[3][m - 1]),
        Lang::Ko => format!("{y}년 {m}월 {d}일"),
        Lang::Ru => format!("{d} {} {y} г.", MONTHS[4][m - 1]),
        Lang::Tr => format!("{d} {} {y}", MONTHS[5][m - 1]),
        Lang::Kk => format!("{y} жылғы {d} {}", MONTHS[6][m - 1]),
    }
}

/// Santa quotes for a language (own set, ui 3.9). Pair with
/// `fp_characters::quote::next` using the list length.
pub fn quotes(lang: Lang) -> &'static [Quote] {
    match lang {
        Lang::En => quotes::EN,
        Lang::Ja => quotes::JA,
        Lang::Es => quotes::ES,
        Lang::De => quotes::DE,
        Lang::Fr => quotes::FR,
        Lang::Ko => quotes::KO,
        Lang::Ru => quotes::RU,
        Lang::Tr => quotes::TR,
        Lang::Kk => quotes::KK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            let j = rest[i..].find('}').map(|j| i + j).unwrap();
            out.push(rest[i..=j].to_string());
            rest = &rest[j + 1..];
        }
        out.sort();
        out
    }

    #[test]
    fn all_keys_have_text_in_every_language() {
        assert_eq!(Key::ALL.len(), strings::TABLE.len());
        for (i, k) in Key::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i);
            for l in Lang::ALL {
                let s = t(l, *k);
                assert!(!s.is_empty(), "{k:?} {l:?}");
                assert!(!s.to_lowercase().contains(concat!("koi", "art")), "{k:?} {l:?}");
            }
        }
    }

    #[test]
    fn placeholders_match_across_languages() {
        for k in Key::ALL {
            let want = tokens(t(Lang::En, *k));
            for l in Lang::ALL {
                assert_eq!(tokens(t(l, *k)), want, "{k:?} {l:?}");
            }
        }
    }

    #[test]
    fn every_language_is_translated() {
        for l in Lang::ALL.into_iter().skip(1) {
            let same = Key::ALL.iter().filter(|k| t(l, **k) == t(Lang::En, **k)).count();
            assert!(same * 4 < Key::ALL.len(), "{l:?} looks untranslated ({same} equal)");
        }
    }

    #[test]
    fn fmt_is_single_pass() {
        assert_eq!(fill("{0}-{1}", &["{1}", "x"]), "{1}-x");
        assert_eq!(fill("{0} {0}", &["a"]), "a a");
        assert_eq!(fill("{1} {0}", &["a", "b"]), "b a");
        assert_eq!(fill("{2} {x} {", &["a"]), "{2} {x} {");
        assert_eq!(fill("{} {0", &["a"]), "{} {0");
        assert_eq!(fill("no args", &[]), "no args");
        assert_eq!(fill("é{0}ü", &["ß"]), "éßü");
        assert_eq!(version(Lang::En, "{0}"), "Version {0}");
        assert_eq!(decor_count(Lang::En, 1, 2), "1 of 2 placed");
    }

    #[test]
    fn names_are_distinct_within_a_language() {
        fn distinct(mut v: Vec<&str>, what: &str, l: Lang) {
            let n = v.len();
            v.sort_unstable();
            v.dedup();
            assert_eq!(v.len(), n, "{what} {l:?}");
        }
        for l in Lang::ALL {
            distinct(EffectKind::ALL.iter().map(|e| effect(l, *e)).collect(), "effects", l);
            distinct(EffectKind::ALL.iter().map(|e| wallpaper_name(l, *e)).collect(), "papers", l);
            distinct(Wind::ALL.iter().map(|w| wind(l, *w)).collect(), "wind", l);
            distinct(Visitor::ALL.iter().map(|v| visitor(l, *v)).collect(), "visitors", l);
            distinct((0..8).map(|i| time_name(l, i)).collect(), "times", l);
            distinct((0..6).map(|i| tree_size(l, i)).collect(), "sizes", l);
            distinct(ITEMS.iter().map(|(_, k)| t(l, *k)).collect(), "items", l);
            distinct(DECOR.iter().map(|(_, k)| t(l, *k)).collect(), "decor", l);
            distinct(TOPPERS.iter().map(|(_, k)| t(l, *k)).collect(), "toppers", l);
            let mut trims: Vec<Key> = TRIMS.iter().map(|(_, k)| *k).collect();
            trims.dedup();
            assert_eq!(trims.len(), 8);
            distinct(trims.iter().map(|k| t(l, *k)).collect(), "trims", l);
            distinct((0..4).map(|i| tree_quip(l, i)).collect(), "tree quips", l);
            distinct((0..4).map(|i| dress_quip(l, i)).collect(), "dress quips", l);
            distinct((0..4).map(|i| gift_thanks(l, i)).collect(), "thanks", l);
            distinct((0..HOW_TIPS).map(|i| how_tip(l, i).0).collect(), "tips", l);
        }
        assert_eq!(with_weather(Lang::Ko, EffectKind::Snow), "눈 효과와 함께");
        assert_eq!(t(Lang::En, Key::TrayPointerHint), "Click this icon");
    }

    #[test]
    fn effect_helpers() {
        for l in Lang::ALL {
            for e in EffectKind::ALL {
                for s in [effect(l, e), effect_detail(l, e), pitch(l, e), wallpaper_name(l, e)] {
                    assert!(!s.is_empty());
                }
                assert!(try_it(l, e).len() > 4);
                assert!(with_weather(l, e).contains(effect(l, e)));
            }
            for w in Wind::ALL {
                assert!(!wind(l, w).is_empty());
            }
            for v in Visitor::ALL {
                assert!(!visitor(l, v).is_empty());
            }
        }
        assert_eq!(effect(Lang::En, EffectKind::Snow), "Snow");
        assert_eq!(wind(Lang::En, Wind::Changing), "Gusty");
        assert_eq!(visitor(Lang::En, Visitor::Yeti), "Yeti");
        assert_eq!(wallpaper_name(Lang::En, EffectKind::Starfall), "Stars");
        assert_eq!(try_it(Lang::En, EffectKind::Rain), "On screen: switch to the thunderstorm");
        assert_eq!(with_weather(Lang::En, EffectKind::Rain), "with Rain");
    }

    #[test]
    fn fmt_and_helpers() {
        assert_eq!(fmt(Lang::En, Key::ObStep, &["2", "6"]), "Effect 2 of 6");
        assert_eq!(fmt(Lang::En, Key::AppName, &[]), "Frostpane");
        assert_eq!(step(Lang::Ru, 1, 6), "Эффект 1 из 6");
        assert_eq!(version(Lang::En, "1.2.3"), "Version 1.2.3");
        assert_eq!(screen_number(Lang::De, 2), "Bildschirm 2");
        assert_eq!(decor_count(Lang::En, 3, 30), "3 of 30 placed");
        assert_eq!(decor_count(Lang::Ko, 3, 30), "30개 중 3개 배치됨");
        assert_eq!(update_available(Lang::En, "0.2.0"), "Version 0.2.0 is available");
        assert!(update_prompt_body(Lang::Fr, "0.2.0").contains("0.2.0"));
        assert_eq!(update_downloading(Lang::En, 50, 200), "Downloading… 25%");
        assert_eq!(update_downloading(Lang::Tr, 5, 0), "İndiriliyor… %0");
        assert_eq!(update_downloading(Lang::En, 300, 200), "Downloading… 100%");
        assert_eq!(update_downloading(Lang::En, u64::MAX, u64::MAX), "Downloading… 1%");
        assert_eq!(update_done(Lang::En, "2.0.0"), "Updated to version 2.0.0");
        assert_eq!(update_done(Lang::Kk, "2.0.0"), "2.0.0 нұсқасына жаңартылды");
        for why in [ManualReason::Package, ManualReason::Folder, ManualReason::Other] {
            for l in Lang::ALL {
                assert!(update_manual(l, why, "3.1.4").contains("3.1.4"), "{l:?} {why:?}");
            }
        }
        assert_ne!(
            update_manual(Lang::En, ManualReason::Package, "1"),
            update_manual(Lang::En, ManualReason::Folder, "1")
        );
        assert_eq!(checked(Lang::En, "Snow"), "\u{2713} Snow");
        assert!(language_label().contains("Язык"));
        assert!(language_label().ends_with("/ Dil / Тіл"));
        assert_eq!(with_weather(Lang::Tr, EffectKind::Snow), "Kar efektiyle");
        assert_eq!(step(Lang::Kk, 2, 6), "Әсер 2/6");
        assert_eq!(t(Lang::Tr, Key::TabSettings), "Ayarlar");
        assert_eq!(t(Lang::Kk, Key::TabSettings), "Баптаулар");
    }

    #[test]
    fn upper_follows_language_casing() {
        assert_eq!(upper(Lang::Tr, "Bir efekt seç"), "BİR EFEKT SEÇ");
        assert_eq!(upper(Lang::Tr, "ışık ve iğne"), "IŞIK VE İĞNE");
        assert_eq!(upper(Lang::En, "pick it"), "PICK IT");
        assert_eq!(upper(Lang::Kk, "әсер таңда, тіл"), "ӘСЕР ТАҢДА, ТІЛ");
        assert_eq!(upper(Lang::De, "straße"), "STRASSE");
    }

    #[test]
    fn time_and_tips() {
        assert_eq!(time_name(Lang::En, 0), "Dawn");
        assert_eq!(time_name(Lang::En, 7), "Night");
        assert_eq!(time_name(Lang::En, 99), "Night");
        assert_eq!(HOW_TIPS, 12);
        for l in Lang::ALL {
            for i in 0..HOW_TIPS {
                let (a, b) = how_tip(l, i);
                assert!(!a.is_empty() && !b.is_empty());
            }
        }
        assert_eq!(how_tip(Lang::En, 99), how_tip(Lang::En, 11));
        assert_eq!(how_tip(Lang::En, 0).0, "Find the tray menu");
    }

    #[test]
    fn random_picks_wrap() {
        for i in 0..8 {
            assert_eq!(tree_quip(Lang::En, i), tree_quip(Lang::En, i + 4));
            assert_eq!(dress_quip(Lang::En, i), dress_quip(Lang::En, i + 4));
            assert_eq!(gift_thanks(Lang::En, i), gift_thanks(Lang::En, i + 4));
        }
        assert_ne!(tree_quip(Lang::En, 0), tree_quip(Lang::En, 1));
        assert_eq!(gift_thanks(Lang::En, 0), "Thank you, Santa!");
        assert_eq!(dress_quip(Lang::En, 1), "Winter fashion week is on.");
    }

    #[test]
    fn tree_sizes_clamp() {
        assert_eq!(tree_size(Lang::En, 0), "Bonsai");
        assert_eq!(tree_size(Lang::En, 4), "Too big");
        assert_eq!(tree_size(Lang::En, 5), "Send help");
        assert_eq!(tree_size(Lang::En, 200), "Send help");
    }

    #[test]
    fn named_things() {
        for l in Lang::ALL {
            for (id, _) in ITEMS {
                assert_ne!(item_name(l, id), id);
            }
            for (id, _) in DECOR {
                assert!(decor_name(l, id).is_some());
            }
            for (id, _) in TOPPERS {
                assert!(topper_name(l, id).is_some());
            }
            for (id, _) in TRIMS {
                assert!(trim_name(l, id).is_some());
            }
        }
        assert_eq!(item_name(Lang::En, "candyCane"), "Candy cane");
        assert_eq!(item_name(Lang::En, "candycane"), "Candy cane");
        assert_eq!(item_name(Lang::En, "nope"), "nope");
        assert_eq!(decor_name(Lang::En, "snowGlobe"), Some("Snow globe"));
        assert_eq!(decor_name(Lang::En, "x"), None);
        assert_eq!(topper_name(Lang::En, "Spire"), Some("Spire"));
        assert_eq!(topper_name(Lang::En, "x"), None);
        assert_eq!(trim_name(Lang::En, "tinsel"), Some("Garland"));
        assert_eq!(trim_name(Lang::En, "candy"), Some("Candy canes"));
        assert_eq!(trim_name(Lang::En, "x"), None);
    }

    #[test]
    fn plurals() {
        use Plural::*;
        assert_eq!(plural_category(Lang::En, 1), One);
        assert_eq!(plural_category(Lang::En, 0), Other);
        assert_eq!(plural_category(Lang::De, 2), Other);
        assert_eq!(plural_category(Lang::Es, 1), One);
        assert_eq!(plural_category(Lang::Fr, 0), One);
        assert_eq!(plural_category(Lang::Fr, 1), One);
        assert_eq!(plural_category(Lang::Fr, 2), Other);
        assert_eq!(plural_category(Lang::Ja, 1), Other);
        assert_eq!(plural_category(Lang::Ko, 1), Other);
        assert_eq!(plural_category(Lang::Ru, 1), One);
        assert_eq!(plural_category(Lang::Ru, 21), One);
        assert_eq!(plural_category(Lang::Ru, 11), Many);
        assert_eq!(plural_category(Lang::Ru, 2), Few);
        assert_eq!(plural_category(Lang::Ru, 23), Few);
        assert_eq!(plural_category(Lang::Ru, 12), Many);
        assert_eq!(plural_category(Lang::Ru, 14), Many);
        assert_eq!(plural_category(Lang::Ru, 5), Many);
        assert_eq!(plural_category(Lang::Ru, 0), Many);
        assert_eq!(screen_count(Lang::En, 1), "1 screen");
        assert_eq!(screen_count(Lang::En, 3), "3 screens");
        assert_eq!(screen_count(Lang::Ru, 2), "2 экрана");
        assert_eq!(screen_count(Lang::Ru, 5), "5 экранов");
        assert_eq!(screen_count(Lang::Ru, 21), "21 экран");
        assert_eq!(screen_count(Lang::Ja, 2), "2画面");
        for n in [1, 3, 21] {
            assert_eq!(plural_category(Lang::Tr, n), Other);
            assert_eq!(plural_category(Lang::Kk, n), Other);
        }
        assert_eq!(screen_count(Lang::Tr, 3), "3 ekran");
        assert_eq!(screen_count(Lang::Kk, 3), "3 экран");
        for l in Lang::ALL {
            for n in [0, 1, 2, 5, 11, 21] {
                assert!(screen_count(l, n).contains(&n.to_string()));
            }
        }
    }

    #[test]
    fn dates() {
        assert_eq!(format_date(Lang::En, "2026-11-16"), "November 16, 2026");
        assert_eq!(format_date(Lang::Ja, "2026-11-16"), "2026年11月16日");
        assert_eq!(format_date(Lang::Es, "2026-03-05"), "5 de marzo de 2026");
        assert_eq!(format_date(Lang::De, "2026-03-05"), "5. März 2026");
        assert_eq!(format_date(Lang::Fr, "2026-12-01"), "1 décembre 2026");
        assert_eq!(format_date(Lang::Ko, "2026-01-02"), "2026년 1월 2일");
        assert_eq!(format_date(Lang::Ru, "2026-08-09"), "9 августа 2026 г.");
        assert_eq!(format_date(Lang::Tr, "2026-11-18"), "18 Kasım 2026");
        assert_eq!(format_date(Lang::Tr, "2026-02-01"), "1 Şubat 2026");
        assert_eq!(format_date(Lang::Kk, "2026-11-18"), "2026 жылғы 18 қараша");
        assert_eq!(format_date(Lang::Kk, "2026-04-30"), "2026 жылғы 30 сәуір");
        assert_eq!(format_date(Lang::En, "2028-02-29"), "February 29, 2028");
        assert_eq!(format_date(Lang::En, "2000-02-29"), "February 29, 2000");
        assert_eq!(format_date(Lang::En, "2026-12-31"), "December 31, 2026");
        assert_eq!(format_date(Lang::En, "0001-01-01"), "January 1, 1");
        assert_eq!(format_date(Lang::De, "2026-01-31"), "31. Januar 2026");
        let bad = [
            "",
            "2026-11-1",
            "2026/11/16",
            "2026-13-01",
            "2026-00-01",
            "2026-11-32",
            "2026-11-00",
            "2026-02-29",
            "2100-02-29",
            "2026-04-31",
            "2026-06-31",
            "2026-09-31",
            "2026-11-31",
            "0000-01-01",
            "20x6-11-16",
            "2026-1a-16",
            "2026-11-1a",
            "2026-1é-1",
            "é026-11-16",
        ];
        for b in bad {
            assert_eq!(format_date(Lang::En, b), b);
        }
        for l in Lang::ALL {
            for m in 1..=12 {
                let iso = format!("2026-{m:02}-15");
                let out = format_date(l, &iso);
                assert_ne!(out, iso, "{l:?} {m}");
                assert!(out.contains("15") && out.contains("2026"), "{out}");
            }
        }
        for (m, name) in MONTHS[3].iter().enumerate() {
            assert_eq!(format_date(Lang::Fr, &format!("2026-{:02}-01", m + 1)), format!("1 {name} 2026"));
        }
    }

    #[test]
    fn quote_sets() {
        for l in Lang::ALL {
            let qs = quotes(l);
            assert!(qs.len() >= 10);
            for q in qs {
                assert!(!q.text.is_empty());
                assert!(q.by.is_none_or(|b| !b.is_empty() && b.chars().count() < 40));
                assert!(q.text.chars().count() < 120);
            }
            let mut texts: Vec<_> = qs.iter().map(|q| q.text).collect();
            texts.sort();
            texts.dedup();
            assert_eq!(texts.len(), qs.len());
        }
        let q = Quote { text: "a", by: Some("b") };
        assert_eq!(q, q);
    }
}
