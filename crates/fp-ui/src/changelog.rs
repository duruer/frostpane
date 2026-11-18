//! Changelog tab data and logic (ui 3.4). The release list is Frostpane's
//! own; texts exist in the seven UI languages (English is the fallback).

use fp_core::effect::Lang;
use fp_text::Key;

/// Kind of a change, in display order (New, Improved, Fixed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    New,
    Improved,
    Fixed,
}

impl Kind {
    pub fn label(self, lang: Lang) -> &'static str {
        fp_text::t(
            lang,
            match self {
                Kind::New => Key::ChangeNew,
                Kind::Improved => Key::ChangeImproved,
                Kind::Fixed => Key::ChangeFixed,
            },
        )
    }

    /// Pill colour (0xRRGGBB).
    pub fn color(self) -> u32 {
        match self {
            Kind::New => 0x73B2FF,
            Kind::Improved => 0x66D18C,
            Kind::Fixed => 0xFFB259,
        }
    }
}

/// One change line: texts in `Lang::ALL` order (en ja es de fr ko ru); an
/// empty string falls back to English.
#[derive(Clone, Copy, Debug)]
pub struct Change {
    pub kind: Kind,
    pub text: [&'static str; 7],
}

#[derive(Clone, Copy, Debug)]
pub struct Release {
    pub number: &'static str,
    /// `yyyy-MM-dd`.
    pub date: &'static str,
    pub changes: &'static [Change],
}

fn lang_index(lang: Lang) -> usize {
    Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0)
}

impl Change {
    /// Text in `lang`, English when that translation is empty.
    pub fn text(&self, lang: Lang) -> &'static str {
        let t = self.text[lang_index(lang)];
        if t.is_empty() { self.text[0] } else { t }
    }
}

impl Release {
    /// Changes sorted by kind (stable, so the written order holds inside a kind).
    pub fn sorted(&self) -> Vec<&Change> {
        let mut v: Vec<&Change> = self.changes.iter().collect();
        v.sort_by_key(|c| c.kind);
        v
    }
}

