//! Smoke run: `cargo run -p fp-net --example net_smoke -- --seconds 5 [url]`.
//! Fetches one URL on the worker and exits by itself.
use fp_net::Worker;
use std::time::Duration;

fn main() {
    let mut secs = 5u64;
    let mut url =
        "https://api.open-meteo.com/v1/forecast?latitude=52.52&longitude=13.41&current=weather_code".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--seconds" {
            secs = args.next().and_then(|v| v.parse().ok()).unwrap_or(secs);
        } else {
            url = a;
        }
    }
    let w = Worker::spawn(|| {});
    w.request(1, url);
    match w.wait(Duration::from_secs(secs)) {
        Some(r) => println!("reply {}: {:?}", r.id, r.body.map(|b| b.len())),
        None => println!("no reply in {secs}s"),
    }
    std::process::exit(0);
}
