//! Platform-free logic of the Linux session services: environment parsing,
//! desktop detection, `.desktop` text, command building and the meaning of
//! D-Bus replies and signals. Compiles and is tested on every target.

use fp_platform::SystemEvent;
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

// ---------------------------------------------------------------- instance

/// Lock file and socket of the single-instance guard: in `$XDG_RUNTIME_DIR`
/// when it is an absolute path, else `/tmp` with the user id in the names.
pub fn instance_paths(xdg_runtime_dir: Option<&str>, uid: u32) -> (PathBuf, PathBuf) {
    match xdg_runtime_dir.map(str::trim).filter(|d| d.starts_with('/')) {
        Some(d) => (Path::new(d).join("frostpane.lock"), Path::new(d).join("frostpane.sock")),
        None => {
            let tmp = Path::new("/tmp");
            (tmp.join(format!("frostpane-{uid}.lock")), tmp.join(format!("frostpane-{uid}.sock")))
        }
    }
}

/// The line a second launch sends to the first instance.
pub const OPEN_MESSAGE: &[u8] = b"open\n";

/// The listener acts on `open` lines only.
pub fn is_open_message(line: &str) -> bool {
    line.trim() == "open"
}

// ----------------------------------------------------------------- desktop

/// The desktop environment, as far as wallpaper handling cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    /// GNOME, Unity, Budgie and other gsettings `org.gnome.desktop` desktops.
    Gnome,
    Cinnamon,
    Mate,
    Kde,
    Xfce,
    Sway,
    Hyprland,
    Other,
}

/// Detects the desktop from `XDG_CURRENT_DESKTOP` (colon list), then
/// `DESKTOP_SESSION`, then the compositor's own variables.
pub fn detect_desktop(
    current: Option<&str>,
    session: Option<&str>,
    sway_sock: bool,
    hyprland_signature: bool,
) -> Desktop {
    let words = |s: Option<&str>| -> Vec<String> {
        s.unwrap_or("")
            .split([':', ';'])
            .map(|w| w.trim().to_ascii_lowercase())
            .map(|w| w.strip_prefix("x-").map(String::from).unwrap_or(w))
            .filter(|w| !w.is_empty())
            .collect()
    };
    for list in [words(current), words(session)] {
        let has = |names: &[&str]| list.iter().any(|w| names.contains(&w.as_str()));
        if has(&["cinnamon"]) {
            return Desktop::Cinnamon;
        }
        if has(&["mate"]) {
            return Desktop::Mate;
        }
        if has(&["kde", "plasma"]) {
            return Desktop::Kde;
        }
        if has(&["xfce", "xubuntu"]) {
            return Desktop::Xfce;
        }
        if has(&["sway"]) {
            return Desktop::Sway;
        }
        if has(&["hyprland"]) {
            return Desktop::Hyprland;
        }
        if has(&["gnome", "unity", "budgie", "ubuntu", "gnome-classic", "gnome-flashback"]) {
            return Desktop::Gnome;
        }
    }
    if hyprland_signature {
        Desktop::Hyprland
    } else if sway_sock {
        Desktop::Sway
    } else {
        Desktop::Other
    }
}

/// `program` as an executable name in a `PATH` value; `exists` answers for
/// one candidate path.
pub fn find_in_path(program: &str, path: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(program)).find(|p| exists(p))
}

/// Only `https:` links are opened.
pub fn url_allowed(url: &str) -> bool {
    url.trim().to_ascii_lowercase().starts_with("https://")
}

/// Seconds in `/proc/uptime` ("12345.67 54321.00") as milliseconds.
pub fn uptime_ms(proc_uptime: &str) -> Option<u64> {
    let secs: f64 = proc_uptime.split_whitespace().next()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some((secs * 1000.0) as u64)
}

// --------------------------------------------------------------- autostart

/// Desktop Entry `Exec` quoting: arguments with reserved characters go in
/// double quotes with `"`, `` ` ``, `$`, `\` escaped; `%` is doubled.
pub fn exec_quote(arg: &str) -> String {
    let arg = arg.replace('%', "%%");
    let plain = !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./%+=:,@".contains(c));
    if plain {
        return arg;
    }
    let mut out = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Content of `~/.config/autostart/frostpane.desktop`. `exe` is `$APPIMAGE`
/// or the running executable.
pub fn autostart_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Frostpane\nComment=Weather overlay\nExec={} --quiet\n\
         Icon=frostpane\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exec_quote(exe)
    )
}

/// `$XDG_CONFIG_HOME/autostart` else `~/.config/autostart`.
pub fn autostart_dir(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(x) = xdg_config_home.map(str::trim).filter(|x| x.starts_with('/')) {
        return Some(Path::new(x).join("autostart"));
    }
    home.map(str::trim).filter(|h| !h.is_empty()).map(|h| Path::new(h).join(".config/autostart"))
}

/// What an existing autostart file says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutostartState {
    /// Would run at login.
    pub on: bool,
    /// Someone disabled it (`Hidden=true` or `X-GNOME-Autostart-enabled=false`).
    pub blocked: bool,
}

