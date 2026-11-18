//! In-app updater, platform side: how this copy was installed, what gets
//! replaced, and the file swap itself. The swap is plain renames in the
//! target's own folder, so it is the same on every OS; unpacking a release
//! file and starting the new copy differ and live behind [`Updater`]
//! (`fp-win`, `fp-mac`, `fp-linux`).
//!
//! Swap: the new copy is staged next to the target (`<target>.new`), the
//! running one is renamed aside (`<target>.old`; Windows allows renaming a
//! running exe, macOS and Linux keep the open inode), the staged one is
//! renamed into place, and the new process removes the aside copy once the
//! old one has exited ([`clean`]). Any failure before the new process runs
//! is rolled back ([`unswap`]).

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

/// How this copy of Frostpane was installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// Portable `frostpane.exe` from the Windows zip.
    WindowsExe,
    /// `Frostpane.app` bundle.
    MacApp,
    /// A Linux AppImage (`$APPIMAGE`).
    AppImage,
    /// The plain Linux binary from the tar.gz.
    LinuxBinary,
    /// Installed by a package manager (.deb, distro packages): updates come
    /// from there.
    LinuxPackage,
    /// Anything else (other OS, a development build outside a bundle).
    Unsupported,
}

/// Why this copy cannot replace itself; the app opens the release page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocked {
    /// Managed by a package manager.
    Package,
    /// The folder holding the app is not writable (Program Files,
    /// /Applications without rights, a read-only mount).
    ReadOnly,
    /// No self-update for this kind of install.
    Unsupported,
}

/// What a self-update replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub kind: InstallKind,
    /// The exe, the `.app` bundle or the AppImage file.
    pub target: PathBuf,
    /// The new copy, staged next to the target.
    pub staged: PathBuf,
    /// Where the running copy is moved during the swap.
    pub aside: PathBuf,
}

/// Unpacks and relaunches for one OS. `stage` runs on a worker thread.
pub trait Updater: Send + Sync {
    /// Unpacks the verified release file `download` into `plan.staged`
    /// (replacing whatever is there). `scratch` is an empty folder the call
    /// may use.
    fn stage(&self, plan: &InstallPlan, download: &Path, scratch: &Path) -> Result<(), String>;
    /// Starts the swapped-in copy at `plan.target` with `args`. Does not wait.
    fn relaunch(&self, plan: &InstallPlan, args: &[String]) -> Result<(), String>;
}

/// Directories a package manager owns (Linux).
const PACKAGE_ROOTS: [&str; 6] = ["/usr/", "/opt/", "/nix/", "/snap/", "/app/", "/gnu/"];

/// The kind of install and the path a self-update replaces. `os` is
/// `std::env::consts::OS`, `exe` the running executable, `appimage` the
/// `APPIMAGE` variable.
pub fn detect(os: &str, exe: &Path, appimage: Option<&OsStr>) -> (InstallKind, PathBuf) {
    match os {
        "windows" => (InstallKind::WindowsExe, exe.to_path_buf()),
        "macos" => match exe.ancestors().nth(3) {
            Some(app)
                if app.extension() == Some(OsStr::new("app"))
                    && exe.parent().and_then(Path::file_name) == Some(OsStr::new("MacOS")) =>
            {
                (InstallKind::MacApp, app.to_path_buf())
            }
            _ => (InstallKind::Unsupported, exe.to_path_buf()),
        },
        "linux" => match appimage.filter(|a| !a.is_empty()) {
            Some(a) => (InstallKind::AppImage, PathBuf::from(a)),
            None if PACKAGE_ROOTS.iter().any(|r| exe.starts_with(r)) => (InstallKind::LinuxPackage, exe.to_path_buf()),
            None => (InstallKind::LinuxBinary, exe.to_path_buf()),
        },
        _ => (InstallKind::Unsupported, exe.to_path_buf()),
    }
}

