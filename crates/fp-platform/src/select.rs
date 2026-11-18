//! Which desktop backend a Linux session gets (porting design 2.3). Pure: the
//! environment and the probes come in as closures.

/// Window source of the Wayland backend (foreign window geometry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowSource {
    /// KWin script reporting over D-Bus.
    KWin,
    /// Hyprland IPC socket.
    Hyprland,
    /// sway (i3) IPC.
    Sway,
    /// None: ground-only snow.
    None,
}

/// The decision of [`linux`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Native Wayland with layer-shell panels.
    Wayland { source: WindowSource },
    /// X11. `xwayland`: a Wayland session without layer shell, so only X11
    /// clients become ledges (degraded).
    X11 { xwayland: bool },
    /// No display server reachable.
    Nothing,
}

impl Choice {
    /// The egui windows must run on X11 (winit forced to X11): always with the
    /// X11 backend, and on request (`FROSTPANE_UI=x11`) with Wayland.
    pub fn ui_on_x11(&self, env: &dyn Fn(&str) -> Option<String>) -> bool {
        match self {
            Choice::X11 { .. } => true,
            Choice::Wayland { .. } => env("FROSTPANE_UI").is_some_and(|v| v.trim().eq_ignore_ascii_case("x11")),
            Choice::Nothing => false,
        }
    }
}

/// Probes the selection may need; each runs at most once and only when its
/// answer matters.
pub struct Probes<'a> {
    /// The Wayland compositor offers `zwlr_layer_shell_v1` (version >= 3).
    pub layer_shell: &'a mut dyn FnMut() -> bool,
    /// `org.kde.KWin` is on the session bus.
    pub kwin: &'a mut dyn FnMut() -> bool,
}

fn set(env: &dyn Fn(&str) -> Option<String>, key: &str) -> bool {
    env(key).is_some_and(|v| !v.trim().is_empty())
}

fn source(env: &dyn Fn(&str) -> Option<String>, kwin: &mut dyn FnMut() -> bool) -> WindowSource {
    let kde = env("XDG_CURRENT_DESKTOP").is_some_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")));
    if kde && kwin() {
        WindowSource::KWin
    } else if set(env, "HYPRLAND_INSTANCE_SIGNATURE") {
        WindowSource::Hyprland
    } else if set(env, "SWAYSOCK") {
        WindowSource::Sway
    } else {
        WindowSource::None
    }
}

/// Design 2.3: `FROSTPANE_BACKEND=x11|wayland` wins; a Wayland session with
/// layer shell gets the Wayland backend (window source by desktop); without
/// layer shell X11 over XWayland when `DISPLAY` is set; plain X11 when only
/// `DISPLAY` is set; else nothing.
pub fn linux(env: &dyn Fn(&str) -> Option<String>, probes: Probes<'_>) -> Choice {
    let wayland_session = set(env, "WAYLAND_DISPLAY")
        || env("XDG_SESSION_TYPE").is_some_and(|t| t.trim().eq_ignore_ascii_case("wayland"));
    let display = set(env, "DISPLAY");
    match env("FROSTPANE_BACKEND").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("x11") => return Choice::X11 { xwayland: wayland_session },
        Some("wayland") => return Choice::Wayland { source: source(env, probes.kwin) },
        _ => {}
    }
    if wayland_session && (probes.layer_shell)() {
        return Choice::Wayland { source: source(env, probes.kwin) };
    }
    if display {
        return Choice::X11 { xwayland: wayland_session };
    }
    Choice::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn run(vars: &[(&str, &str)], layer_shell: bool, kwin: bool) -> (Choice, usize) {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let env = move |k: &str| map.get(k).cloned();
        let mut probed = 0;
        let mut ls = || {
            probed += 1;
            layer_shell
        };
        let mut kw = || kwin;
        let c = linux(&env, Probes { layer_shell: &mut ls, kwin: &mut kw });
        (c, probed)
    }

    #[test]
    fn override_wins() {
        assert_eq!(
            run(&[("FROSTPANE_BACKEND", "X11"), ("WAYLAND_DISPLAY", "w")], true, false).0,
            Choice::X11 { xwayland: true }
        );
        let (c, probed) = run(&[("FROSTPANE_BACKEND", " wayland "), ("SWAYSOCK", "/s")], false, false);
        assert_eq!(c, Choice::Wayland { source: WindowSource::Sway });
        assert_eq!(probed, 0);
        // Unknown values fall through to detection.
        assert_eq!(
            run(&[("FROSTPANE_BACKEND", "mir"), ("DISPLAY", ":0")], true, false).0,
            Choice::X11 { xwayland: false }
        );
    }

    #[test]
    fn wayland_sources() {
        let kde = [("WAYLAND_DISPLAY", "wayland-0"), ("XDG_CURRENT_DESKTOP", "KDE")];
        assert_eq!(run(&kde, true, true).0, Choice::Wayland { source: WindowSource::KWin });
        // KDE without KWin on the bus: no source.
        assert_eq!(run(&kde, true, false).0, Choice::Wayland { source: WindowSource::None });
        let hypr = [("XDG_SESSION_TYPE", "wayland"), ("HYPRLAND_INSTANCE_SIGNATURE", "abc")];
        assert_eq!(run(&hypr, true, false).0, Choice::Wayland { source: WindowSource::Hyprland });
        let mixed = [("WAYLAND_DISPLAY", "w"), ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"), ("SWAYSOCK", " ")];
        assert_eq!(run(&mixed, true, true).0, Choice::Wayland { source: WindowSource::None });
    }

    #[test]
    fn fallbacks() {
        // GNOME / Weston: no layer shell, XWayland available.
        let (c, probed) = run(&[("WAYLAND_DISPLAY", "w"), ("DISPLAY", ":0")], false, false);
        assert_eq!(c, Choice::X11 { xwayland: true });
        assert_eq!(probed, 1);
        // Plain X11 never probes Wayland.
        let (c, probed) = run(&[("DISPLAY", ":1"), ("XDG_SESSION_TYPE", "x11")], true, false);
        assert_eq!(c, Choice::X11 { xwayland: false });
        assert_eq!(probed, 0);
        assert_eq!(run(&[("WAYLAND_DISPLAY", "w")], false, false).0, Choice::Nothing);
        assert_eq!(run(&[("DISPLAY", "")], true, false).0, Choice::Nothing);
    }

    #[test]
    fn ui_backend() {
        let none = |_: &str| None;
        let x11 = |k: &str| (k == "FROSTPANE_UI").then(|| "X11".to_string());
        assert!(Choice::X11 { xwayland: false }.ui_on_x11(&none));
        assert!(!Choice::Wayland { source: WindowSource::None }.ui_on_x11(&none));
        assert!(Choice::Wayland { source: WindowSource::KWin }.ui_on_x11(&x11));
        assert!(!Choice::Nothing.ui_on_x11(&x11));
        assert!(format!("{:?}", Choice::Nothing).contains("Nothing"));
    }
}