/// Reads the `[Desktop Entry]` keys that disable the entry.
pub fn parse_autostart(text: &str) -> AutostartState {
    let mut in_entry = false;
    let mut blocked = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "Hidden" if v.eq_ignore_ascii_case("true") => blocked = true,
                "X-GNOME-Autostart-enabled" if v.eq_ignore_ascii_case("false") => blocked = true,
                _ => {}
            }
        }
    }
    AutostartState { on: !blocked, blocked }
}

// --------------------------------------------------------------- wallpaper

/// A program to run, without a shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub program: &'static str,
    pub args: Vec<String>,
}

fn cmd(program: &'static str, args: &[&str]) -> Cmd {
    Cmd { program, args: args.iter().map(|a| a.to_string()).collect() }
}

/// `file://` URI of an absolute path (percent-encoded).
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Path of a `file://` URI (percent-decoded); other schemes give `None`.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.trim().strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(out).ok()?;
    s.starts_with('/').then(|| PathBuf::from(s))
}

/// `gsettings get` output (`'text'\n`) without the quotes; `@as []`, an
/// empty string and the like give `None`.
pub fn parse_gsettings_string(out: &str) -> Option<String> {
    let inner = out.trim().strip_prefix('\'')?.strip_suffix('\'')?;
    let mut s = String::new();
    let mut esc = false;
    for c in inner.chars() {
        if esc {
            s.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else {
            s.push(c);
        }
    }
    (!s.is_empty()).then_some(s)
}

/// A string for the Plasma scripting engine (JavaScript double quotes).
pub fn js_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Plasma shell script that sets the image: `all` on every desktop, `each`
/// `(screen index, file)` overriding it per screen.
pub fn plasma_script(all: Option<&Path>, each: &[(usize, PathBuf)]) -> String {
    let all = all.map_or("null".to_string(), |p| js_quote(&file_uri(p)));
    let each: Vec<String> = each.iter().map(|(i, p)| format!("\"{i}\": {}", js_quote(&file_uri(p)))).collect();
    format!(
        "var all = {all};\nvar each = {{{}}};\nvar ds = desktops();\n\
         for (var i = 0; i < ds.length; i++) {{\n  var d = ds[i];\n  \
         var u = (each[d.screen] !== undefined) ? each[d.screen] : all;\n  if (u === null) continue;\n  \
         d.wallpaperPlugin = \"org.kde.image\";\n  \
         d.currentConfigGroup = [\"Wallpaper\", \"org.kde.image\", \"General\"];\n  \
         d.writeConfig(\"Image\", u);\n}}\n",
        each.join(", ")
    )
}

/// First `Image=` of the Plasma desktop applets file as a path.
pub fn parse_plasma_image(appletsrc: &str) -> Option<PathBuf> {
    let line = appletsrc.lines().map(str::trim).find(|l| l.starts_with("Image=file://"))?;
    uri_to_path(line.strip_prefix("Image=")?)
}

/// `xfconf-query -c xfce4-desktop -l` lines that hold a monitor's image.
pub fn xfce_image_props(list: &str) -> Vec<String> {
    list.lines()
        .map(str::trim)
        .filter(|l| l.contains("/monitor") && l.ends_with("/last-image"))
        .map(String::from)
        .collect()
}

/// Monitor id of an xfce property: `/backdrop/screen0/monitorHDMI-1/...` is
/// `HDMI-1`, `monitor0` is `0`.
pub fn xfce_prop_monitor(prop: &str) -> Option<&str> {
    let at = prop.find("/monitor")? + "/monitor".len();
    prop[at..].split('/').next().filter(|m| !m.is_empty())
}

/// Commands that set the wallpaper. `items` are `(monitor id, file)`; an id
/// of `None` means every monitor. `xfce_props` come from
/// [`xfce_image_props`]. `feh` says the fallback tool exists.
pub fn wallpaper_cmds(desktop: Desktop, items: &[(Option<&str>, &Path)], xfce_props: &[String], feh: bool) -> Vec<Cmd> {
    let Some(&(_, first)) = items.first() else { return Vec::new() };
    let p = first.to_string_lossy().into_owned();
    let uri = file_uri(first);
    match desktop {
        Desktop::Gnome => vec![
            cmd("gsettings", &["set", "org.gnome.desktop.background", "picture-options", "zoom"]),
            cmd("gsettings", &["set", "org.gnome.desktop.background", "picture-uri", &uri]),
            cmd("gsettings", &["set", "org.gnome.desktop.background", "picture-uri-dark", &uri]),
        ],
        Desktop::Cinnamon => vec![
            cmd("gsettings", &["set", "org.cinnamon.desktop.background", "picture-options", "zoom"]),
            cmd("gsettings", &["set", "org.cinnamon.desktop.background", "picture-uri", &uri]),
        ],
        Desktop::Mate => vec![
            cmd("gsettings", &["set", "org.mate.background", "picture-options", "zoom"]),
            cmd("gsettings", &["set", "org.mate.background", "picture-filename", &p]),
        ],
        Desktop::Xfce => xfce_props
            .iter()
            .filter_map(|prop| {
                let (_, file) = items.iter().find(|(id, _)| id.is_none() || xfce_prop_monitor(prop) == *id)?;
                Some(cmd("xfconf-query", &["-c", "xfce4-desktop", "-p", prop, "-s", &file.to_string_lossy()]))
            })
            .collect(),
        Desktop::Sway => items
            .iter()
            .map(|(id, f)| {
                let q = format!("\"{}\"", f.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""));
                cmd("swaymsg", &["output", id.unwrap_or("*"), "bg", &q, "fill"])
            })
            .collect(),
        Desktop::Hyprland => {
            let mut v = Vec::new();
            for (id, f) in items {
                let f = f.to_string_lossy();
                v.push(cmd("hyprctl", &["hyprpaper", "preload", &f]));
                v.push(cmd("hyprctl", &["hyprpaper", "wallpaper", &format!("{},{f}", id.unwrap_or(""))]));
            }
            v
        }
        // KDE goes over D-Bus (`plasma_script`); nothing to run here.
        Desktop::Kde => Vec::new(),
        Desktop::Other if feh => {
            let mut args = vec!["--bg-fill".to_string()];
            args.extend(items.iter().map(|(_, f)| f.to_string_lossy().into_owned()));
            vec![Cmd { program: "feh", args }]
        }
        Desktop::Other => Vec::new(),
    }
}

