# Grafana Dashboards

Version-controlled Grafana dashboards for the exchange.

## Dashboards

| File | UID | Purpose |
|------|-----|---------|
| `exchange-overview.json` | `exchange-overview` | Top-level: throughput, latency, errors, operating mode, WS connections, lock waits, WAL batch size |
| `engine-metrics.json` | `engine-metrics` | Per-symbol: matching latency p99, queue depth, orderbook depth, orders/fills rate |
| `task-health.json` | `task-health` | Background tasks: heartbeat age table, audit flusher pending, liquidation rate, insurance fund, withdrawal/deposit queues |

## Import

```bash
# Via Grafana HTTP API
curl -X POST http://admin:admin@localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  -d @ops/grafana/exchange-overview.json

# Or via Grafana UI: Dashboards → Import → Upload JSON file
```

## Data Source

All dashboards expect a Prometheus data source named `Prometheus` pointing
at the exchange's `/metrics` endpoint (protected by `X-Metrics-Key` header).

Configure the Prometheus scrape job:

```yaml
scrape_configs:
  - job_name: 'exchange'
    metrics_path: '/metrics'
    static_configs:
      - targets: ['localhost:8080']
    # X-Metrics-Key header if configured
    # headers:
    #   X-Metrics-Key: 'your-metrics-api-key'
```

## Alerts

Alert rules are defined in Alertmanager, not in these dashboards.
See `docs/slo.md` for burn-rate alerting targets and runbook links.
