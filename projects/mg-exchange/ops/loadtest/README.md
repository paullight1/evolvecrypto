# Load Testing

Four profiles covering the standard exchange workload patterns.
Scripts use [k6](https://k6.io/) for HTTP/WS load generation with
coordinated-omission-correct latency measurement via HdrHistogram.

## Prerequisites

```bash
# Install k6
brew install k6    # macOS
# or: https://k6.io/docs/get-started/installation/

# Start the exchange
cargo run --release --bin exchange
```

## Profiles

| Profile | Script | Duration | Target | Purpose |
|---------|--------|----------|--------|---------|
| Baseline | `baseline.js` | 5 min | 100 VUs steady | Establish normal latency/throughput |
| Burst | `burst.js` | 2 min | 10x spike for 30s | Test rate limiter + queue depth |
| Ramp | `ramp.js` | 10 min | Linear 1→500 VUs | Find the breaking point |
| Soak | `soak.js` | 1 hour | 50 VUs steady | Detect memory leaks / drift |

## Running

```bash
# Single profile
k6 run ops/loadtest/baseline.js

# With custom base URL
k6 run -e BASE_URL=http://localhost:8080 ops/loadtest/baseline.js

# Export results to JSON for post-analysis
k6 run --out json=results.json ops/loadtest/baseline.js
```

## SLO Validation

Each script has thresholds matching `docs/slo.md`:
- `http_req_duration{p(99)}` < 10ms (order ack)
- `http_req_failed` < 0.1%
- No 5xx responses

## Notes

- k6 uses Go's net/http which avoids coordinated omission by default
  (each VU maintains its own connection and waits for the response).
- For sub-ms precision, complement with Criterion benchmarks in
  `crates/api-gateway/benches/`.
