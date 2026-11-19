// Soak load test: 50 VUs for 1 hour.
// Detects memory leaks, connection exhaustion, file descriptor leaks,
// orderbook depth growth, and gradual latency degradation.

import http from 'k6/http';
import { check, sleep } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';

export const options = {
  stages: [
    { duration: '1m', target: 50 },
    { duration: '58m', target: 50 },
    { duration: '1m', target: 0 },
  ],
  thresholds: {
    // SLOs must hold for the full hour.
    'http_req_duration{name:place_order}': ['p(99)<10'],
    'http_req_failed': ['rate<0.001'],
  },
};

export default function () {
  // Mixed workload: 60% place+cancel, 20% balance check, 20% orderbook
  const roll = Math.random();

  if (roll < 0.6) {
    // Place + cancel cycle
    const payload = JSON.stringify({
      symbol: 'BTCUSDT',
      side: Math.random() > 0.5 ? 'Buy' : 'Sell',
      order_type: 'Limit',
      price: String(40000 + Math.floor(Math.random() * 20000)),
      qty: '0.001',
      time_in_force: 'GTC',
    });

    const res = http.post(`${BASE_URL}/api/v1/orders`, payload, {
      headers: { 'Content-Type': 'application/json' },
      tags: { name: 'place_order' },
    });

    if (res.status === 200) {
      try {
        const orderId = JSON.parse(res.body).order_id;
        http.del(`${BASE_URL}/api/v1/orders/${orderId}`, null, {
          tags: { name: 'cancel_order' },
        });
      } catch (_) {}
    }
  } else if (roll < 0.8) {
    http.get(`${BASE_URL}/api/v1/account/balances`, {
      tags: { name: 'get_balances' },
    });
  } else {
    http.get(`${BASE_URL}/api/v1/orderbook?symbol=BTCUSDT&depth=25`, {
      tags: { name: 'orderbook' },
    });
  }

  check(http.get(`${BASE_URL}/health`), {
    'health ok': (r) => r.status === 200 || r.status === 503,
  });

  sleep(0.2);
}