/// `path` with `suffix` appended to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// The swap plan for `(kind, target)` from [`detect`], or why there is none.
/// `writable` says whether a folder accepts new files ([`dir_writable`]).
pub fn plan(kind: InstallKind, target: &Path, writable: fn(&Path) -> bool) -> Result<InstallPlan, Blocked> {
    match kind {
        InstallKind::LinuxPackage => return Err(Blocked::Package),
        InstallKind::Unsupported => return Err(Blocked::Unsupported),
        _ => {}
    }
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).ok_or(Blocked::Unsupported)?;
    if !writable(dir) {
        return Err(Blocked::ReadOnly);
    }
    Ok(InstallPlan {
        kind,
        target: target.to_path_buf(),
        staged: with_suffix(target, ".new"),
        aside: with_suffix(target, ".old"),
    })
}

/// True when a file can be created (and removed) in `dir`.
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".frostpane-write-test-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Removes a file or a folder tree; absent is fine.
pub fn remove_any(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// Puts the staged copy in place, keeping the running one at `aside`. On
/// failure everything is as before.
pub fn swap(plan: &InstallPlan) -> io::Result<()> {
    if std::fs::symlink_metadata(&plan.staged).is_err() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "nothing staged"));
    }
    remove_any(&plan.aside)?;
    std::fs::rename(&plan.target, &plan.aside)?;
    if let Err(e) = std::fs::rename(&plan.staged, &plan.target) {
        let _ = std::fs::rename(&plan.aside, &plan.target);
        return Err(e);
    }
    Ok(())
}

/// Undoes [`swap`] (the new copy could not be started): the new copy goes
/// back to `staged`, the old one back to `target`.
pub fn unswap(plan: &InstallPlan) -> io::Result<()> {
    std::fs::rename(&plan.target, &plan.staged)?;
    std::fs::rename(&plan.aside, &plan.target)
}

