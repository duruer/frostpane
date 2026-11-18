//! The in-app updater: download, verify, stage, swap, relaunch.
//!
//! One job at a time runs on its own thread: fetch the release's
//! `SHA256SUMS.txt` and its minisign signature, check the signature against
//! the embedded release key, stream the file for this build into
//! `<data dir>/update/`, check its SHA-256 against the signed list, and let
//! the platform's [`Updater`] unpack it next to the installed copy. Nothing
//! outside the data folder changes before every check passed. The swap and
//! the relaunch run on the UI thread when the host says so
//! ([`SelfUpdate::install`]); a failed relaunch is rolled back.

use fp_platform::update::{self as plat, Blocked, InstallKind, InstallPlan, Updater};
use fp_sources::install::{self, Download, Package, Refused};
use fp_sources::update::Release;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// Command-line flag of a copy started by an update.
pub const UPDATED_FLAG: &str = "--updated";
/// Pref: the version an update just put in place (toast on its first start).
pub const KEY_UPDATED_TO: &str = "updatedTo";

/// Why an install stopped (the card shows one message per kind).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    Network,
    Verify,
    Install,
}

/// News from the job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Progress {
        done: u64,
        total: u64,
    },
    /// Verified and staged; [`SelfUpdate::install`] can run.
    Ready,
    Failed(Fail),
}

/// The release file kind for an install kind.
pub fn package(kind: InstallKind) -> Option<Package> {
    match kind {
        InstallKind::WindowsExe => Some(Package::WindowsZip),
        InstallKind::MacApp => Some(Package::MacZip),
        InstallKind::AppImage => Some(Package::AppImage),
        InstallKind::LinuxBinary => Some(Package::Tarball),
        InstallKind::LinuxPackage | InstallKind::Unsupported => None,
    }
}

/// What the job needs, shareable with its thread.
struct Job {
    dl: Arc<dyn Download>,
    os: Arc<dyn Updater>,
    plan: InstallPlan,
    release: Release,
    package: Package,
    target: (&'static str, &'static str),
    scratch: PathBuf,
    key: String,
}

/// Download, verify and stage. Sends progress; returns the outcome.
fn run(job: &Job, tx: &Sender<Msg>) -> Result<(), Fail> {
    let r = &job.release;
    let sums_asset = install::find(r, install::SUMS_NAME).ok_or(Fail::Verify)?;
    // Unsigned releases are never installed.
    let sig_asset = install::find(r, install::SIG_NAME).ok_or(Fail::Verify)?;
    let asset = install::pick(r, job.target.0, job.target.1, job.package).ok_or(Fail::Install)?;
    let sums = job.dl.bytes(&sums_asset.url, install::MAX_SMALL_BYTES).map_err(|e| net(&e))?;
    let sig = job.dl.bytes(&sig_asset.url, install::MAX_SMALL_BYTES).map_err(|e| net(&e))?;
    let sig = String::from_utf8_lossy(&sig);
    // Check the list before the big download.
    install::verify_sums(&sums, &sig, &job.key).map_err(|e| refused(&e))?;
    let _ = plat::remove_any(&job.scratch);
    std::fs::create_dir_all(&job.scratch).map_err(|e| {
        log::warn!("update: {}: {e}", job.scratch.display());
        Fail::Install
    })?;
    // A fixed name: nothing from the release JSON ends up in a path.
    let file = job.scratch.join("release-file");
    let total = asset.size;
    let _ = tx.send(Msg::Progress { done: 0, total });
    let mut last = 0u64;
    let digest = job
        .dl
        .file(&asset.url, &file, install::asset_cap(asset), &mut |done| {
            // About every 1 % (and the end), not every 64 KiB.
            if done == total || done.saturating_sub(last) >= (total / 100).max(1 << 16) {
                last = done;
                let _ = tx.send(Msg::Progress { done, total });
            }
        })
        .map_err(|e| net(&e))?;
    install::verify_file(&sums, &sig, &job.key, &asset.name, &digest).map_err(|e| refused(&e))?;
    let unpack = job.scratch.join("unpack");
    std::fs::create_dir_all(&unpack).map_err(|_| Fail::Install)?;
    job.os.stage(&job.plan, &file, &unpack).map_err(|e| {
        log::warn!("update: staging failed: {e}");
        Fail::Install
    })?;
    let _ = plat::remove_any(&job.scratch);
    log::info!("update: {} verified and staged at {}", asset.name, job.plan.staged.display());
    Ok(())
}

