// Baseline load test: steady 100 VUs for 5 minutes.
// Establishes normal latency and throughput under moderate load.

import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';

// Custom metrics
const orderLatency = new Trend('order_latency', true);
const orderFailRate = new Rate('order_fail_rate');

export const options = {
  stages: [
    { duration: '30s', target: 100 },  // ramp up
    { duration: '4m', target: 100 },   // steady state
    { duration: '30s', target: 0 },    // ramp down
  ],
  thresholds: {
    // SLO: order ack p99 < 10ms
    'http_req_duration{name:place_order}': ['p(99)<10'],
    // SLO: error rate < 0.1%
    'http_req_failed': ['rate<0.001'],
    // No 5xx
    'checks': ['rate>0.999'],
  },
};

// Each VU cycles through: place order → cancel order → check balance
export default function () {
  // 1. Place a limit order (far from market — won't fill)
  const orderPayload = JSON.stringify({
    symbol: 'BTCUSDT',
    side: 'Buy',
    order_type: 'Limit',
    price: '10000',   // far below market — rests on book
    qty: '0.001',
    time_in_force: 'GTC',
  });

  const placeRes = http.post(`${BASE_URL}/api/v1/orders`, orderPayload, {
    headers: { 'Content-Type': 'application/json' },
    tags: { name: 'place_order' },
  });

  orderLatency.add(placeRes.timings.duration);
  orderFailRate.add(placeRes.status !== 200);

  check(placeRes, {
    'place order status 200': (r) => r.status === 200,
    'place order has order_id': (r) => {
      try { return JSON.parse(r.body).order_id !== undefined; }
      catch { return false; }
    },
  });

  // 2. Cancel it if placement succeeded
  if (placeRes.status === 200) {
    try {
      const orderId = JSON.parse(placeRes.body).order_id;
      const cancelRes = http.del(`${BASE_URL}/api/v1/orders/${orderId}`, null, {
        tags: { name: 'cancel_order' },
      });
      check(cancelRes, {
        'cancel status 200': (r) => r.status === 200,
      });
    } catch (_) { /* ignore parse errors */ }
  }

  // 3. Read balances (lightweight GET)
  const balRes = http.get(`${BASE_URL}/api/v1/account/balances`, {
    tags: { name: 'get_balances' },
  });
  check(balRes, {
    'balances status 200': (r) => r.status === 200,
  });

  // 4. Read orderbook (public endpoint, no auth)
  http.get(`${BASE_URL}/api/v1/orderbook?symbol=BTCUSDT&depth=10`, {
    tags: { name: 'orderbook' },
  });

  sleep(0.1); // 100ms think time between iterations
}
