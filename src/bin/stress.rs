// Fires N concurrent workers hammering /set + /get to see if the Mutex chokes.
// Usage: cargo run --release --bin stress -- [concurrency] [total_requests]
//   defaults: 200 concurrent, 100_000 total
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let concurrency: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(200);
    let total: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let base = "http://127.0.0.1:3000";

    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(concurrency)
        .build()
        .unwrap();

    let sem = Arc::new(Semaphore::new(concurrency));
    let latencies = Arc::new(std::sync::Mutex::new(Vec::<u128>::with_capacity(total as usize)));
    let errors = Arc::new(AtomicU64::new(0));

    let start = Instant::now();
    let mut handles = Vec::with_capacity(total as usize);
    for i in 0..total {
        let permit = sem.clone().acquire_owned().await.unwrap();
        let client = client.clone();
        let latencies = latencies.clone();
        let errors = errors.clone();
        handles.push(tokio::spawn(async move {
            let _permit = permit;
            let key = format!("k{}", i % 1000);
            let t = Instant::now();
            // half writes, half reads — both take the same lock
            let res = if i % 2 == 0 {
                client
                    .post(format!("{base}/set"))
                    .json(&serde_json::json!({ "key": key, "value": "v" }))
                    .send()
                    .await
            } else {
                client
                    .post(format!("{base}/get"))
                    .json(&serde_json::json!({ "key": key }))
                    .send()
                    .await
            };
            match res {
                Ok(r) if r.status().is_success() => {
                    latencies.lock().unwrap().push(t.elapsed().as_micros());
                }
                _ => {
                    errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    let elapsed = start.elapsed();

    let mut lat = Arc::try_unwrap(latencies).unwrap().into_inner().unwrap();
    lat.sort_unstable();
    let errs = errors.load(Ordering::Relaxed);
    let ok = lat.len();
    let pct = |p: f64| lat.get(((lat.len() as f64 * p) as usize).min(lat.len().saturating_sub(1))).copied().unwrap_or(0);

    println!("concurrency: {concurrency}, total: {total}");
    println!("elapsed: {:.2}s", elapsed.as_secs_f64());
    println!("throughput: {:.0} req/s", total as f64 / elapsed.as_secs_f64());
    println!("ok: {ok}, errors: {errs}");
    if ok > 0 {
        println!(
            "latency us  p50={} p90={} p99={} max={}",
            pct(0.50), pct(0.90), pct(0.99), lat.last().unwrap()
        );
    }
}