fn net(e: &str) -> Fail {
    log::warn!("update: download failed: {e}");
    Fail::Network
}

fn refused(e: &Refused) -> Fail {
    log::warn!("update: refused: {e}");
    Fail::Verify
}

/// The updater as the UI thread sees it.
pub struct SelfUpdate {
    os: Option<Arc<dyn Updater>>,
    plan: Result<InstallPlan, Blocked>,
    dl: Arc<dyn Download>,
    scratch: PathBuf,
    key: String,
    target: (&'static str, &'static str),
    rx: Option<Receiver<Msg>>,
    /// Tag of the release being fetched or staged.
    tag: Option<String>,
    ready: bool,
    installed: bool,
}

impl SelfUpdate {
    /// `plan` from [`plan_here`]; `os` the platform's updater (none: this
    /// platform cannot update itself).
    pub fn new(
        os: Option<Arc<dyn Updater>>,
        plan: Result<InstallPlan, Blocked>,
        dl: Arc<dyn Download>,
        data_dir: &Path,
    ) -> Self {
        Self {
            os,
            plan,
            dl,
            scratch: data_dir.join("update"),
            key: install::RELEASE_KEY.to_string(),
            target: (std::env::consts::OS, std::env::consts::ARCH),
            rx: None,
            tag: None,
            ready: false,
            installed: false,
        }
    }

    /// Why this copy cannot update itself (`None`: it can).
    pub fn blocked(&self) -> Option<Blocked> {
        match (&self.os, &self.plan) {
            (_, Err(b)) => Some(*b),
            (None, Ok(_)) => Some(Blocked::Unsupported),
            (Some(_), Ok(p)) if package(p.kind).is_none() => Some(Blocked::Unsupported),
            _ => None,
        }
    }

    /// A release for this build exists in `r` (and this copy can update).
    pub fn can_install(&self, r: &Release) -> bool {
        self.blocked().is_none()
            && self
                .plan
                .as_ref()
                .ok()
                .and_then(|p| package(p.kind))
                .is_some_and(|pk| install::pick(r, self.target.0, self.target.1, pk).is_some())
    }

    /// Size of the file this build would download from `r` (0: none).
    pub fn size_for(&self, r: &Release) -> u64 {
        let pk = self.plan.as_ref().ok().and_then(|p| package(p.kind));
        pk.and_then(|pk| install::pick(r, self.target.0, self.target.1, pk)).map_or(0, |a| a.size)
    }

    /// A job runs.
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// `r` is verified and staged.
    pub fn ready_for(&self, r: &Release) -> bool {
        self.ready && self.tag.as_deref() == Some(r.tag.as_str())
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Start fetching `r`. False when nothing started (busy, already
    /// staged, or no file for this build).
    pub fn start(&mut self, r: &Release) -> bool {
        if self.busy() || self.ready_for(r) || !self.can_install(r) {
            return false;
        }
        let (Some(os), Ok(plan)) = (self.os.clone(), self.plan.clone()) else { return false };
        let Some(package) = package(plan.kind) else { return false };
        let job = Job {
            dl: self.dl.clone(),
            os,
            plan,
            release: r.clone(),
            package,
            target: self.target,
            scratch: self.scratch.clone(),
            key: self.key.clone(),
        };
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("fp-update".into()).spawn(move || {
            let msg = match run(&job, &tx) {
                Ok(()) => Msg::Ready,
                Err(f) => {
                    let _ = plat::remove_any(&job.scratch);
                    Msg::Failed(f)
                }
            };
            let _ = tx.send(msg);
        });
        if spawned.is_err() {
            return false;
        }
        self.rx = Some(rx);
        self.tag = Some(r.tag.clone());
        self.ready = false;
        true
    }

    /// News since the last call (non-blocking).
    pub fn poll(&mut self) -> Vec<Msg> {
        let mut out = Vec::new();
        let Some(rx) = &self.rx else { return out };
        let mut done = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Ready => {
                    self.ready = true;
                    done = true;
                }
                Msg::Failed(_) => done = true,
                Msg::Progress { .. } => {}
            }
            out.push(m);
        }
        if done {
            self.rx = None;
        }
        out
    }

