//! HTTPS client for Open-Meteo (forecast + geocoding), the GitHub "latest
//! release" check and the in-app updater's downloads ([`Downloader`]). Not
//! part of the coverage gate.
//!
//! Spec: weather-sources 2.3 (fetch, 20 s timeout, failures change nothing),
//! core-app 2.11 (update check: 20 s after start, then every 4 h).
//!
//! All calls block, so the app runs them on worker threads ([`Worker`], the
//! updater's own thread) and posts the results back to the UI thread.

use fp_sources::Fetch;
use fp_sources::install::Download;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
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
        Self { agent: tls_agent(ureq::AgentBuilder::new().timeout(TIMEOUT)) }
    }
}

/// `b` with the user agent and Windows TLS (schannel through native-tls).
fn tls_agent(b: ureq::AgentBuilder) -> ureq::Agent {
    let b = b.user_agent(concat!("Frostpane/", env!("CARGO_PKG_VERSION")));
    match native_tls::TlsConnector::new() {
        Ok(t) => b.tls_connector(Arc::new(t)).build(),
        Err(e) => {
            log::warn!("TLS init failed: {e}");
            b.build()
        }
    }
}

/// Redirects followed per download.
const MAX_REDIRECTS: usize = 5;
/// No byte for this long ends a download.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Release downloads for the updater. Redirects are followed by hand so
/// every hop passes the URL rule (`fp_sources::install::download_allowed`);
/// bodies are capped and hashed while they stream.
pub struct Downloader {
    agent: ureq::Agent,
    allowed: fn(&str) -> bool,
}

impl Downloader {
    /// With the release URL rule.
    pub fn release() -> Self {
        Self::with_rule(fp_sources::install::download_allowed)
    }

    /// With any URL rule (tests use loopback http).
    pub fn with_rule(allowed: fn(&str) -> bool) -> Self {
        let b = ureq::AgentBuilder::new().redirects(0).timeout_connect(TIMEOUT).timeout_read(READ_TIMEOUT);
        Self { agent: tls_agent(b), allowed }
    }

    /// GET `url`, following up to [`MAX_REDIRECTS`] allowed redirects.
    fn open(&self, url: &str) -> Result<ureq::Response, String> {
        let mut at = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            if !(self.allowed)(&at) {
                return Err(format!("refused URL: {at}"));
            }
            let resp = self.agent.get(&at).call().map_err(|e| e.to_string())?;
            if !(300..400).contains(&resp.status()) {
                return Ok(resp);
            }
            let next = resp.header("location").ok_or("redirect without a location")?;
            at = resolve(&at, next);
        }
        Err("too many redirects".into())
    }

    /// Streams the body of `url` to `out`, at most `max` bytes, and returns
    /// its SHA-256.
    fn pump(
        &self,
        url: &str,
        max: u64,
        out: &mut dyn Write,
        progress: &mut dyn FnMut(u64),
    ) -> Result<[u8; 32], String> {
        let resp = self.open(url)?;
        if let Some(len) = resp.header("content-length").and_then(|v| v.trim().parse::<u64>().ok())
            && len > max
        {
            return Err(format!("{url}: {len} bytes is more than the {max} allowed"));
        }
        let mut body = resp.into_reader().take(max + 1);
        let mut hash = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let n = body.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > max {
                return Err(format!("{url}: more than the {max} bytes allowed"));
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            progress(total);
        }
        Ok(hash.finalize().into())
    }
}