/// Command that reads the current wallpaper (`None`: not readable that way).
pub fn wallpaper_read_cmd(desktop: Desktop, dark: bool, xfce_prop: Option<&str>) -> Option<Cmd> {
    match desktop {
        Desktop::Gnome => {
            let key = if dark { "picture-uri-dark" } else { "picture-uri" };
            Some(cmd("gsettings", &["get", "org.gnome.desktop.background", key]))
        }
        Desktop::Cinnamon => Some(cmd("gsettings", &["get", "org.cinnamon.desktop.background", "picture-uri"])),
        Desktop::Mate => Some(cmd("gsettings", &["get", "org.mate.background", "picture-filename"])),
        Desktop::Xfce => xfce_prop.map(|p| cmd("xfconf-query", &["-c", "xfce4-desktop", "-p", p])),
        Desktop::Hyprland => Some(cmd("hyprctl", &["hyprpaper", "listactive"])),
        _ => None,
    }
}

/// Current wallpaper from the output of [`wallpaper_read_cmd`].
pub fn parse_wallpaper_read(desktop: Desktop, out: &str) -> Option<PathBuf> {
    match desktop {
        Desktop::Gnome | Desktop::Cinnamon => uri_to_path(&parse_gsettings_string(out)?),
        Desktop::Mate => parse_gsettings_string(out).map(PathBuf::from),
        Desktop::Xfce => out.lines().map(str::trim).find(|l| l.starts_with('/')).map(PathBuf::from),
        // "HDMI-A-1 = /path/to.png"
        Desktop::Hyprland => out
            .lines()
            .find_map(|l| l.split_once('=').map(|(_, f)| f.trim()))
            .filter(|f| f.starts_with('/'))
            .map(PathBuf::from),
        _ => None,
    }
}

/// Outputs `(name, width, height)` from `swaymsg -t get_outputs -r`.
pub fn parse_sway_outputs(json: &str) -> Vec<(String, u32, u32)> {
    let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    a.iter()
        .filter(|o| o["active"].as_bool().unwrap_or(true))
        .filter_map(|o| {
            let n = o["name"].as_str()?;
            let w = o["rect"]["width"].as_u64().unwrap_or(0);
            let h = o["rect"]["height"].as_u64().unwrap_or(0);
            Some((n.to_string(), w as u32, h as u32))
        })
        .collect()
}

/// Outputs `(name, width, height)` from `hyprctl monitors -j`.
pub fn parse_hypr_monitors(json: &str) -> Vec<(String, u32, u32)> {
    let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    a.iter()
        .filter_map(|o| {
            let n = o["name"].as_str()?;
            Some((n.to_string(), o["width"].as_u64().unwrap_or(0) as u32, o["height"].as_u64().unwrap_or(0) as u32))
        })
        .collect()
}

/// The desktop can change the wallpaper, given which tools exist.
pub fn wallpaper_supported(desktop: Desktop, has: impl Fn(&str) -> bool, plasma_on_bus: bool) -> bool {
    match desktop {
        Desktop::Gnome | Desktop::Cinnamon | Desktop::Mate => has("gsettings"),
        Desktop::Kde => plasma_on_bus,
        Desktop::Xfce => has("xfconf-query"),
        Desktop::Sway => has("swaymsg"),
        Desktop::Hyprland => has("hyprctl"),
        Desktop::Other => has("feh"),
    }
}

// ------------------------------------------------------------------- D-Bus