    /// Swap the staged copy in and start it with `args`; the caller then
    /// quits. A failed start puts the old copy back.
    pub fn install(&mut self, args: &[String]) -> Result<(), Fail> {
        let (Some(os), Ok(plan)) = (&self.os, &self.plan) else { return Err(Fail::Install) };
        if !self.ready || self.installed {
            return Err(Fail::Install);
        }
        plat::swap(plan).map_err(|e| {
            log::warn!("update: swap failed: {e}");
            Fail::Install
        })?;
        if let Err(e) = os.relaunch(plan, args) {
            log::warn!("update: relaunch failed: {e}");
            if let Err(e) = plat::unswap(plan) {
                log::error!("update: rollback failed: {e}");
            }
            return Err(Fail::Install);
        }
        self.installed = true;
        Ok(())
    }

    /// At quit: put a staged copy in place without starting it (it runs at
    /// the next start). True when it did.
    pub fn apply_at_quit(&mut self) -> bool {
        let Ok(plan) = &self.plan else { return false };
        if !self.ready || self.installed {
            return false;
        }
        match plat::swap(plan) {
            Ok(()) => {
                self.installed = true;
                true
            }
            Err(e) => {
                log::warn!("update: swap at quit failed: {e}");
                false
            }
        }
    }

    /// The tag being fetched or staged.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }
}

/// The swap plan for this process.
pub fn plan_here() -> Result<InstallPlan, Blocked> {
    let exe = std::env::current_exe().map_err(|_| Blocked::Unsupported)?;
    let appimage = std::env::var_os("APPIMAGE");
    let (kind, target) = plat::detect(std::env::consts::OS, &exe, appimage.as_deref());
    plat::plan(kind, &target, plat::dir_writable)
}

/// Removes what a previous update left next to the installed copy. Call
/// with the single-instance lock held.
pub fn clean_leftovers() {
    if let Ok(p) = plan_here() {
        plat::clean(&p);
    }
}

