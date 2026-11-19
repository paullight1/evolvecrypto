mod scenarios;
mod stats;

use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tracing::info;

use crate::scenarios::Scenario;
use crate::stats::LoadTestStats;

#[derive(Parser)]
#[command(name = "load-test", about = "Exchange load test tool")]
struct Args {
    /// Base URL of the exchange
    #[arg(long, default_value = "http://localhost:8080")]
    url: String,
    /// Number of concurrent users
    #[arg(long, default_value = "10")]
    users: u32,
    /// Test duration in seconds
    #[arg(long, default_value = "60")]
    duration: u64,
    /// Orders per second per user (rate-limits each user's iteration loop)
    #[arg(long, default_value = "10")]
    rate: u32,
    /// Scenario: "market_maker", "mixed"
    #[arg(long, default_value = "mixed")]
    scenario: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    info!(
        url = %args.url,
        users = args.users,
        duration_secs = args.duration,
        rate = args.rate,
        scenario = %args.scenario,
        "starting load test"
    );

    let stats = Arc::new(LoadTestStats::new());
    let base_url = Arc::new(args.url.clone());
    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build HTTP client"),
    );

    let test_duration = Duration::from_secs(args.duration);
    let iter_delay = Duration::from_micros(1_000_000 / args.rate.max(1) as u64);

    // Spawn progress reporter task
    let stats_for_reporter = stats.clone();
    let reporter = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let total = stats_for_reporter.total_requests.load(std::sync::atomic::Ordering::Relaxed);
            let ok = stats_for_reporter.successful.load(std::sync::atomic::Ordering::Relaxed);
            let err = stats_for_reporter.failed.load(std::sync::atomic::Ordering::Relaxed);
            info!(total, ok, err, "progress");
        }
    });

    // Spawn user tasks
    let start = Instant::now();
    let mut handles = Vec::with_capacity(args.users as usize);

    for user_id in 0..args.users {
        let client = client.clone();
        let base_url = base_url.clone();
        let stats = stats.clone();
        let scenario_name = args.scenario.clone();

        let handle = tokio::spawn(async move {
            run_user(
                user_id,
                scenario_name,
                client,
                base_url,
                stats,
                test_duration,
                iter_delay,
            )
            .await;
        });
        handles.push(handle);
    }

    // Wait for all users
    for h in handles {
        let _ = h.await;
    }
    reporter.abort();

    let elapsed = start.elapsed().as_secs_f64();
    let report = stats.report(elapsed);
    print_report(&report);
}

async fn run_user(
    user_id: u32,
    scenario_name: String,
    client: Arc<reqwest::Client>,
    base_url: Arc<String>,
    stats: Arc<LoadTestStats>,
    duration: Duration,
    iter_delay: Duration,
) {
    let deadline = Instant::now() + duration;

    loop {
        if Instant::now() >= deadline {
            break;
        }

        let iter_start = Instant::now();

        match scenario_name.as_str() {
            "market_maker" => {
                let s = scenarios::market_maker::MarketMakerScenario::new("BTCUSDT");
                s.run(client.clone(), base_url.clone(), stats.clone()).await;
            }
            "mixed" | _ => {
                let s = scenarios::mixed::MixedScenario::new();
                s.run(client.clone(), base_url.clone(), stats.clone()).await;
            }
        }

        // Rate limit: sleep the remainder of the iteration window.
        let elapsed = iter_start.elapsed();
        if elapsed < iter_delay {
            tokio::time::sleep(iter_delay - elapsed).await;
        }
    }

    tracing::debug!(user_id, "user done");
}

fn print_report(report: &stats::LoadTestReport) {
    println!("\n========= LOAD TEST REPORT =========");
    println!("Duration:        {:.1}s", report.duration_secs);
    println!("Total requests:  {}", report.total_requests);
    println!("Successful:      {}", report.successful);
    println!("Failed:          {}", report.failed);
    println!("Error rate:      {:.2}%", report.error_rate_pct);
    println!("RPS:             {:.1}", report.rps);
    println!("Latency p50:     {}µs", report.p50_us);
    println!("Latency p95:     {}µs", report.p95_us);
    println!("Latency p99:     {}µs", report.p99_us);
    println!("====================================\n");
}
