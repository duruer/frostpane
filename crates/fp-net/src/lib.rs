//! HTTPS client for Open-Meteo (forecast + geocoding) and the GitHub
//! "latest release" notice. Not part of the coverage gate.
//!
//! Spec: weather-sources 2.3 (fetch, 20 s timeout, failures change nothing),
//! core-app 2.11 (update check: 20 s after start, then every 4 h, notice
//! only, never downloads or installs).
//!
//! All calls block, so the app runs them on one worker thread ([`Worker`])
//! and posts the results back to the UI thread.

use fp_sources::Fetch;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::Duration;

/// Request timeout (weather-sources 2.3, core-app 2.11: 20 s).
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// ureq + Windows TLS (schannel through native-tls).
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        let tls = native_tls::TlsConnector::new().map_err(|e| log::warn!("TLS init failed: {e}")).ok();
        let mut b =
            ureq::AgentBuilder::new().timeout(TIMEOUT).user_agent(concat!("Frostpane/", env!("CARGO_PKG_VERSION")));
        if let Some(t) = tls {
            b = b.tls_connector(Arc::new(t));
        }
        Self { agent: b.build() }
    }
}

/// Transport rule (core-app 2.11 "URL rule"): https only, plain http only
/// when the host is loopback (local tests, a local mirror).
pub fn url_allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    let Some(rest) = lower.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    match host {
        "localhost" => true,
        h => h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()),
    }
}

impl Fetch for Http {
    fn get(&self, url: &str) -> Result<String, String> {
        if !url_allowed(url) {
            return Err(format!("refused non-https URL: {url}"));
        }
        self.agent.get(url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())
    }
}

/// A queued GET; `id` is echoed back so the caller can match the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub url: String,
}

/// Reply to a [`Request`]. Failures carry the message; callers change
/// nothing on `Err` and do not retry before the next tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub id: u64,
    pub body: Result<String, String>,
}

/// One worker thread running requests in order. Dropping it stops the
/// thread after the request in flight (at most [`TIMEOUT`]); requests still
/// queued are skipped.
pub struct Worker {
    tx: Option<Sender<Request>>,
    stop: Arc<AtomicBool>,
    rx: Receiver<Reply>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Spawn with the real [`Http`] client; `wake` runs after each reply
    /// (the app passes `request_repaint`).
    pub fn spawn(wake: impl Fn() + Send + 'static) -> Self {
        Self::with_fetch(Http::default(), wake)
    }

    /// Spawn with any [`Fetch`] (tests).
    pub fn with_fetch(fetch: impl Fetch + 'static, wake: impl Fn() + Send + 'static) -> Self {
        let (tx, jobs) = channel::<Request>();
        let (out, rx) = channel::<Reply>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("fp-net".into())
            .spawn(move || {
                for job in jobs {
                    if stop2.load(Ordering::SeqCst) {
                        break;
                    }
                    let body = fetch.get(&job.url);
                    if let Err(e) = &body {
                        log::warn!("GET {} failed: {e}", job.url);
                    }
                    if out.send(Reply { id: job.id, body }).is_err() {
                        break;
                    }
                    wake();
                }
            })
            .map_err(|e| log::warn!("fp-net worker spawn failed: {e}"))
            .ok();
        Self { tx: Some(tx), stop, rx, handle }
    }

    /// Queue a GET. False if the worker is gone.
    pub fn request(&self, id: u64, url: impl Into<String>) -> bool {
        match &self.tx {
            Some(tx) => tx.send(Request { id, url: url.into() }).is_ok(),
            None => false,
        }
    }

    /// Next finished reply, if any (non-blocking).
    pub fn poll(&self) -> Option<Reply> {
        self.rx.try_recv().ok()
    }

    /// Wait up to `d` for a reply.
    pub fn wait(&self, d: Duration) -> Option<Reply> {
        self.rx.recv_timeout(d).ok()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Echo;
    impl Fetch for Echo {
        fn get(&self, url: &str) -> Result<String, String> {
            if url.contains("bad") { Err("nope".into()) } else { Ok(url.to_uppercase()) }
        }
    }

    #[test]
    fn worker_orders_and_wakes() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let w = Worker::with_fetch(Echo, move || {
            n2.fetch_add(1, Ordering::SeqCst);
        });
        assert!(w.request(1, "a"));
        assert!(w.request(2, "bad"));
        let r1 = w.wait(Duration::from_secs(5)).unwrap();
        let r2 = w.wait(Duration::from_secs(5)).unwrap();
        assert_eq!(r1, Reply { id: 1, body: Ok("A".into()) });
        assert_eq!(r2.id, 2);
        assert!(r2.body.is_err());
        drop(w);
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }

    fn serve(status: &'static str, body: &'static str) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ =
                    write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        format!("http://{addr}/x")
    }

    /// Blocks on a URL containing "slow" until released; counts calls.
    struct Gate {
        calls: Arc<AtomicUsize>,
        release: std::sync::Mutex<Receiver<()>>,
    }
    impl Fetch for Gate {
        fn get(&self, url: &str) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if url == "slow" {
                let _ = self.release.lock().unwrap().recv_timeout(Duration::from_secs(5));
            }
            Ok(url.into())
        }
    }

    #[test]
    fn drop_skips_queued_requests() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (go, release) = channel::<()>();
        let w = Worker::with_fetch(Gate { calls: calls.clone(), release: std::sync::Mutex::new(release) }, || {});
        assert!(w.request(1, "slow"));
        for i in 2..10 {
            assert!(w.request(i, "queued"));
        }
        // Wait until the first call is in flight, then release it from another
        // thread shortly after drop has set the stop flag.
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let _ = go.send(());
        });
        drop(w);
        t.join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn poll_is_non_blocking_and_request_after_ok() {
        let w = Worker::with_fetch(Echo, || {});
        assert!(w.poll().is_none());
        assert!(w.request(7, "x"));
        let r = w.wait(Duration::from_secs(5)).unwrap();
        assert_eq!(r, Reply { id: 7, body: Ok("X".into()) });
        assert!(w.wait(Duration::from_millis(10)).is_none());
    }

    #[test]
    fn url_rule() {
        assert!(url_allowed("https://api.open-meteo.com/v1/forecast?x=1"));
        assert!(url_allowed("HTTPS://api.github.com/repos/a/b/releases/latest"));
        assert!(url_allowed("http://127.0.0.1:8080/x"));
        assert!(url_allowed("http://localhost/x"));
        assert!(url_allowed("http://[::1]:9/x"));
        assert!(url_allowed("http://127.5.0.1"));
        assert!(!url_allowed("http://example.com/x"));
        assert!(!url_allowed("http://127.0.0.1.evil.com/x"));
        assert!(!url_allowed("http://localhost.evil.com/x"));
        assert!(!url_allowed("http://evil.com/?h=127.0.0.1"));
        assert!(!url_allowed("ftp://127.0.0.1/x"));
        assert!(!url_allowed("file:///c:/x"));
        assert!(!url_allowed(""));
        assert!(url_allowed("http://user@127.0.0.1/x"));
        assert!(!url_allowed("http://127.0.0.1@evil.com/x"));
        let e = Http::default().get("http://example.com/").unwrap_err();
        assert!(e.contains("non-https"), "{e}");
    }

    #[test]
    fn http_ok_and_non_2xx() {
        let h = Http::default();
        assert_eq!(h.get(&serve("200 OK", "{\"a\":1}")).unwrap(), "{\"a\":1}");
        assert!(h.get(&serve("500 Internal Server Error", "x")).is_err());
        assert!(h.get("http://127.0.0.1:1/").is_err());
    }
}