/// The release version without the leading `v`.
pub fn version_of(tag: &str) -> &str {
    tag.trim_start_matches(['v', 'V'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use fp_sources::update::Asset;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const SUMS: &[u8] = include_bytes!("../../../crates/fp-sources/testdata/SHA256SUMS.txt");
    const SIG: &[u8] = include_bytes!("../../../crates/fp-sources/testdata/SHA256SUMS.txt.minisig");
    const TEST_KEY: &str = include_str!("../../../crates/fp-sources/testdata/test.pub");
    const ZIP: &str = "frostpane-v9.0.0-windows-x64.zip";

    /// In-memory release files.
    struct Files(Mutex<BTreeMap<String, Vec<u8>>>);

    impl Download for Files {
        fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
            let m = self.0.lock().unwrap();
            let b = m.get(url).ok_or("404")?;
            if b.len() as u64 > max { Err("too big".into()) } else { Ok(b.clone()) }
        }
        fn file(&self, url: &str, dest: &Path, max: u64, progress: &mut dyn FnMut(u64)) -> Result<[u8; 32], String> {
            let b = self.bytes(url, max)?;
            std::fs::write(dest, &b).map_err(|e| e.to_string())?;
            progress(b.len() as u64);
            // The fixture list vouches for b"payload" only.
            let sha = if b == b"payload" {
                install::parse_hex32("239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5").unwrap()
            } else {
                [0; 32]
            };
            Ok(sha)
        }
    }

    /// Copies the download to `staged`; the relaunch can be told to fail.
    struct FakeOs {
        launches: Mutex<Vec<Vec<String>>>,
        fail_launch: bool,
    }

    impl Updater for FakeOs {
        fn stage(&self, plan: &InstallPlan, download: &Path, _: &Path) -> Result<(), String> {
            std::fs::copy(download, &plan.staged).map(|_| ()).map_err(|e| e.to_string())
        }
        fn relaunch(&self, _: &InstallPlan, args: &[String]) -> Result<(), String> {
            if self.fail_launch {
                return Err("no".into());
            }
            self.launches.lock().unwrap().push(args.to_vec());
            Ok(())
        }
    }

    fn url(name: &str) -> String {
        format!("{}v9.0.0/{name}", install::REPO_DOWNLOADS)
    }

    fn release(with_sig: bool) -> Release {
        let mut names = vec![ZIP, install::SUMS_NAME];
        if with_sig {
            names.push(install::SIG_NAME);
        }
        Release {
            tag: "v9.0.0".into(),
            url: "https://github.com/duruer/frostpane/releases/tag/v9.0.0".into(),
            assets: names.iter().map(|n| Asset { name: n.to_string(), url: url(n), size: 7 }).collect(),
        }
    }

    struct Rig {
        dir: PathBuf,
        up: SelfUpdate,
        os: Arc<FakeOs>,
        files: Arc<Files>,
    }

    fn rig(name: &str, payload: &[u8], fail_launch: bool) -> Rig {
        let dir = std::env::temp_dir().join(format!("fp-app-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("app")).unwrap();
        let target = dir.join("app/frostpane.exe");
        std::fs::write(&target, b"old").unwrap();
        let plan = plat::plan(InstallKind::WindowsExe, &target, plat::dir_writable);
        let mut m = BTreeMap::new();
        m.insert(url(ZIP), payload.to_vec());
        m.insert(url(install::SUMS_NAME), SUMS.to_vec());
        m.insert(url(install::SIG_NAME), SIG.to_vec());
        let files = Arc::new(Files(Mutex::new(m)));
        let os = Arc::new(FakeOs { launches: Mutex::new(Vec::new()), fail_launch });
        let mut up = SelfUpdate::new(Some(os.clone()), plan, files.clone(), &dir.join("data"));
        up.key = TEST_KEY.to_string();
        up.target = ("windows", "x86_64");
        Rig { dir, up, os, files }
    }

    fn wait(up: &mut SelfUpdate) -> Vec<Msg> {
        let mut all = Vec::new();
        for _ in 0..400 {
            all.extend(up.poll());
            if !up.busy() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        all
    }

    #[test]
    fn verified_download_installs_and_relaunches() {
        let mut r = rig("ok", b"payload", false);
        let rel = release(true);
        assert!(r.up.blocked().is_none());
        assert!(r.up.can_install(&rel));
        assert_eq!(r.up.size_for(&rel), 7);
        assert!(r.up.start(&rel));
        assert!(!r.up.start(&rel), "one job at a time");
        let msgs = wait(&mut r.up);
        assert_eq!(msgs.first(), Some(&Msg::Progress { done: 0, total: 7 }));
        assert_eq!(msgs.last(), Some(&Msg::Ready));
        assert!(r.up.ready() && r.up.ready_for(&rel));
        assert_eq!(r.up.tag(), Some("v9.0.0"));
        assert!(!r.up.start(&rel), "already staged");
        // The download area is cleaned once staged.
        assert!(!r.dir.join("data/update").exists());
        r.up.install(&["--updated".into()]).unwrap();
        assert_eq!(std::fs::read(r.dir.join("app/frostpane.exe")).unwrap(), b"payload");
        assert_eq!(std::fs::read(r.dir.join("app/frostpane.exe.old")).unwrap(), b"old");
        assert_eq!(r.os.launches.lock().unwrap().as_slice(), &[vec!["--updated".to_string()]]);
        // Only once.
        assert_eq!(r.up.install(&[]), Err(Fail::Install));
        assert!(!r.up.apply_at_quit());
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn tampered_or_unsigned_releases_are_refused() {
        // Wrong bytes: the checksum does not match the signed list.
        let mut r = rig("tampered", b"evil!!!", false);
        assert!(r.up.start(&release(true)));
        assert_eq!(wait(&mut r.up).last(), Some(&Msg::Failed(Fail::Verify)));
        assert!(!r.up.ready());
        assert_eq!(std::fs::read(r.dir.join("app/frostpane.exe")).unwrap(), b"old");
        assert!(!r.dir.join("app/frostpane.exe.new").exists());
        // No signature on the release.
        assert!(r.up.start(&release(false)));
        assert_eq!(wait(&mut r.up).last(), Some(&Msg::Failed(Fail::Verify)));
        // A list signed by another key (the real release key here).
        r.up.key = install::RELEASE_KEY.to_string();
        r.files.0.lock().unwrap().insert(url(ZIP), b"payload".to_vec());
        assert!(r.up.start(&release(true)));
        assert_eq!(wait(&mut r.up).last(), Some(&Msg::Failed(Fail::Verify)));
        // Network trouble.
        r.files.0.lock().unwrap().remove(&url(install::SUMS_NAME));
        assert!(r.up.start(&release(true)));
        assert_eq!(wait(&mut r.up).last(), Some(&Msg::Failed(Fail::Network)));
        assert_eq!(r.up.install(&[]), Err(Fail::Install), "nothing staged");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn failed_relaunch_rolls_back_and_quit_applies() {
        let mut r = rig("rollback", b"payload", true);
        assert!(r.up.start(&release(true)));
        assert_eq!(wait(&mut r.up).last(), Some(&Msg::Ready));
        assert_eq!(r.up.install(&[]), Err(Fail::Install));
        assert_eq!(std::fs::read(r.dir.join("app/frostpane.exe")).unwrap(), b"old");
        assert_eq!(std::fs::read(r.dir.join("app/frostpane.exe.new")).unwrap(), b"payload");
        // At quit the staged copy goes in without a start.
        assert!(r.up.apply_at_quit());
        assert_eq!(std::fs::read(r.dir.join("app/frostpane.exe")).unwrap(), b"payload");
        assert!(!r.up.apply_at_quit());
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn blocked_copies_never_start() {
        let files: Arc<dyn Download> = Arc::new(Files(Mutex::new(BTreeMap::new())));
        let dir = std::env::temp_dir();
        let deb = SelfUpdate::new(None, Err(Blocked::Package), files.clone(), &dir);
        assert_eq!(deb.blocked(), Some(Blocked::Package));
        assert!(!deb.can_install(&release(true)));
        let plan = plat::plan(InstallKind::WindowsExe, &dir.join("x.exe"), |_| true);
        let mut no_os = SelfUpdate::new(None, plan.clone(), files.clone(), &dir);
        assert_eq!(no_os.blocked(), Some(Blocked::Unsupported));
        assert!(!no_os.start(&release(true)));
        assert!(!no_os.apply_at_quit());
        // No file for this build in the release.
        let os: Arc<dyn Updater> = Arc::new(FakeOs { launches: Mutex::new(Vec::new()), fail_launch: false });
        let mut other = SelfUpdate::new(Some(os), plan, files, &dir);
        other.target = ("windows", "riscv64");
        assert!(!other.can_install(&release(true)));
        assert_eq!(other.size_for(&release(true)), 0);
        assert!(!other.start(&release(true)));
        assert!(other.poll().is_empty());
    }

    #[test]
    fn kinds_and_versions() {
        assert_eq!(package(InstallKind::WindowsExe), Some(Package::WindowsZip));
        assert_eq!(package(InstallKind::MacApp), Some(Package::MacZip));
        assert_eq!(package(InstallKind::AppImage), Some(Package::AppImage));
        assert_eq!(package(InstallKind::LinuxBinary), Some(Package::Tarball));
        assert_eq!(package(InstallKind::LinuxPackage), None);
        assert_eq!(package(InstallKind::Unsupported), None);
        assert_eq!(version_of("v1.2.3"), "1.2.3");
        assert_eq!(version_of("1.2.3"), "1.2.3");
        // The test binary itself is a plain exe somewhere writable.
        let _ = plan_here();
    }
}