/// Well-known names, paths and interfaces.
pub mod dbus {
    pub const LOGIN: &str = "org.freedesktop.login1";
    pub const LOGIN_PATH: &str = "/org/freedesktop/login1";
    pub const LOGIN_MANAGER: &str = "org.freedesktop.login1.Manager";
    pub const LOGIN_SESSION: &str = "org.freedesktop.login1.Session";
    pub const UPOWER: &str = "org.freedesktop.UPower";
    pub const UPOWER_PATH: &str = "/org/freedesktop/UPower";
    pub const UPOWER_DEVICE: &str = "org.freedesktop.UPower.Device";
    pub const UPOWER_DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
    pub const PPD: &str = "net.hadess.PowerProfiles";
    pub const PPD_PATH: &str = "/net/hadess/PowerProfiles";
    pub const PPD_NEW: &str = "org.freedesktop.UPower.PowerProfiles";
    pub const PPD_NEW_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";
    pub const PORTAL: &str = "org.freedesktop.portal.Desktop";
    pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
    pub const PORTAL_SETTINGS: &str = "org.freedesktop.portal.Settings";
    pub const PROPS: &str = "org.freedesktop.DBus.Properties";
    pub const PLASMA: &str = "org.kde.plasmashell";
    pub const PLASMA_PATH: &str = "/PlasmaShell";
    pub const PLASMA_IFACE: &str = "org.kde.PlasmaShell";
    pub const KWIN: &str = "org.kde.KWin";
    pub const APPEARANCE: &str = "org.freedesktop.appearance";
    pub const GNOME_INTERFACE: &str = "org.gnome.desktop.interface";
    pub const KDE_GLOBALS: &str = "org.kde.kdeglobals.KDE";

    /// Match rules for the system bus; `session_path` is our logind session.
    pub fn system_rules(session_path: Option<&str>) -> Vec<String> {
        let mut v = vec![
            format!("type='signal',interface='{LOGIN_MANAGER}',member='PrepareForSleep'"),
            format!("type='signal',interface='{PROPS}',path='{UPOWER_PATH}'"),
            format!("type='signal',interface='{PROPS}',path='{UPOWER_DISPLAY}'"),
            format!("type='signal',interface='{PROPS}',path='{PPD_PATH}'"),
            format!("type='signal',interface='{PROPS}',path='{PPD_NEW_PATH}'"),
        ];
        if let Some(p) = session_path {
            v.push(format!("type='signal',interface='{LOGIN_SESSION}',path='{p}'"));
            v.push(format!("type='signal',interface='{PROPS}',path='{p}'"));
        }
        v
    }

    /// Match rules for the session bus.
    pub fn session_rules() -> Vec<String> {
        vec![
            format!("type='signal',interface='{PORTAL_SETTINGS}',member='SettingChanged'"),
            "type='signal',member='ActiveChanged'".to_string(),
        ]
    }
}

/// A decoded D-Bus value, as far as the services care.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Bool(bool),
    U32(u32),
    F64(f64),
    Str(String),
}

/// A signal the watcher threads decode.
#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    PrepareForSleep(bool),
    Lock,
    Unlock,
    /// `PropertiesChanged` of `iface`, changed values only.
    Props {
        iface: String,
        changed: Vec<(String, Val)>,
    },
    /// A screensaver `ActiveChanged`.
    ScreenSaver(bool),
    /// Portal `SettingChanged`.
    Portal {
        ns: String,
        key: String,
        value: Val,
    },
}

/// A change of the cached session facts.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    OnBattery(bool),
    HasBattery(bool),
    PowerSaver(bool),
    /// Portal `color-scheme`: 0 none, 1 dark, 2 light.
    ColorScheme(u32),
    ReducedMotion(bool),
}

/// What one signal means: the events for the app and the cached facts that
/// change with it.
pub fn react(sig: &Signal) -> (Vec<SystemEvent>, Vec<Change>) {
    let mut ev = Vec::new();
    let mut ch = Vec::new();
    match sig {
        Signal::PrepareForSleep(true) => ev.push(SystemEvent::Suspend),
        Signal::PrepareForSleep(false) => ev.push(SystemEvent::Resume),
        Signal::Lock => ev.push(SystemEvent::Locked),
        Signal::Unlock => ev.push(SystemEvent::Unlocked),
        Signal::ScreenSaver(true) => ev.push(SystemEvent::DisplayOff),
        Signal::ScreenSaver(false) => ev.push(SystemEvent::DisplayOn),
        Signal::Props { iface, changed } => {
            for (k, v) in changed {
                match (iface.as_str(), k.as_str(), v) {
                    (dbus::LOGIN_SESSION, "LockedHint", Val::Bool(true)) => ev.push(SystemEvent::Locked),
                    (dbus::LOGIN_SESSION, "LockedHint", Val::Bool(false)) => ev.push(SystemEvent::Unlocked),
                    (dbus::UPOWER, "OnBattery", Val::Bool(b)) => {
                        ch.push(Change::OnBattery(*b));
                        ev.push(SystemEvent::PowerChanged);
                    }
                    (dbus::UPOWER_DEVICE, "IsPresent", Val::Bool(b)) => {
                        ch.push(Change::HasBattery(*b));
                        ev.push(SystemEvent::PowerChanged);
                    }
                    (dbus::PPD | dbus::PPD_NEW, "ActiveProfile", Val::Str(s)) => {
                        ch.push(Change::PowerSaver(s == "power-saver"));
                        ev.push(SystemEvent::PowerChanged);
                    }
                    _ => {}
                }
            }
        }
        Signal::Portal { ns, key, value } => {
            if let Some(c) = portal_change(ns, key, value) {
                if matches!(c, Change::ColorScheme(_)) {
                    ev.push(SystemEvent::ThemeChanged);
                }
                ch.push(c);
            }
        }
    }
    (ev, ch)
}

