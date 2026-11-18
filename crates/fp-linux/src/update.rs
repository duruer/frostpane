//! In-app updater, Linux part: the AppImage is the release file itself;
//! the tar.gz holds the `frostpane` binary one folder down. Both are staged
//! next to the target with mode 0755 and started directly. Package-manager
//! installs never get here (`fp_platform::update::Blocked::Package`).

use fp_platform::update::{InstallKind, InstallPlan, Updater, remove_any};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// [`Updater`] for the AppImage and the tar.gz binary.
pub struct LinuxUpdater;

/// `<scratch>/<one folder>/frostpane` after unpacking a release tarball.
fn tar_binary(download: &Path, scratch: &Path) -> Result<PathBuf, String> {
    let out = Command::new("tar")
        .arg("-xzf")
        .arg(download)
        .arg("-C")
        .arg(scratch)
        .arg("--no-same-owner")
        .output()
        .map_err(|e| format!("tar: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    std::fs::read_dir(scratch)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path().join("frostpane"))
        .find(|p| p.is_file())
        .ok_or_else(|| "frostpane is not in the archive".to_string())
}

impl Updater for LinuxUpdater {
    fn stage(&self, plan: &InstallPlan, download: &Path, scratch: &Path) -> Result<(), String> {
        remove_any(&plan.staged).map_err(|e| e.to_string())?;
        let source = match plan.kind {
            InstallKind::AppImage => download.to_path_buf(),
            _ => tar_binary(download, scratch)?,
        };
        let result = std::fs::copy(&source, &plan.staged)
            .and_then(|_| std::fs::set_permissions(&plan.staged, std::fs::Permissions::from_mode(0o755)))
            .map_err(|e| format!("{}: {e}", plan.staged.display()));
        if result.is_err() {
            let _ = remove_any(&plan.staged);
        }
        result
    }

    fn relaunch(&self, plan: &InstallPlan, args: &[String]) -> Result<(), String> {
        let dir = plan.target.parent().unwrap_or(Path::new("/"));
        // The old AppImage's mount goes away with this process; its runtime
        // variables must not leak into the new one.
        Command::new(&plan.target)
            .args(args)
            .current_dir(dir)
            .env_remove("APPDIR")
            .env_remove("APPIMAGE")
            .env_remove("ARGV0")
            .env_remove("OWD")
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", plan.target.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_platform::update::{dir_writable, plan, swap};

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp-linux-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tarball_and_appimage_flow() {
        let d = tmp("flow");
        // A release-shaped tarball whose "binary" is a shell script.
        let src = d.join("src/frostpane-v9.0.0-linux-x64");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("frostpane"), b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::write(src.join("LICENSE"), b"gpl").unwrap();
        let tgz = d.join("r.tar.gz");
        let ok = Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(d.join("src"))
            .arg("frostpane-v9.0.0-linux-x64")
            .status()
            .unwrap();
        assert!(ok.success());
        let target = d.join("app/frostpane");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"old").unwrap();
        let p = plan(InstallKind::LinuxBinary, &target, dir_writable).unwrap();
        let scratch = d.join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();
        LinuxUpdater.stage(&p, &tgz, &scratch).unwrap();
        let mode = std::fs::metadata(&p.staged).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        swap(&p).unwrap();
        LinuxUpdater.relaunch(&p, &["--updated".into(), "1.0.0".into()]).unwrap();
        // AppImage: the download is the file.
        let img = d.join("Frostpane.AppImage");
        std::fs::write(&img, b"old image").unwrap();
        let q = plan(InstallKind::AppImage, &img, dir_writable).unwrap();
        let dl = d.join("dl.AppImage");
        std::fs::write(&dl, b"new image").unwrap();
        LinuxUpdater.stage(&q, &dl, &scratch).unwrap();
        assert_eq!(std::fs::read(&q.staged).unwrap(), b"new image");
        // Broken tarball, empty tarball.
        std::fs::write(&tgz, b"junk").unwrap();
        assert!(LinuxUpdater.stage(&p, &tgz, &d.join("scratch2")).is_err());
        let empty = tmp("empty");
        assert!(tar_binary(&dl, &empty).is_err());
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