const fn c(kind: Kind, text: [&'static str; 7]) -> Change {
    Change { kind, text }
}

static V0_1_0: [Change; 8] = [
    c(
        Kind::New,
        [
            "Six effects for your desktop: snow, rain with thunderstorms, cherry blossom, autumn leaves, fireflies and shooting stars.",
            "デスクトップに六つの効果：雪、雷雨つきの雨、桜、紅葉、蛍、流れ星。",
            "Seis efectos para tu escritorio: nieve, lluvia con tormenta, cerezos, hojas de otoño, luciérnagas y estrellas fugaces.",
            "Sechs Effekte für den Desktop: Schnee, Regen mit Gewitter, Kirschblüte, Herbstlaub, Glühwürmchen und Sternschnuppen.",
            "Six effets pour votre bureau : neige, pluie avec orage, cerisiers, feuilles d’automne, lucioles et étoiles filantes.",
            "바탕화면을 위한 여섯 가지 효과: 눈, 천둥을 동반한 비, 벚꽃, 단풍, 반딧불이, 별똥별.",
            "Шесть эффектов для рабочего стола: снег, дождь с грозой, сакура, листопад, светлячки и падающие звёзды.",
        ],
    ),
    c(
        Kind::New,
        [
            "Follow the weather outside with a city you pick, follow the season, or let Frostpane surprise you.",
            "選んだ都市の外の天気や季節に合わせたり、おまかせにしたりできます。",
            "Sigue el clima de fuera con la ciudad que elijas, sigue la estación o deja que Frostpane te sorprenda.",
            "Folge dem Wetter einer Stadt deiner Wahl, der Jahreszeit oder lass dich überraschen.",
            "Suivez la météo d’une ville de votre choix, la saison, ou laissez-vous surprendre.",
            "원하는 도시의 바깥 날씨나 계절을 따라가거나 랜덤으로 즐겨 보세요.",
            "Следуйте погоде выбранного города или времени года, либо доверьтесь случаю.",
        ],
    ),
    c(
        Kind::New,
        [
            "Roll snowballs, stack snowmen and dress them up from the style card.",
            "雪玉を転がして雪だるまを作り、スタイルカードで着せ替えができます。",
            "Haz bolas de nieve, apila muñecos y vístelos desde la tarjeta de estilo.",
            "Rolle Schneebälle, baue Schneemänner und kleide sie mit der Stilkarte ein.",
            "Roulez des boules de neige, empilez des bonshommes et habillez-les avec la carte de style.",
            "눈덩이를 굴려 눈사람을 쌓고 스타일 카드로 꾸며 보세요.",
            "Катайте снежки, лепите снеговиков и наряжайте их карточкой стиля.",
        ],
    ),
    c(
        Kind::New,
        [
            "Plant and decorate trees, and hang lights, baubles and more anywhere on the desktop.",
            "木を植えて飾り付け、ライトやオーナメントをデスクトップのどこにでも吊るせます。",
            "Planta y decora árboles, y cuelga luces, bolas y más en cualquier parte del escritorio.",
            "Pflanze und schmücke Bäume und hänge Lichter, Kugeln und mehr überall auf den Desktop.",
            "Plantez et décorez des sapins, accrochez guirlandes et boules où vous voulez sur le bureau.",
            "나무를 심어 꾸미고 조명과 장식을 바탕화면 어디든 걸어 보세요.",
            "Сажайте и украшайте ёлки, развешивайте гирлянды и шары где угодно на рабочем столе.",
        ],
    ),
    c(
        Kind::New,
        [
            "Visitors drop by: Santa with a daily gift, the snowplough, the yeti, snowman fights and a little toy train.",
            "サンタ（毎日のプレゼント）、除雪車、イエティ、雪だるま合戦、おもちゃの電車が遊びに来ます。",
            "Llegan visitas: Santa con un regalo diario, la quitanieves, el yeti, peleas de muñecos y un tren de juguete.",
            "Besuch kommt vorbei: der Weihnachtsmann mit Tagesgeschenk, Schneepflug, Yeti, Schneemann-Kämpfe und ein Spielzeugzug.",
            "Des visiteurs passent : le Père Noël et son cadeau du jour, le chasse-neige, le yéti, des combats de bonshommes et un petit train.",
            "산타(매일 선물), 제설차, 예티, 눈사람 싸움, 장난감 기차가 찾아와요.",
            "Приходят гости: Санта с подарком дня, снегоуборщик, йети, бои снеговиков и игрушечный поезд.",
        ],
    ),
    c(
        Kind::New,
        [
            "Wallpapers that change with the time of day.",
            "時間帯で変わる壁紙。",
            "Fondos que cambian con la hora del día.",
            "Hintergründe, die sich mit der Tageszeit ändern.",
            "Des fonds d’écran qui changent au fil de la journée.",
            "하루 시간에 따라 바뀌는 배경화면.",
            "Обои, меняющиеся в течение дня.",
        ],
    ),
    c(
        Kind::New,
        [
            "Seven languages, a settings window, and a tray menu with everything in reach.",
            "七つの言語、設定ウィンドウ、すべてに手が届くトレイメニュー。",
            "Siete idiomas, una ventana de ajustes y un menú de bandeja con todo a mano.",
            "Sieben Sprachen, ein Einstellungsfenster und ein Tray-Menü mit allem in Reichweite.",
            "Sept langues, une fenêtre de réglages et un menu de zone de notification à portée de main.",
            "일곱 가지 언어, 설정 창, 모든 것이 손닿는 트레이 메뉴.",
            "Семь языков, окно настроек и меню в трее со всем под рукой.",
        ],
    ),
    c(
        Kind::Improved,
        [
            "Weather pauses on battery and stays out of the way of fullscreen apps.",
            "バッテリー駆動時は一時停止し、全画面アプリの邪魔をしません。",
            "El clima se pausa con batería y no estorba a las apps a pantalla completa.",
            "Das Wetter pausiert im Akkubetrieb und stört keine Vollbild-Apps.",
            "La météo se met en pause sur batterie et ne gêne pas les applis plein écran.",
            "배터리 사용 중에는 멈추고 전체 화면 앱을 방해하지 않아요.",
            "На батарее погода приостанавливается и не мешает полноэкранным приложениям.",
        ],
    ),
];

static RELEASES: [Release; 1] = [Release { number: "0.1.0", date: "2026-11-17", changes: &V0_1_0 }];

/// All releases, newest first.
pub fn releases() -> &'static [Release] {
    &RELEASES
}

/// Number of the latest release.
pub fn latest() -> &'static str {
    RELEASES[0].number
}

/// True when the stored `seenChangelog` differs from the latest release.
pub fn unseen(seen: Option<&str>) -> bool {
    seen != Some(latest())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unseen_and_latest() {
        assert_eq!(latest(), "0.1.0");
        assert!(unseen(None));
        assert!(unseen(Some("0.0.1")));
        assert!(!unseen(Some("0.1.0")));
    }

    #[test]
    fn every_text_exists_in_every_language() {
        for r in releases() {
            assert!(!r.changes.is_empty());
            for ch in r.changes {
                for l in Lang::ALL {
                    assert!(!ch.text(l).is_empty());
                }
            }
        }
    }

    #[test]
    fn dates_format_in_every_language() {
        for l in Lang::ALL {
            let s = fp_text::format_date(l, releases()[0].date);
            assert!(s.contains("2026"), "{s}");
        }
    }

    #[test]
    fn sorted_is_stable_by_kind() {
        static C: [Change; 4] = [
            c(Kind::Fixed, ["f", "", "", "", "", "", ""]),
            c(Kind::New, ["n1", "", "", "", "", "", ""]),
            c(Kind::Improved, ["i", "", "", "", "", "", ""]),
            c(Kind::New, ["n2", "", "", "", "", "", ""]),
        ];
        let r = Release { number: "9", date: "2026-01-01", changes: &C };
        let t: Vec<_> = r.sorted().iter().map(|x| x.text(Lang::De)).collect();
        assert_eq!(t, ["n1", "n2", "i", "f"]);
    }

    #[test]
    fn kind_labels_and_colours() {
        assert_eq!(Kind::New.color(), 0x73B2FF);
        assert_eq!(Kind::Improved.color(), 0x66D18C);
        assert_eq!(Kind::Fixed.color(), 0xFFB259);
        for k in [Kind::New, Kind::Improved, Kind::Fixed] {
            assert!(!k.label(Lang::En).is_empty());
        }
    }
}