/// Portal setting to cached fact.
pub fn portal_change(ns: &str, key: &str, value: &Val) -> Option<Change> {
    match (ns, key, value) {
        (dbus::APPEARANCE, "color-scheme", Val::U32(n)) => Some(Change::ColorScheme(*n)),
        (dbus::GNOME_INTERFACE, "enable-animations", Val::Bool(on)) => Some(Change::ReducedMotion(!on)),
        (dbus::KDE_GLOBALS, "AnimationDurationFactor", Val::F64(f)) => Some(Change::ReducedMotion(*f <= 0.0)),
        _ => None,
    }
}

/// The tray should draw dark icons: the portal says the colour scheme is light.
pub fn scheme_is_light(color_scheme: u32) -> bool {
    color_scheme == 2
}

/// Only object paths below logind's session tree go into a match rule.
pub fn valid_session_path(p: &str) -> bool {
    let tail = p.strip_prefix("/org/freedesktop/login1/session/");
    tail.is_some_and(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// A Plasma `evaluateScript` reply that reports a script error.
pub fn plasma_reply_ok(reply: &str) -> bool {
    !reply.to_ascii_lowercase().contains("error")
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

    #[test]
    fn instance_path_choice() {
        let (l, s) = instance_paths(Some("/run/user/1000"), 1000);
        assert_eq!(l, Path::new("/run/user/1000/frostpane.lock"));
        assert_eq!(s, Path::new("/run/user/1000/frostpane.sock"));
        let (l, s) = instance_paths(Some("relative"), 7);
        assert_eq!(l, Path::new("/tmp/frostpane-7.lock"));
        assert_eq!(s, Path::new("/tmp/frostpane-7.sock"));
        assert_eq!(instance_paths(None, 7).1, Path::new("/tmp/frostpane-7.sock"));
        assert!(is_open_message("open\n"));
        assert!(!is_open_message("close"));
        assert_eq!(OPEN_MESSAGE, b"open\n");
    }

    #[test]
    fn desktops() {
        let d = |c, s| detect_desktop(c, s, false, false);
        assert_eq!(d(Some("ubuntu:GNOME"), None), Desktop::Gnome);
        assert_eq!(d(Some("Budgie:GNOME"), None), Desktop::Gnome);
        assert_eq!(d(Some("X-Cinnamon"), None), Desktop::Cinnamon);
        assert_eq!(d(Some("MATE"), None), Desktop::Mate);
        assert_eq!(d(Some("KDE"), None), Desktop::Kde);
        assert_eq!(d(Some("XFCE"), None), Desktop::Xfce);
        assert_eq!(d(Some("sway"), None), Desktop::Sway);
        assert_eq!(d(Some("Hyprland"), None), Desktop::Hyprland);
        assert_eq!(d(None, Some("plasma")), Desktop::Kde);
        assert_eq!(d(Some("LXQt"), Some("lxqt")), Desktop::Other);
        assert_eq!(d(None, None), Desktop::Other);
        assert_eq!(detect_desktop(None, None, true, false), Desktop::Sway);
        assert_eq!(detect_desktop(Some("i3"), None, true, true), Desktop::Hyprland);
    }

    #[test]
    fn small_helpers() {
        let have = |p: &Path| p == Path::new("/usr/bin/feh");
        assert_eq!(find_in_path("feh", "/bin::/usr/bin", have), Some(PathBuf::from("/usr/bin/feh")));
        assert_eq!(find_in_path("feh", "/bin", have), None);
        assert!(url_allowed(" HTTPS://x.org"));
        assert!(!url_allowed("http://x.org"));
        assert!(!url_allowed("file:///etc/passwd"));
        assert!(!url_allowed("-o"));
        assert_eq!(uptime_ms("12.50 99.0\n"), Some(12_500));
        assert_eq!(uptime_ms("x"), None);
        assert_eq!(uptime_ms("-1 0"), None);
        assert_eq!(uptime_ms(""), None);
    }

    #[test]
    fn autostart_text() {
        let t = autostart_entry("/opt/Frost Pane/frostpane");
        assert!(t.starts_with("[Desktop Entry]\n"));
        assert!(t.contains("Exec=\"/opt/Frost Pane/frostpane\" --quiet\n"));
        assert!(t.contains("X-GNOME-Autostart-enabled=true\n"));
        assert!(autostart_entry("/usr/bin/frostpane").contains("Exec=/usr/bin/frostpane --quiet\n"));
        assert_eq!(exec_quote("a$b"), "\"a\\$b\"");
        assert_eq!(exec_quote("50%"), "50%%");
        assert_eq!(exec_quote(""), "\"\"");
        assert_eq!(exec_quote("q\"`\\"), "\"q\\\"\\`\\\\\"");
        assert_eq!(parse_autostart(&t), AutostartState { on: true, blocked: false });
        let hidden = format!("{t}Hidden=true\n");
        assert_eq!(parse_autostart(&hidden), AutostartState { on: false, blocked: true });
        let gnome = "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n";
        assert!(parse_autostart(gnome).blocked);
        // Keys of other groups do not count.
        let other = "[Desktop Entry]\nName=x\n[Desktop Action a]\nHidden=true\n";
        assert!(!parse_autostart(other).blocked);
        assert!(!parse_autostart("junk").blocked);
    }

    #[test]
    fn autostart_dirs() {
        assert_eq!(autostart_dir(Some("/c"), Some("/h")), Some(PathBuf::from("/c/autostart")));
        assert_eq!(autostart_dir(Some("c"), Some("/h")), Some(PathBuf::from("/h/.config/autostart")));
        assert_eq!(autostart_dir(None, Some("")), None);
    }

    #[test]
    fn uris() {
        let p = Path::new("/home/u/My Pics/a#b.png");
        let uri = file_uri(p);
        assert_eq!(uri, "file:///home/u/My%20Pics/a%23b.png");
        assert_eq!(uri_to_path(&uri), Some(p.to_path_buf()));
        assert_eq!(uri_to_path("file://localhost/x"), Some(PathBuf::from("/x")));
        assert_eq!(uri_to_path("https://x"), None);
        assert_eq!(uri_to_path("file://rel"), None);
        assert_eq!(uri_to_path("file:///a%2"), None);
        assert_eq!(uri_to_path("file:///a%zz"), None);
        assert_eq!(uri_to_path("file:///a%ff"), None);
        assert_eq!(parse_gsettings_string("'file:///a.png'\n"), Some("file:///a.png".into()));
        assert_eq!(parse_gsettings_string("'it\\'s'"), Some("it's".into()));
        assert_eq!(parse_gsettings_string("''"), None);
        assert_eq!(parse_gsettings_string("@as []"), None);
    }

    #[test]
    fn plasma() {
        assert_eq!(js_quote("a\"b\\c\n\r\u{1}"), "\"a\\\"b\\\\c\\n\\r\\u0001\"");
        let s = plasma_script(Some(Path::new("/w/a.png")), &[(1, PathBuf::from("/w/b.png"))]);
        assert!(s.contains("var all = \"file:///w/a.png\";"));
        assert!(s.contains("var each = {\"1\": \"file:///w/b.png\"};"));
        assert!(s.contains("writeConfig(\"Image\", u)"));
        assert!(plasma_script(None, &[]).contains("var all = null;\nvar each = {};"));
        let rc = "[Containments][1][Wallpaper][org.kde.image][General]\nImage=file:///w/a%20b.png\n";
        assert_eq!(parse_plasma_image(rc), Some(PathBuf::from("/w/a b.png")));
        assert_eq!(parse_plasma_image("Image=Next\n"), None);
        assert!(plasma_reply_ok(""));
        assert!(!plasma_reply_ok("file:1: ReferenceError: x"));
    }

    #[test]
    fn xfce_props() {
        let list = "/backdrop/screen0/monitor0/workspace0/last-image\n\
                    /backdrop/screen0/monitorHDMI-1/workspace0/last-image\n\
                    /backdrop/screen0/monitor0/workspace0/image-style\n/other\n";
        let props = xfce_image_props(list);
        assert_eq!(props.len(), 2);
        assert_eq!(xfce_prop_monitor(&props[0]), Some("0"));
        assert_eq!(xfce_prop_monitor(&props[1]), Some("HDMI-1"));
        assert_eq!(xfce_prop_monitor("/x"), None);
        assert_eq!(xfce_prop_monitor("/x/monitor/y"), None);
        let a = Path::new("/w/a.png");
        let b = Path::new("/w/b.png");
        let all = wallpaper_cmds(Desktop::Xfce, &[(None, a)], &props, false);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].program, "xfconf-query");
        assert_eq!(all[1].args, ["-c", "xfce4-desktop", "-p", &props[1], "-s", "/w/a.png"]);
        let each = wallpaper_cmds(Desktop::Xfce, &[(Some("HDMI-1"), b)], &props, false);
        assert_eq!(each.len(), 1);
        assert_eq!(each[0].args.last().map(String::as_str), Some("/w/b.png"));
        assert!(wallpaper_cmds(Desktop::Xfce, &[(None, a)], &[], false).is_empty());
    }

    #[test]
    fn wallpaper_commands() {
        let a = Path::new("/w/a b.png");
        let items = [(None, a)];
        let g = wallpaper_cmds(Desktop::Gnome, &items, &[], false);
        assert_eq!(g.len(), 3);
        assert_eq!(g[1].args, ["set", "org.gnome.desktop.background", "picture-uri", "file:///w/a%20b.png"]);
        assert_eq!(g[2].args[2], "picture-uri-dark");
        let c = wallpaper_cmds(Desktop::Cinnamon, &items, &[], false);
        assert_eq!(c[1].args[1], "org.cinnamon.desktop.background");
        let m = wallpaper_cmds(Desktop::Mate, &items, &[], false);
        assert_eq!(m[1].args, ["set", "org.mate.background", "picture-filename", "/w/a b.png"]);
        let s = wallpaper_cmds(Desktop::Sway, &items, &[], false);
        assert_eq!(s[0].program, "swaymsg");
        assert_eq!(s[0].args, ["output", "*", "bg", "\"/w/a b.png\"", "fill"]);
        let s = wallpaper_cmds(Desktop::Sway, &[(Some("DP-1"), Path::new("/w/\"q\".png"))], &[], false);
        assert_eq!(s[0].args[1], "DP-1");
        assert_eq!(s[0].args[3], "\"/w/\\\"q\\\".png\"");
        let h = wallpaper_cmds(Desktop::Hyprland, &[(None, a), (Some("DP-1"), a)], &[], false);
        assert_eq!(h.len(), 4);
        assert_eq!(h[1].args, ["hyprpaper", "wallpaper", ",/w/a b.png"]);
        assert_eq!(h[3].args, ["hyprpaper", "wallpaper", "DP-1,/w/a b.png"]);
        let f = wallpaper_cmds(Desktop::Other, &[(None, a), (None, Path::new("/w/c.png"))], &[], true);
        assert_eq!(f, [Cmd { program: "feh", args: vec!["--bg-fill".into(), "/w/a b.png".into(), "/w/c.png".into()] }]);
        assert!(wallpaper_cmds(Desktop::Other, &items, &[], false).is_empty());
        assert!(wallpaper_cmds(Desktop::Kde, &items, &[], true).is_empty());
        assert!(wallpaper_cmds(Desktop::Gnome, &[], &[], true).is_empty());
    }

    #[test]
    fn wallpaper_reads() {
        let g = wallpaper_read_cmd(Desktop::Gnome, true, None).unwrap();
        assert_eq!(g.args[2], "picture-uri-dark");
        assert_eq!(wallpaper_read_cmd(Desktop::Gnome, false, None).unwrap().args[2], "picture-uri");
        assert!(wallpaper_read_cmd(Desktop::Cinnamon, false, None).is_some());
        assert!(wallpaper_read_cmd(Desktop::Mate, false, None).is_some());
        assert!(wallpaper_read_cmd(Desktop::Xfce, false, None).is_none());
        assert!(wallpaper_read_cmd(Desktop::Xfce, false, Some("/p")).is_some());
        assert!(wallpaper_read_cmd(Desktop::Hyprland, false, None).is_some());
        assert!(wallpaper_read_cmd(Desktop::Sway, false, None).is_none());
        let want = Some(PathBuf::from("/w/a b.png"));
        assert_eq!(parse_wallpaper_read(Desktop::Gnome, "'file:///w/a%20b.png'\n"), want);
        assert_eq!(parse_wallpaper_read(Desktop::Cinnamon, "'file:///w/a%20b.png'"), want);
        assert_eq!(parse_wallpaper_read(Desktop::Mate, "'/w/a b.png'"), want);
        assert_eq!(parse_wallpaper_read(Desktop::Xfce, "/w/a b.png\n"), want);
        assert_eq!(parse_wallpaper_read(Desktop::Hyprland, "DP-1 = /w/a b.png\n"), want);
        assert_eq!(parse_wallpaper_read(Desktop::Hyprland, "no wallpapers active"), None);
        assert_eq!(parse_wallpaper_read(Desktop::Kde, "x"), None);
    }

    #[test]
    fn outputs() {
        let sway = r#"[{"name":"DP-1","active":true,"rect":{"x":0,"y":0,"width":1920,"height":1080}},
                       {"name":"HDMI-A-1","active":false,"rect":{"width":1,"height":1}}]"#;
        assert_eq!(parse_sway_outputs(sway), [("DP-1".to_string(), 1920, 1080)]);
        assert!(parse_sway_outputs("nope").is_empty());
        let hypr = r#"[{"name":"eDP-1","width":2560,"height":1600}]"#;
        assert_eq!(parse_hypr_monitors(hypr), [("eDP-1".to_string(), 2560, 1600)]);
        assert!(parse_hypr_monitors("{}").is_empty());
    }

    #[test]
    fn support() {
        let only = |n: &'static str| move |t: &str| t == n;
        assert!(wallpaper_supported(Desktop::Gnome, only("gsettings"), false));
        assert!(!wallpaper_supported(Desktop::Gnome, only("feh"), false));
        assert!(wallpaper_supported(Desktop::Cinnamon, only("gsettings"), false));
        assert!(wallpaper_supported(Desktop::Mate, only("gsettings"), false));
        assert!(wallpaper_supported(Desktop::Kde, |_| false, true));
        assert!(!wallpaper_supported(Desktop::Kde, |_| true, false));
        assert!(wallpaper_supported(Desktop::Xfce, only("xfconf-query"), false));
        assert!(wallpaper_supported(Desktop::Sway, only("swaymsg"), false));
        assert!(wallpaper_supported(Desktop::Hyprland, only("hyprctl"), false));
        assert!(wallpaper_supported(Desktop::Other, only("feh"), false));
        assert!(!wallpaper_supported(Desktop::Other, |_| false, true));
    }

    fn props(iface: &str, k: &str, v: Val) -> Signal {
        Signal::Props { iface: iface.into(), changed: vec![(k.into(), v)] }
    }

    #[test]
    fn signals_to_events() {
        use SystemEvent::*;
        assert_eq!(react(&Signal::PrepareForSleep(true)).0, [Suspend]);
        assert_eq!(react(&Signal::PrepareForSleep(false)).0, [Resume]);
        assert_eq!(react(&Signal::Lock).0, [Locked]);
        assert_eq!(react(&Signal::Unlock).0, [Unlocked]);
        assert_eq!(react(&Signal::ScreenSaver(true)).0, [DisplayOff]);
        assert_eq!(react(&Signal::ScreenSaver(false)).0, [DisplayOn]);
        assert_eq!(react(&props(dbus::LOGIN_SESSION, "LockedHint", Val::Bool(true))).0, [Locked]);
        assert_eq!(react(&props(dbus::LOGIN_SESSION, "LockedHint", Val::Bool(false))).0, [Unlocked]);
        assert_eq!(
            react(&props(dbus::UPOWER, "OnBattery", Val::Bool(true))),
            (vec![PowerChanged], vec![Change::OnBattery(true)])
        );
        assert_eq!(react(&props(dbus::UPOWER_DEVICE, "IsPresent", Val::Bool(false))).1, [Change::HasBattery(false)]);
        assert_eq!(
            react(&props(dbus::PPD, "ActiveProfile", Val::Str("power-saver".into()))).1,
            [Change::PowerSaver(true)]
        );
        assert_eq!(
            react(&props(dbus::PPD_NEW, "ActiveProfile", Val::Str("balanced".into()))).1,
            [Change::PowerSaver(false)]
        );
        // Unrelated properties and wrong value types do nothing.
        assert_eq!(react(&props(dbus::UPOWER, "LidIsClosed", Val::Bool(true))), (vec![], vec![]));
        assert_eq!(react(&props(dbus::UPOWER, "OnBattery", Val::U32(1))), (vec![], vec![]));
    }

    #[test]
    fn portal_values() {
        let dark = Signal::Portal { ns: dbus::APPEARANCE.into(), key: "color-scheme".into(), value: Val::U32(1) };
        assert_eq!(react(&dark), (vec![SystemEvent::ThemeChanged], vec![Change::ColorScheme(1)]));
        let anim = |v| portal_change(dbus::GNOME_INTERFACE, "enable-animations", &v);
        assert_eq!(anim(Val::Bool(false)), Some(Change::ReducedMotion(true)));
        assert_eq!(anim(Val::Bool(true)), Some(Change::ReducedMotion(false)));
        let kde = |f| portal_change(dbus::KDE_GLOBALS, "AnimationDurationFactor", &Val::F64(f));
        assert_eq!(kde(0.0), Some(Change::ReducedMotion(true)));
        assert_eq!(kde(1.0), Some(Change::ReducedMotion(false)));
        assert_eq!(portal_change("x", "y", &Val::Bool(true)), None);
        let motion = Signal::Portal {
            ns: dbus::GNOME_INTERFACE.into(),
            key: "enable-animations".into(),
            value: Val::Bool(false),
        };
        assert_eq!(react(&motion), (vec![], vec![Change::ReducedMotion(true)]));
        assert!(scheme_is_light(2));
        assert!(!scheme_is_light(1) && !scheme_is_light(0));
    }

    #[test]
    fn rules_and_paths() {
        assert!(valid_session_path("/org/freedesktop/login1/session/_32"));
        assert!(!valid_session_path("/org/freedesktop/login1/session/"));
        assert!(!valid_session_path("/org/freedesktop/login1/session/a'b"));
        assert!(!valid_session_path("/elsewhere"));
        let r = dbus::system_rules(Some("/org/freedesktop/login1/session/_32"));
        assert!(r.iter().any(|x| x.contains("PrepareForSleep")));
        assert!(r.iter().any(|x| x.contains("session/_32") && x.contains("login1.Session")));
        assert_eq!(dbus::system_rules(None).len(), 5);
        assert_eq!(dbus::session_rules().len(), 2);
    }
}
