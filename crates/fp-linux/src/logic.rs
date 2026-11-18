//! Platform-free parsers of the Linux session services.

use std::path::{Path, PathBuf};

/// `$XDG_DATA_HOME/frostpane`, else `~/.local/share/frostpane` (XDG base
/// directories: a relative `XDG_DATA_HOME` is ignored).
pub fn data_dir(xdg_data_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(x) = xdg_data_home.map(str::trim).filter(|x| x.starts_with('/')) {
        return Some(Path::new(x).join("frostpane"));
    }
    home.map(str::trim).filter(|h| !h.is_empty()).map(|h| Path::new(h).join(".local/share/frostpane"))
}

/// Two-letter UI language from `LC_ALL`, `LC_MESSAGES`, `LANG` (first set,
/// in that order); `C`/`POSIX` or nothing gives `"en"`.
pub fn language(lc_all: Option<&str>, lc_messages: Option<&str>, lang: Option<&str>) -> String {
    let v = [lc_all, lc_messages, lang].into_iter().flatten().map(str::trim).find(|v| !v.is_empty());
    let code: String = v.unwrap_or("").chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    if code.len() < 2 || code.eq_ignore_ascii_case("c") || code.eq_ignore_ascii_case("posix") {
        return "en".into();
    }
    code.to_ascii_lowercase()
}

/// IANA zone from `TZ` (`:Europe/Berlin` or `Europe/Berlin`) or the target of
/// the `/etc/localtime` link (`.../zoneinfo/Europe/Berlin`); else `"UTC"`.
pub fn time_zone(tz: Option<&str>, localtime_link: Option<&Path>) -> String {
    if let Some(t) = tz.map(|t| t.trim().trim_start_matches(':')).filter(|t| !t.is_empty() && !t.starts_with('/')) {
        return t.to_string();
    }
    let link = localtime_link.and_then(Path::to_str).unwrap_or("");
    match link.rfind("zoneinfo/") {
        Some(i) if i + 9 < link.len() => link[i + 9..].to_string(),
        _ => "UTC".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dirs() {
        assert_eq!(data_dir(Some("/x/data"), Some("/home/u")), Some(PathBuf::from("/x/data/frostpane")));
        assert_eq!(data_dir(Some("rel"), Some("/home/u")), Some(Path::new("/home/u").join(".local/share/frostpane")));
        assert_eq!(data_dir(None, Some(" ")), None);
        assert_eq!(data_dir(None, None), None);
    }

    #[test]
    fn languages() {
        assert_eq!(language(None, None, Some("tr_TR.UTF-8")), "tr");
        assert_eq!(language(Some(""), Some("de_DE"), Some("fr_FR")), "de");
        assert_eq!(language(Some("C.UTF-8"), None, Some("tr_TR")), "en");
        assert_eq!(language(None, None, Some("POSIX")), "en");
        assert_eq!(language(None, None, None), "en");
        assert_eq!(language(Some("PT_br"), None, None), "pt");
    }

    #[test]
    fn zones() {
        assert_eq!(time_zone(Some(":Europe/Istanbul"), None), "Europe/Istanbul");
        assert_eq!(time_zone(Some("/etc/zone"), Some(Path::new("/usr/share/zoneinfo/Asia/Tokyo"))), "Asia/Tokyo");
        assert_eq!(time_zone(None, Some(Path::new("/usr/share/zoneinfo/"))), "UTC");
        assert_eq!(time_zone(None, None), "UTC");
    }
}
