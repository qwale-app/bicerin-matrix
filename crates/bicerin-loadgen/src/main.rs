use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use std::{sync::{atomic::{AtomicUsize, Ordering}, Arc, Mutex}, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Operation {
    Sync,
    Send,
}

#[derive(Clone, Debug, Parser)]
#[command(name = "bicerin-loadgen", about = "Measure Bicerin request latency against a live deployment")]
struct Args {
    #[arg(long)]
    base_url: String,
    #[arg(long)]
    access_token: String,
    #[arg(long, value_enum, default_value_t = Operation::Sync)]
    operation: Operation,
    #[arg(long, default_value_t = 100)]
    requests: usize,
    #[arg(long, default_value_t = 10)]
    concurrency: usize,
    #[arg(long)]
    room_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.requests == 0 || args.concurrency == 0 {
        bail!("requests and concurrency must be greater than zero");
    }
    if matches!(args.operation, Operation::Send) && args.room_id.is_none() {
        bail!("--room-id is required for send benchmarks");
    }

    let client = reqwest::Client::new();
    let next = Arc::new(AtomicUsize::new(0));
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(args.requests)));
    let failures = Arc::new(AtomicUsize::new(0));
    let workers = args.concurrency.min(args.requests);
    let started = Instant::now();
    let mut tasks = Vec::with_capacity(workers);

    for _ in 0..workers {
        let client = client.clone();
        let args = args.clone();
        let next = Arc::clone(&next);
        let latencies = Arc::clone(&latencies);
        let failures = Arc::clone(&failures);
        tasks.push(tokio::spawn(async move {
            loop {
                let request_number = next.fetch_add(1, Ordering::Relaxed);
                if request_number >= args.requests {
                    break;
                }
                let started = Instant::now();
                let result = issue_request(&client, &args, request_number).await;
                let elapsed = started.elapsed();
                if result.is_ok() {
                    latencies.lock().expect("latency lock poisoned").push(elapsed);
                } else {
                    failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }
    for task in tasks {
        task.await.context("load worker panicked")?;
    }

    let elapsed = started.elapsed();
    let mut latencies = latencies.lock().expect("latency lock poisoned").clone();
    latencies.sort_unstable();
    let completed = latencies.len();
    println!("operation={:?} attempted={} completed={} failed={} elapsed_ms={}", args.operation, args.requests, completed, failures.load(Ordering::Relaxed), elapsed.as_millis());
    if completed > 0 {
        println!("p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} throughput_rps={:.2}", millis(percentile(&latencies, 0.50)), millis(percentile(&latencies, 0.95)), millis(percentile(&latencies, 0.99)), completed as f64 / elapsed.as_secs_f64());
    }
    if completed == 0 || failures.load(Ordering::Relaxed) > 0 {
        bail!("load run had failed requests");
    }
    Ok(())
}

async fn issue_request(client: &reqwest::Client, args: &Args, request_number: usize) -> Result<()> {
    let base = args.base_url.trim_end_matches('/');
    let response = match args.operation {
        Operation::Sync => client.get(format!("{base}/_matrix/client/v3/sync?timeout=0")),
        Operation::Send => client.put(format!(
            "{base}/_matrix/client/v3/rooms/{}/send/m.room.message/loadgen-{}",
            args.room_id.as_deref().expect("room id checked"),
            request_number
        )).json(&serde_json::json!({"msgtype": "m.text", "body": "bicerin-loadgen"})),
    }
    .bearer_auth(&args.access_token)
    .send()
    .await
    .context("HTTP request failed")?;
    if !response.status().is_success() {
        bail!("HTTP {}", response.status());
    }
    Ok(())
}

fn percentile(latencies: &[Duration], percentile: f64) -> Duration {
    let index = ((latencies.len() as f64 * percentile).ceil() as usize).saturating_sub(1);
    latencies[index.min(latencies.len().saturating_sub(1))]
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::percentile;
    use std::time::Duration;

    #[test]
    fn percentile_uses_nearest_rank() {
        let values = [1, 2, 3, 4, 5].map(Duration::from_millis);
        assert_eq!(percentile(&values, 0.50), Duration::from_millis(3));
        assert_eq!(percentile(&values, 0.95), Duration::from_millis(5));
    }
}