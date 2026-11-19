# Exchange Load Tests

Pure-Rust load test tool for the exchange REST API. No external tooling (k6, Locust) required.

## Running

Start the exchange first:

```bash
cargo run -p exchange-server
```

Then run the load test from the repo root:

```bash
# Quick smoke test: 5 users, 30 seconds, mixed scenario
cargo run -p exchange-load-test -- --users 5 --duration 30

# Market-maker scenario: 20 users, 60 seconds, 5 orders/s per user
cargo run -p exchange-load-test -- \
  --url http://localhost:8080 \
  --users 20 \
  --duration 60 \
  --rate 5 \
  --scenario market_maker

# High-throughput mixed scenario: 50 users, 120 seconds, 20 orders/s per user
cargo run -p exchange-load-test -- \
  --users 50 \
  --duration 120 \
  --rate 20 \
  --scenario mixed
```

Release build for maximum throughput:

```bash
cargo build --release -p exchange-load-test
./target/release/load-test --users 100 --duration 120
```

## Scenarios

| Scenario | Description |
|---|---|
| `mixed` | 60% market orders, 40% limit orders across BTC/ETH/SOL (default) |
| `market_maker` | Continuously post limit bid + ask, then cancel both |

## Output

Progress is printed every 5 seconds. Final report includes:

- Total requests, successful, failed
- Error rate (%)
- Requests per second (RPS)
- Latency percentiles: p50, p95, p99 (microseconds)

Example:

```
========= LOAD TEST REPORT =========
Duration:        60.1s
Total requests:  12034
Successful:      11980
Failed:          54
Error rate:      0.45%
RPS:             200.2
Latency p50:     1842µs
Latency p95:     8210µs
Latency p99:     15430µs
====================================
```

## Flags

| Flag | Default | Description |
|---|---|---|
| `--url` | `http://localhost:8080` | Exchange base URL |
| `--users` | `10` | Concurrent virtual users |
| `--duration` | `60` | Test duration in seconds |
| `--rate` | `10` | Target iterations per second per user |
| `--scenario` | `mixed` | Scenario name |

Set `RUST_LOG=debug` to see per-request errors.
