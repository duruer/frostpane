//! Single instance: an exclusive `flock` on a lock file decides who is first;
//! the first instance listens on a Unix socket, a second launch connects,
//! sends `open` and exits. A stale socket file of a dead instance is
//! replaced (the lock dies with the process).

use crate::logic;
use fp_platform::InstanceGuard;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

/// Outcome of [`SingleInstance::acquire_at`].
pub enum Acquired {
    First(SingleInstance),
    Second,
}

/// The first instance's lock and listening socket.
pub struct SingleInstance {
    _lock: File,
    listener: UnixListener,
}

impl SingleInstance {
    /// Claims `lock`; the first instance binds `sock`. A second launch
    /// connects to `sock` and sends `open` unless `quiet`.
    pub fn acquire_at(lock: &Path, sock: &Path, quiet: bool) -> std::io::Result<Acquired> {
        let file = OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(lock)?;
        // SAFETY: `flock` on a descriptor we own; no memory is touched.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() != std::io::ErrorKind::WouldBlock {
                return Err(err);
            }
            if !quiet {
                notify_first(sock);
            }
            return Ok(Acquired::Second);
        }
        let _ = std::fs::remove_file(sock);
        let listener = UnixListener::bind(sock)?;
        let _ = std::fs::set_permissions(sock, std::fs::Permissions::from_mode(0o600));
        Ok(Acquired::First(SingleInstance { _lock: file, listener }))
    }
}

/// The first instance may still be between lock and bind: retry for ~1 s.
fn notify_first(sock: &Path) {
    for _ in 0..20 {
        if let Ok(mut s) = UnixStream::connect(sock) {
            let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
            let _ = s.write_all(logic::OPEN_MESSAGE);
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl InstanceGuard for SingleInstance {
    fn listen(&self, on_open: Box<dyn Fn() + Send>) {
        let Ok(listener) = self.listener.try_clone() else { return };
        let spawned = std::thread::Builder::new().name("fp-instance".into()).spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut line = String::new();
                if BufReader::new(stream.take(64)).read_line(&mut line).is_ok() && logic::is_open_message(&line) {
                    on_open();
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("single-instance listener not started: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::channel;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fp-linux-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn second_launch_opens_first() {
        let d = temp("inst");
        let (lock, sock) = (d.join("a.lock"), d.join("a.sock"));
        let Acquired::First(first) = SingleInstance::acquire_at(&lock, &sock, false).unwrap() else { panic!("first") };
        let (tx, rx) = channel();
        first.listen(Box::new(move || tx.send(()).unwrap()));
        assert!(matches!(SingleInstance::acquire_at(&lock, &sock, false).unwrap(), Acquired::Second));
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        // A quiet second launch sends nothing.
        assert!(matches!(SingleInstance::acquire_at(&lock, &sock, true).unwrap(), Acquired::Second));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        // Junk on the socket is ignored.
        UnixStream::connect(&sock).unwrap().write_all(b"hello\n").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
        drop(first);
        // The lock is free again after the first instance ends, stale socket and all.
        assert!(matches!(SingleInstance::acquire_at(&lock, &sock, false).unwrap(), Acquired::First(_)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn bad_paths_are_errors() {
        assert!(SingleInstance::acquire_at(Path::new("/nonexistent-dir/x.lock"), Path::new("/x"), true).is_err());
        let d = temp("long");
        let long = d.join("s".repeat(200));
        assert!(SingleInstance::acquire_at(&d.join("l.lock"), &long, true).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Child half of `second_process_opens_first`: does something only when
    /// the parent set the directory variable.
    #[test]
    fn child_second_launch() {
        let Ok(d) = std::env::var("FP_LINUX_CHILD_DIR") else { return };
        let d = Path::new(&d);
        let r = SingleInstance::acquire_at(&d.join("p.lock"), &d.join("p.sock"), false).unwrap();
        assert!(matches!(r, Acquired::Second));
    }

    #[test]
    fn second_process_opens_first() {
        let d = temp("proc");
        let Acquired::First(first) = SingleInstance::acquire_at(&d.join("p.lock"), &d.join("p.sock"), false).unwrap()
        else {
            panic!("first")
        };
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        first.listen(Box::new(move || {
            h.fetch_add(1, Ordering::SeqCst);
        }));
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "instance::tests::child_second_launch", "--nocapture"])
            .env("FP_LINUX_CHILD_DIR", &d)
            .status()
            .unwrap();
        assert!(status.success());
        for _ in 0..60 {
            if hits.load(Ordering::SeqCst) > 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
