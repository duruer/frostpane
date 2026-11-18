//! In-app updater, Windows part: take `frostpane.exe` out of the release zip
//! and start the swapped-in exe. The swap itself is `fp_platform::update`.

use fp_platform::update::{InstallPlan, Updater};
use std::io::Read;
// Listed only to pick flate2's pure-Rust backend for zip's deflate.
use flate2 as _;
use std::path::Path;

/// The exe inside the release zip.
const EXE: &str = "frostpane.exe";
/// Largest exe accepted from the zip.
const MAX_EXE_BYTES: u64 = 128 << 20;

/// [`Updater`] for the portable Windows zip.
pub struct WinUpdater;

/// Copies `frostpane.exe` (at the root of the zip or one folder down) to
/// `dest`.
pub fn extract_exe(zip: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let index = (0..archive.len())
        .find(|&i| {
            archive.by_index(i).ok().and_then(|f| f.enclosed_name()).is_some_and(|p| {
                p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(EXE)) && p.components().count() <= 2
            })
        })
        .ok_or("frostpane.exe is not in the zip")?;
    let entry = archive.by_index(index).map_err(|e| e.to_string())?;
    if entry.size() > MAX_EXE_BYTES {
        return Err("frostpane.exe in the zip is too large".into());
    }
    let mut out = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let copied = std::io::copy(&mut entry.take(MAX_EXE_BYTES + 1), &mut out).map_err(|e| e.to_string())?;
    if copied > MAX_EXE_BYTES {
        drop(out);
        let _ = std::fs::remove_file(dest);
        return Err("frostpane.exe in the zip is too large".into());
    }
    out.sync_all().map_err(|e| e.to_string())
}

impl Updater for WinUpdater {
    fn stage(&self, plan: &InstallPlan, download: &Path, _scratch: &Path) -> Result<(), String> {
        fp_platform::update::remove_any(&plan.staged).map_err(|e| e.to_string())?;
        extract_exe(download, &plan.staged).inspect_err(|_| {
            let _ = std::fs::remove_file(&plan.staged);
        })
    }

    fn relaunch(&self, plan: &InstallPlan, args: &[String]) -> Result<(), String> {
        let dir = plan.target.parent().unwrap_or(Path::new("."));
        std::process::Command::new(&plan.target)
            .args(args)
            .current_dir(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", plan.target.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_platform::update::{InstallKind, dir_writable, plan, swap};
    use std::io::Write;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fp-win-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn extracts_the_exe_only() {
        let d = tmp("extract");
        let zip = d.join("a.zip");
        make_zip(&zip, &[("LICENSE", b"gpl"), ("frostpane.exe", b"MZ new"), ("README.md", b"r")]);
        extract_exe(&zip, &d.join("out.exe")).unwrap();
        assert_eq!(std::fs::read(d.join("out.exe")).unwrap(), b"MZ new");
        // One folder down is fine, deeper or missing is not.
        make_zip(&zip, &[("frostpane-v9/frostpane.exe", b"MZ 2")]);
        extract_exe(&zip, &d.join("out.exe")).unwrap();
        assert_eq!(std::fs::read(d.join("out.exe")).unwrap(), b"MZ 2");
        make_zip(&zip, &[("a/b/frostpane.exe", b"MZ 3"), ("../frostpane.exe", b"evil")]);
        assert!(extract_exe(&zip, &d.join("out.exe")).unwrap_err().contains("not in the zip"));
        std::fs::write(&zip, b"not a zip").unwrap();
        assert!(extract_exe(&zip, &d.join("out.exe")).is_err());
        assert!(extract_exe(&d.join("missing.zip"), &d.join("out.exe")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The whole Windows flow on dummy files: stage from a zip, swap while
    /// the "running" exe is held open, relaunch a real (tiny) process.
    #[test]
    fn stage_swap_relaunch_in_a_temp_dir() {
        let d = tmp("flow");
        let target = d.join("frostpane.exe");
        // Any real exe works as the new copy; it is started with a harmless
        // argument and exits at once.
        let cmd = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32\\cmd.exe");
        let new_exe = std::fs::read(&cmd).unwrap();
        std::fs::write(&target, b"old exe").unwrap();
        let zip = d.join("release.zip");
        make_zip(&zip, &[("frostpane.exe", &new_exe), ("LICENSE", b"gpl")]);
        let p = plan(InstallKind::WindowsExe, &target, dir_writable).unwrap();
        let up = WinUpdater;
        up.stage(&p, &zip, &d).unwrap();
        assert_eq!(std::fs::read(&p.staged).unwrap().len(), new_exe.len());
        // A handle on the old exe, like the running process has.
        let held = std::fs::File::open(&target).unwrap();
        swap(&p).unwrap();
        assert_eq!(std::fs::read(&p.aside).unwrap(), b"old exe");
        drop(held);
        up.relaunch(&p, &["/c".into(), "exit".into(), "0".into()]).unwrap();
        // Nothing to start any more.
        let gone = InstallPlan { target: d.join("missing.exe"), ..p.clone() };
        assert!(up.relaunch(&gone, &[]).is_err());
        // A broken download leaves nothing staged.
        std::fs::write(&zip, b"junk").unwrap();
        assert!(up.stage(&p, &zip, &d).is_err());
        assert!(!p.staged.exists());
        fp_platform::update::clean(&p);
        // The spawned cmd may still hold the exe for a moment.
        for _ in 0..50 {
            if std::fs::remove_dir_all(&d).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
