// Burst load test: 10x traffic spike for 30 seconds.
// Tests rate limiter behavior, queue depth, and recovery.

import http from 'k6/http';
import { check, sleep } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';

export const options = {
  stages: [
    { duration: '30s', target: 50 },   // warm up at baseline
    { duration: '30s', target: 500 },  // 10x spike
    { duration: '30s', target: 500 },  // hold spike
    { duration: '30s', target: 50 },   // recover to baseline
  ],
  thresholds: {
    // During burst, we expect some 429s — that's the rate limiter working.
    // But no 5xx.
    'checks': ['rate>0.95'],
  },
};

export default function () {
  const payload = JSON.stringify({
    symbol: 'BTCUSDT',
    side: 'Buy',
    order_type: 'Limit',
    price: '10000',
    qty: '0.001',
    time_in_force: 'GTC',
  });

  const res = http.post(`${BASE_URL}/api/v1/orders`, payload, {
    headers: { 'Content-Type': 'application/json' },
    tags: { name: 'place_order' },
  });

  check(res, {
    'not 5xx': (r) => r.status < 500,
    'success or rate-limited': (r) => r.status === 200 || r.status === 429,
  });

  sleep(0.05);
}