/// `next` (a Location header) relative to `base`.
fn resolve(base: &str, next: &str) -> String {
    if next.contains("://") {
        return next.to_string();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    let host_end = base[scheme_end..].find('/').map_or(base.len(), |i| scheme_end + i);
    if next.starts_with('/') {
        format!("{}{next}", &base[..host_end])
    } else {
        let dir = base[host_end..].rfind('/').map_or(host_end, |i| host_end + i);
        format!("{}/{next}", &base[..dir])
    }
}

impl Download for Downloader {
    fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        self.pump(url, max, &mut out, &mut |_| {})?;
        Ok(out)
    }

    fn file(&self, url: &str, dest: &Path, max: u64, progress: &mut dyn FnMut(u64)) -> Result<[u8; 32], String> {
        let mut f = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        let hash = self.pump(url, max, &mut f, progress)?;
        f.sync_all().map_err(|e| e.to_string())?;
        Ok(hash)
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

    /// Serves `routes` (path, raw response) to any number of connections
    /// until the test process ends.
    fn serve_many(routes: &'static [(&'static str, &'static str)]) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { break };
                let mut buf = [0u8; 2048];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/");
                let resp = routes.iter().find(|(p, _)| *p == path).map_or(NOT_FOUND, |r| r.1);
                let _ = s.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    fn loopback_only(u: &str) -> bool {
        url_allowed(u) && !u.contains("example.com")
    }

    const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const ROUTES: &[(&str, &str)] = &[
        ("/data", "HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload"),
        ("/redir", "HTTP/1.1 302 Found\r\nLocation: /data\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        ("/sub/rel", "HTTP/1.1 302 Found\r\nLocation: ../data\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        (
            "/evil",
            "HTTP/1.1 302 Found\r\nLocation: http://example.com/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        ),
        ("/loop", "HTTP/1.1 301 Moved\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        ("/nowhere", "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        ("/huge", "HTTP/1.1 200 OK\r\nContent-Length: 999999\r\nConnection: close\r\n\r\nabc"),
        (
            "/chunked",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n7\r\npayload\r\n0\r\n\r\n",
        ),
    ];

    /// SHA-256 of b"payload".
    const PAYLOAD_SHA: &str = "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5";

    #[test]
    fn downloads_follow_allowed_redirects_only() {
        let base = serve_many(ROUTES);
        let d = Downloader::with_rule(loopback_only);
        assert_eq!(d.bytes(&format!("{base}/data"), 100).unwrap(), b"payload");
        assert_eq!(d.bytes(&format!("{base}/redir"), 100).unwrap(), b"payload");
        // A relative location ("../data" from /sub/rel).
        assert_eq!(d.bytes(&format!("{base}/sub/rel"), 100).unwrap(), b"payload");
        assert!(d.bytes(&format!("{base}/missing"), 100).unwrap_err().contains("404"));
        let e = d.bytes(&format!("{base}/evil"), 100).unwrap_err();
        assert!(e.contains("refused URL: http://example.com/x"), "{e}");
        assert!(d.bytes(&format!("{base}/loop"), 100).unwrap_err().contains("too many redirects"));
        assert!(d.bytes(&format!("{base}/nowhere"), 100).unwrap_err().contains("location"));
        // The release rule refuses loopback outright.
        assert!(Downloader::release().bytes(&format!("{base}/data"), 100).unwrap_err().contains("refused"));
    }

    #[test]
    fn downloads_are_capped_and_hashed() {
        let base = serve_many(ROUTES);
        let d = Downloader::with_rule(loopback_only);
        assert!(d.bytes(&format!("{base}/huge"), 100).unwrap_err().contains("allowed"));
        // No length header: the stream itself is cut.
        assert!(d.bytes(&format!("{base}/chunked"), 6).unwrap_err().contains("allowed"));
        assert_eq!(d.bytes(&format!("{base}/chunked"), 7).unwrap(), b"payload");
        let dir = std::env::temp_dir().join(format!("fp-net-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("f.zip");
        let mut seen = Vec::new();
        let hash = d.file(&format!("{base}/redir"), &dest, 7, &mut |n| seen.push(n)).unwrap();
        assert_eq!(fp_sources::install::hex(&hash), PAYLOAD_SHA);
        assert_eq!(std::fs::read(&dest).unwrap(), b"payload");
        assert_eq!(seen.last(), Some(&7));
        assert!(d.file(&format!("{base}/data"), &dir.join("no/such/dir"), 7, &mut |_| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locations_resolve() {
        assert_eq!(resolve("https://a/b/c", "https://x/y"), "https://x/y");
        assert_eq!(resolve("https://a/b/c", "/y"), "https://a/y");
        assert_eq!(resolve("https://a/b/c", "y"), "https://a/b/y");
        assert_eq!(resolve("https://a", "/y"), "https://a/y");
        assert_eq!(resolve("https://a", "y"), "https://a/y");
    }

    #[test]
    fn http_ok_and_non_2xx() {
        let h = Http::default();
        assert_eq!(h.get(&serve("200 OK", "{\"a\":1}")).unwrap(), "{\"a\":1}");
        assert!(h.get(&serve("500 Internal Server Error", "x")).is_err());
        assert!(h.get("http://127.0.0.1:1/").is_err());
    }
}
