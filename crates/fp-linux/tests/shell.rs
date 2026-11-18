//! Autostart file, `xdg-open` and per-desktop wallpaper commands against
//! `PATH` shims that only log their arguments. One test: it sets process
//! environment variables before any other thread exists.
#![cfg(target_os = "linux")]

use fp_linux::Services;
use fp_platform::{Platform, Shell};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

fn shim(dir: &Path, name: &str, log: &Path) {
    let p = dir.join(name);
    let script = format!("#!/bin/sh\necho \"{name} $*\" >> \"{}\"\n", log.display());
    std::fs::write(&p, script).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn lines(log: &Path, want: usize) -> Vec<String> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        let v: Vec<String> = text.lines().map(String::from).collect();
        if v.len() >= want || Instant::now() > end {
            return v;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn shell_services() {
    let root = std::env::temp_dir().join(format!("fp-linux-shell-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = root.join("log.txt");
    for n in ["xdg-open", "gsettings"] {
        shim(&bin, n, &log);
    }
    // SAFETY: no other thread runs yet (this is the only test of the binary
    // and no service has been created).
    unsafe {
        std::env::set_var("PATH", &bin);
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("XDG_CURRENT_DESKTOP", "ubuntu:GNOME");
        std::env::remove_var("APPIMAGE");
        std::env::set_var("LANG", "tr_TR.UTF-8");
        std::env::remove_var("LC_ALL");
        std::env::remove_var("LC_MESSAGES");
    }
    let s = Services::new();
    assert_eq!(s.os_language(), "tr");
    assert!(s.uptime_ms() > 0 && s.uptime_ms() < u64::MAX);

    // Autostart: off, on, blocked by a user edit, off again.
    let file = root.join("config/autostart/frostpane.desktop");
    assert!(!s.login_is_on() && !s.login_blocked());
    assert!(s.login_set(true));
    assert!(s.login_is_on());
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("--quiet") && text.contains("Exec="));
    assert!(s.login_set(false));
    assert!(!file.exists() && !s.login_is_on());
    std::fs::write(&file, "[Desktop Entry]\nHidden=true\n").unwrap();
    assert!(s.login_blocked() && !s.login_is_on());
    assert!(!s.login_set(true));
    assert!(s.login_set(false) && file.exists());
    std::fs::remove_file(&file).unwrap();

    // Links: only https is opened.
    s.open_url("http://example.org");
    s.open_url("file:///etc/passwd");
    s.open_url("https://example.org/a b");
    s.open_folder(Path::new("/tmp"));
    let v = lines(&log, 2);
    assert_eq!(v.len(), 2, "{v:?}");
    assert!(v.contains(&"xdg-open https://example.org/a b".to_string()));
    assert!(v.contains(&"xdg-open /tmp".to_string()));

    // Wallpaper on GNOME: gsettings picture-uri and picture-uri-dark.
    assert!(s.wallpaper_supported());
    let png = root.join("a b.png");
    assert!(s.wallpaper_set(&png));
    assert!(!s.wallpaper_set(Path::new("relative.png")));
    let v = lines(&log, 5);
    assert!(v.iter().any(|l| l.contains("picture-uri file://") && l.contains("a%20b.png")), "{v:?}");
    assert!(v.iter().any(|l| l.contains("picture-uri-dark")), "{v:?}");
    assert_eq!(s.wallpaper_screens().len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}
