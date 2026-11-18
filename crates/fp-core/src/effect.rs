//! Canonical enums: effects (misc 2), visitors (social 2.13), languages
//! (core-app 2.16). Raw strings are the persisted values.

use serde::{Deserialize, Serialize};

/// The six weather effects, in the fixed enum order used by menus and random picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectKind {
    Snow,
    Rain,
    Sakura,
    Leaves,
    Fireflies,
    Starfall,
}

impl EffectKind {
    pub const ALL: [EffectKind; 6] = [
        EffectKind::Snow,
        EffectKind::Rain,
        EffectKind::Sakura,
        EffectKind::Leaves,
        EffectKind::Fireflies,
        EffectKind::Starfall,
    ];

    pub fn raw(self) -> &'static str {
        match self {
            EffectKind::Snow => "snow",
            EffectKind::Rain => "rain",
            EffectKind::Sakura => "sakura",
            EffectKind::Leaves => "leaves",
            EffectKind::Fireflies => "fireflies",
            EffectKind::Starfall => "starfall",
        }
    }

    /// Exact, case-sensitive parse; anything else (including "none") is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.raw() == s)
    }

    /// Tray/menu tint, 0xRRGGBB.
    pub fn menu_color(self) -> u32 {
        match self {
            EffectKind::Snow => 0x59B2FF,
            EffectKind::Rain => 0x4080FA,
            EffectKind::Sakura => 0xFF73A8,
            EffectKind::Leaves => 0xF28C26,
            EffectKind::Fireflies => 0xF2C71A,
            EffectKind::Starfall => 0x9E73FF,
        }
    }

    /// Paired wallpaper pack id (Frostpane uses clear ids; see ARCHITECTURE).
    pub fn wallpaper_id(self) -> &'static str {
        match self {
            EffectKind::Snow => "winter",
            EffectKind::Rain => "rain",
            EffectKind::Sakura => "spring",
            EffectKind::Leaves => "autumn",
            EffectKind::Fireflies => "fireflies",
            EffectKind::Starfall => "night",
        }
    }
}

/// Automatic visitors that can be switched off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visitor {
    Santa,
    Snowmen,
    Yeti,
    Plough,
}

impl Visitor {
    pub const ALL: [Visitor; 4] = [Visitor::Santa, Visitor::Snowmen, Visitor::Yeti, Visitor::Plough];

    pub fn raw(self) -> &'static str {
        match self {
            Visitor::Santa => "santa",
            Visitor::Snowmen => "snowmen",
            Visitor::Yeti => "yeti",
            Visitor::Plough => "plough",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.raw() == s)
    }
}

/// UI languages, in picker order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ja,
    Es,
    De,
    Fr,
    Ko,
    Ru,
}

impl Lang {
    pub const ALL: [Lang; 7] = [Lang::En, Lang::Ja, Lang::Es, Lang::De, Lang::Fr, Lang::Ko, Lang::Ru];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Es => "es",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::Ko => "ko",
            Lang::Ru => "ru",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == s)
    }

    /// Language name written in that language.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ja => "日本語",
            Lang::Es => "Español",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::Ko => "한국어",
            Lang::Ru => "Русский",
        }
    }

    /// Startup language: saved code if valid, else OS 2-letter code if
    /// supported, else English.
    pub fn startup(saved: Option<&str>, os: &str) -> Lang {
        saved.and_then(Lang::parse).or_else(|| Lang::parse(os)).unwrap_or(Lang::En)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects() {
        for e in EffectKind::ALL {
            assert_eq!(EffectKind::parse(e.raw()), Some(e));
            assert!(e.menu_color() > 0);
            assert!(!e.wallpaper_id().is_empty());
        }
        assert_eq!(EffectKind::parse("Snow"), None);
        assert_eq!(EffectKind::parse("none"), None);
        assert_eq!(serde_json::to_string(&EffectKind::Starfall).unwrap(), "\"starfall\"");
    }

    #[test]
    fn visitors_and_langs() {
        for v in Visitor::ALL {
            assert_eq!(Visitor::parse(v.raw()), Some(v));
        }
        assert_eq!(Visitor::parse("elf"), None);
        for l in Lang::ALL {
            assert_eq!(Lang::parse(l.code()), Some(l));
            assert!(!l.native_name().is_empty());
        }
        assert_eq!(Lang::startup(Some("de"), "fr"), Lang::De);
        assert_eq!(Lang::startup(Some("xx"), "fr"), Lang::Fr);
        assert_eq!(Lang::startup(None, "tr"), Lang::En);
    }
}