/// Removes what an earlier update left next to the target (the previous
/// copy, an unused staged copy). Call once the single-instance lock is held,
/// so no old process still runs from `aside`.
pub fn clean(plan: &InstallPlan) {
    for p in [&plan.aside, &plan.staged] {
        let _ = remove_any(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detect_kinds() {
        let exe = Path::new("C:/Tools/frostpane.exe");
        assert_eq!(detect("windows", exe, None), (InstallKind::WindowsExe, exe.into()));
        let mac = Path::new("/Applications/Frostpane.app/Contents/MacOS/frostpane");
        assert_eq!(detect("macos", mac, None), (InstallKind::MacApp, "/Applications/Frostpane.app".into()));
        // A bare binary or a lookalike path is no bundle.
        let dev = Path::new("/Users/x/frostpane/target/release/frostpane");
        assert_eq!(detect("macos", dev, None).0, InstallKind::Unsupported);
        let odd = Path::new("/x/Frostpane.app/Contents/Other/frostpane");
        assert_eq!(detect("macos", odd, None).0, InstallKind::Unsupported);
        assert_eq!(detect("macos", Path::new("frostpane"), None).0, InstallKind::Unsupported);
        let bin = Path::new("/home/u/apps/frostpane/frostpane");
        let img = OsStr::new("/home/u/Frostpane.AppImage");
        assert_eq!(detect("linux", bin, Some(img)), (InstallKind::AppImage, img.into()));
        assert_eq!(detect("linux", bin, Some(OsStr::new(""))), (InstallKind::LinuxBinary, bin.into()));
        assert_eq!(detect("linux", bin, None).0, InstallKind::LinuxBinary);
        assert_eq!(detect("linux", Path::new("/usr/bin/frostpane"), None).0, InstallKind::LinuxPackage);
        assert_eq!(detect("linux", Path::new("/opt/frostpane/frostpane"), None).0, InstallKind::LinuxPackage);
        assert_eq!(detect("freebsd", bin, None).0, InstallKind::Unsupported);
    }

    #[test]
    fn plans() {
        let yes = |_: &Path| true;
        let p = plan(InstallKind::WindowsExe, Path::new("C:/T/frostpane.exe"), yes).unwrap();
        assert_eq!(p.staged, Path::new("C:/T/frostpane.exe.new"));
        assert_eq!(p.aside, Path::new("C:/T/frostpane.exe.old"));
        let p = plan(InstallKind::MacApp, Path::new("/Applications/Frostpane.app"), yes).unwrap();
        assert_eq!(p.aside, Path::new("/Applications/Frostpane.app.old"));
        assert_eq!(p.kind, InstallKind::MacApp);
        assert_eq!(plan(InstallKind::LinuxPackage, Path::new("/usr/bin/x"), yes), Err(Blocked::Package));
        assert_eq!(plan(InstallKind::Unsupported, Path::new("/x/y"), yes), Err(Blocked::Unsupported));
        assert_eq!(plan(InstallKind::AppImage, Path::new("x"), yes), Err(Blocked::Unsupported));
        let no = |_: &Path| false;
        assert_eq!(plan(InstallKind::LinuxBinary, Path::new("/srv/x"), no), Err(Blocked::ReadOnly));
    }

    #[test]
    fn writable_probe() {
        let d = tmp("probe");
        assert!(dir_writable(&d));
        assert!(!dir_writable(&d.join("missing")));
        // The probe leaves nothing behind.
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn swap_and_clean_files() {
        let d = tmp("swap");
        let p = plan(InstallKind::WindowsExe, &d.join("frostpane.exe"), dir_writable).unwrap();
        std::fs::write(&p.target, b"old").unwrap();
        // Nothing staged: refused, nothing moved.
        assert_eq!(swap(&p).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read(&p.target).unwrap(), b"old");
        std::fs::write(&p.staged, b"new").unwrap();
        // A stale aside copy from an earlier update is replaced.
        std::fs::write(&p.aside, b"older").unwrap();
        swap(&p).unwrap();
        assert_eq!(std::fs::read(&p.target).unwrap(), b"new");
        assert_eq!(std::fs::read(&p.aside).unwrap(), b"old");
        assert!(!p.staged.exists());
        unswap(&p).unwrap();
        assert_eq!(std::fs::read(&p.target).unwrap(), b"old");
        assert_eq!(std::fs::read(&p.staged).unwrap(), b"new");
        swap(&p).unwrap();
        clean(&p);
        assert!(!p.aside.exists() && !p.staged.exists());
        assert_eq!(std::fs::read(&p.target).unwrap(), b"new");
        // Nothing to undo.
        assert!(unswap(&p).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn swap_bundles_and_failures() {
        let d = tmp("bundle");
        let p = plan(InstallKind::MacApp, &d.join("Frostpane.app"), dir_writable).unwrap();
        std::fs::create_dir_all(p.target.join("Contents")).unwrap();
        std::fs::write(p.target.join("Contents/v"), b"1").unwrap();
        std::fs::create_dir_all(p.staged.join("Contents")).unwrap();
        std::fs::write(p.staged.join("Contents/v"), b"2").unwrap();
        std::fs::create_dir_all(p.aside.join("stale")).unwrap();
        swap(&p).unwrap();
        assert_eq!(std::fs::read(p.target.join("Contents/v")).unwrap(), b"2");
        assert_eq!(std::fs::read(p.aside.join("Contents/v")).unwrap(), b"1");
        clean(&p);
        assert!(!p.aside.exists());
        // Target missing: the first rename fails and nothing else moves.
        let q = plan(InstallKind::LinuxBinary, &d.join("gone"), dir_writable).unwrap();
        std::fs::write(&q.staged, b"n").unwrap();
        assert!(swap(&q).is_err());
        assert!(q.staged.exists());
        // An aside path that cannot even be looked at.
        let bad = InstallPlan { aside: d.join("a\0b"), ..q.clone() };
        assert!(remove_any(&bad.aside).is_err());
        assert!(swap(&bad).is_err());
        // The second rename fails (the staged copy moved away with the
        // target): the target is put back.
        let boxed = d.join("box");
        std::fs::create_dir_all(&boxed).unwrap();
        let r = InstallPlan {
            kind: InstallKind::LinuxBinary,
            target: boxed.clone(),
            staged: boxed.join("new"),
            aside: d.join("box.old"),
        };
        std::fs::write(&r.staged, b"n").unwrap();
        assert!(swap(&r).is_err());
        assert_eq!(std::fs::read(&r.staged).unwrap(), b"n");
        assert!(!r.aside.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
