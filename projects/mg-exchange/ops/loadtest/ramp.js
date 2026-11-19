// Ramp load test: linear increase from 1 to 500 VUs over 10 minutes.
// Finds the throughput ceiling and the VU count where SLOs break.

import http from 'k6/http';
import { check, sleep } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';

export const options = {
  stages: [
    { duration: '10m', target: 500 },
    { duration: '1m', target: 0 },
  ],
  thresholds: {
    // Track but don't fail — we WANT to find the breaking point.
    'http_req_duration{name:place_order}': [{ threshold: 'p(99)<50', abortOnFail: false }],
  },
};

export default function () {
  const payload = JSON.stringify({
    symbol: 'BTCUSDT',
    side: Math.random() > 0.5 ? 'Buy' : 'Sell',
    order_type: 'Limit',
    price: String(45000 + Math.floor(Math.random() * 10000)),
    qty: '0.001',
    time_in_force: 'GTC',
  });

  const res = http.post(`${BASE_URL}/api/v1/orders`, payload, {
    headers: { 'Content-Type': 'application/json' },
    tags: { name: 'place_order' },
  });

  check(res, {
    'not 5xx': (r) => r.status < 500,
  });

  // Also read market data to simulate realistic mixed workload
  http.get(`${BASE_URL}/api/v1/orderbook?symbol=BTCUSDT&depth=25`, {
    tags: { name: 'orderbook' },
  });

  sleep(0.05);
}
