//! Single instance (core-app 2.2): an exclusive `flock` on
//! `<data>/instance.lock` decides who is first; the first instance listens on
//! a Unix socket and a second launch writes `open` to it unless quiet.

use crate::logic;
use fp_platform::{Instance, InstanceGuard};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

fn socket() -> PathBuf {
    let tmp = std::env::var_os("TMPDIR").map(PathBuf::from);
    // SAFETY: getuid has no preconditions.
    logic::socket_path(tmp.as_deref(), unsafe { libc::getuid() })
}

/// The first instance: holds the lock file and the listening socket.
struct First {
    _lock: Option<File>,
    listener: Mutex<Option<UnixListener>>,
    path: PathBuf,
}

impl InstanceGuard for First {
    fn listen(&self, on_open: Box<dyn Fn() + Send>) {
        let Some(l) = self.listener.lock().ok().and_then(|mut g| g.take()) else { return };
        let spawned = std::thread::Builder::new().name("frostpane second launch".into()).spawn(move || {
            for conn in l.incoming() {
                let Ok(mut c) = conn else { continue };
                let _ = c.set_read_timeout(Some(Duration::from_secs(1)));
                let mut buf = [0u8; 64];
                if c.read(&mut buf).is_ok_and(|n| logic::is_open(&buf[..n])) {
                    on_open();
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("single instance: listener thread: {e}");
        }
    }
}

impl Drop for First {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `Err(())` when another process holds the lock. A lock that cannot be made
/// at all (unwritable data folder) counts as ours, so the app still starts.
fn try_lock(dir: &Path) -> Result<Option<File>, ()> {
    let open = std::fs::create_dir_all(dir)
        .and_then(|_| File::options().create(true).truncate(false).write(true).open(dir.join("instance.lock")));
    let f = match open {
        Ok(f) => f,
        Err(e) => {
            log::warn!("single instance: lock file: {e}");
            return Ok(None);
        }
    };
    // SAFETY: valid descriptor owned by `f`.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if r == 0 {
        return Ok(Some(f));
    }
    let held = std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK);
    if held { Err(()) } else { Ok(None) }
}

pub fn acquire(data_dir: &Path, quiet: bool) -> Instance {
    acquire_at(data_dir, socket(), quiet)
}

fn acquire_at(data_dir: &Path, path: PathBuf, quiet: bool) -> Instance {
    let Ok(lock) = try_lock(data_dir) else {
        if !quiet && let Ok(mut s) = UnixStream::connect(&path) {
            let _ = s.write_all(logic::OPEN_LINE);
        }
        return Instance::Second;
    };
    // We hold the lock, so a socket file left behind is stale.
    if lock.is_some() {
        let _ = std::fs::remove_file(&path);
    }
    let listener =
        UnixListener::bind(&path).inspect_err(|e| log::warn!("single instance: socket {}: {e}", path.display())).ok();
    Instance::First(Box::new(First { _lock: lock, listener: Mutex::new(listener), path }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_launch_wakes_the_first() {
        let dir = std::env::temp_dir().join(format!("fp-mac-instance-{}", std::process::id()));
        let sock = dir.join("t.sock");
        let first = acquire_at(&dir, sock.clone(), false).first().expect("first");
        let (tx, rx) = std::sync::mpsc::channel();
        first.listen(Box::new(move || {
            let _ = tx.send(());
        }));
        // flock is per open file, so a second open in this process conflicts too.
        assert!(acquire_at(&dir, sock.clone(), true).first().is_none());
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        assert!(acquire_at(&dir, sock.clone(), false).first().is_none());
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
        drop(first);
        assert!(!sock.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
