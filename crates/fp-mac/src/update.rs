//! In-app updater, macOS part: unpack `Frostpane.app` from the release zip
//! with `ditto` (keeps the bundle's modes, symlinks and signature) and
//! start the swapped-in bundle with `open -n`. The swap itself is
//! `fp_platform::update`.

use fp_platform::update::{InstallPlan, Updater, remove_any};
use std::path::Path;
use std::process::Command;

/// The bundle inside the release zip.
const APP: &str = "Frostpane.app";

/// [`Updater`] for the macOS universal zip.
pub struct MacUpdater;

fn run(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(format!("{cmd:?}: {}", String::from_utf8_lossy(&out.stderr).trim())) }
}

impl Updater for MacUpdater {
    /// Unpacks next to the installed bundle (same volume, so the swap is a
    /// rename), then moves the bundle to `plan.staged`.
    fn stage(&self, plan: &InstallPlan, download: &Path, _scratch: &Path) -> Result<(), String> {
        let parent = plan.target.parent().ok_or("the app has no parent folder")?;
        let unpack = parent.join(".frostpane-update");
        remove_any(&unpack).map_err(|e| e.to_string())?;
        remove_any(&plan.staged).map_err(|e| e.to_string())?;
        let result = (|| {
            run(Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(download).arg(&unpack))?;
            let app = unpack.join(APP);
            if !app.join("Contents/MacOS/frostpane").is_file() {
                return Err("Frostpane.app is not in the zip".to_string());
            }
            std::fs::rename(&app, &plan.staged).map_err(|e| e.to_string())
        })();
        let _ = remove_any(&unpack);
        result
    }

    fn relaunch(&self, plan: &InstallPlan, args: &[String]) -> Result<(), String> {
        Command::new("/usr/bin/open")
            .arg("-n")
            .arg(&plan.target)
            .arg("--args")
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_platform::update::{InstallKind, dir_writable, plan, swap};

    #[test]
    fn stage_and_swap_a_bundle() {
        let d = std::env::temp_dir().join(format!("fp-mac-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let fake = |root: &Path, body: &[u8]| {
            let bin = root.join("Contents/MacOS");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("frostpane"), body).unwrap();
        };
        fake(&d.join("src/Frostpane.app"), b"new");
        let zip = d.join("r.zip");
        run(Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(d.join("src/Frostpane.app"))
            .arg(&zip))
        .unwrap();
        let target = d.join("Applications/Frostpane.app");
        fake(&target, b"old");
        let p = plan(InstallKind::MacApp, &target, dir_writable).unwrap();
        MacUpdater.stage(&p, &zip, &d).unwrap();
        swap(&p).unwrap();
        assert_eq!(std::fs::read(target.join("Contents/MacOS/frostpane")).unwrap(), b"new");
        assert_eq!(std::fs::read(p.aside.join("Contents/MacOS/frostpane")).unwrap(), b"old");
        assert!(!d.join("Applications/.frostpane-update").exists());
        // A zip without the bundle, and a broken zip.
        std::fs::write(d.join("src/other"), b"x").unwrap();
        run(Command::new("/usr/bin/ditto").args(["-c", "-k"]).arg(d.join("src/other")).arg(&zip)).unwrap();
        assert!(MacUpdater.stage(&p, &zip, &d).unwrap_err().contains("not in the zip"));
        std::fs::write(&zip, b"junk").unwrap();
        assert!(MacUpdater.stage(&p, &zip, &d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
