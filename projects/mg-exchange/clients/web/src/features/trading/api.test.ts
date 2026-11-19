import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  placeOrder,
  cancelOrder,
  cancelAllOrders,
  getOpenOrders,
  getOrderHistory,
  getOrderBook,
  getKlines,
  getTicker,
  getRecentTrades,
  getPositions,
  getBalances,
} from './api';

// ---------------------------------------------------------------------------
// Fetch mock helpers
// ---------------------------------------------------------------------------

function mockOkFetch(body: unknown, status = 200): void {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: true,
      status,
      json: () => Promise.resolve(body),
    }),
  );
}

function lastFetchArgs(): [string, RequestInit] {
  return (vi.mocked(fetch).mock.calls[0] as unknown) as [string, RequestInit];
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('placeOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/orders', async () => {
    mockOkFetch({ order_id: 'o1', status: 'open', fills: [] });
    await placeOrder({ symbol: 'BTCUSDT', side: 'buy', order_type: 'limit', qty: '0.01', price: '65000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders');
    expect(init.method).toBe('POST');
  });

  it('sends the order body as JSON', async () => {
    mockOkFetch({ order_id: 'o2', status: 'open', fills: [] });
    const req = { symbol: 'ETHUSDT', side: 'sell' as const, order_type: 'market' as const, qty: '1' };
    await placeOrder(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns the PlaceOrderResponse from the server', async () => {
    const serverResp = { order_id: 'abc', status: 'filled', fills: [{ trade_id: 1, price: '65000', qty: '0.01', role: 'taker' }] };
    mockOkFetch(serverResp);
    const result = await placeOrder({ symbol: 'BTCUSDT', side: 'buy', order_type: 'limit', qty: '0.01' });
    expect(result).toEqual(serverResp);
  });
});

describe('cancelOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends DELETE to /api/v1/orders/{id}', async () => {
    mockOkFetch(undefined, 204);
    await cancelOrder('order-999');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/order-999');
    expect(init.method).toBe('DELETE');
  });

  it('interpolates the order id into the path', async () => {
    mockOkFetch(undefined, 204);
    await cancelOrder('my-special-id');
    const [url] = lastFetchArgs();
    expect(url).toContain('my-special-id');
  });
});

describe('cancelAllOrders', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/orders/cancel-all', async () => {
    mockOkFetch({ cancelled: 3 });
    await cancelAllOrders();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/cancel-all');
    expect(init.method).toBe('POST');
  });

  it('sends symbol in body when provided', async () => {
    mockOkFetch({ cancelled: 1 });
    await cancelAllOrders('ETHUSDT');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ symbol: 'ETHUSDT' });
  });

  it('sends undefined body when no symbol', async () => {
    mockOkFetch({ cancelled: 0 });
    await cancelAllOrders();
    const [, init] = lastFetchArgs();
    expect(init.body).toBeUndefined();
  });
});

describe('getOpenOrders', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/orders', async () => {
    mockOkFetch([]);
    await getOpenOrders();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders');
    expect(init.method).toBe('GET');
  });

  it('appends symbol query param when provided', async () => {
    mockOkFetch([]);
    await getOpenOrders('SOLUSDT');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders?symbol=SOLUSDT');
  });
});

describe('getOrderHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/orders/history with default params', async () => {
    mockOkFetch([]);
    await getOrderHistory();
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/history');
  });

  it('uses custom limit and offset when supplied', async () => {
    mockOkFetch([]);
    await getOrderHistory({ limit: 50, offset: 10 });
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/history?limit=50&offset=10');
  });
});

describe('getOrderBook', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/markets/{symbol}/orderbook with default depth', async () => {
    mockOkFetch({ bids: [], asks: [], sequence: 1 });
    await getOrderBook('BTCUSDT');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/BTCUSDT/orderbook?depth=25');
  });

  it('passes custom depth param', async () => {
    mockOkFetch({ bids: [], asks: [], sequence: 1 });
    await getOrderBook('ETHUSDT', 50);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/ETHUSDT/orderbook?depth=50');
  });

  it('includes the symbol in the URL', async () => {
    mockOkFetch({ bids: [], asks: [], sequence: 1 });
    await getOrderBook('SOLUSDT');
    const [url] = lastFetchArgs();
    expect(url).toContain('SOLUSDT');
  });
});

describe('getKlines', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET with symbol, interval and default limit', async () => {
    mockOkFetch([]);
    await getKlines('BTCUSDT', '1m');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/BTCUSDT/klines?interval=1m&limit=200');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getKlines('ETHUSDT', '1h', 100);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/ETHUSDT/klines?interval=1h&limit=100');
  });
});

describe('getTicker', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/markets/{symbol}/ticker', async () => {
    mockOkFetch({ symbol: 'BTCUSDT', last_price: '65000', price_change_pct_24h: '1.5', high_24h: '66000', low_24h: '64000', volume_24h: '1000', quote_volume_24h: '65000000' });
    await getTicker('BTCUSDT');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/BTCUSDT/ticker');
  });
});

describe('getRecentTrades', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET with default limit of 50', async () => {
    mockOkFetch([]);
    await getRecentTrades('BTCUSDT');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/BTCUSDT/trades?limit=50');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getRecentTrades('ETHUSDT', 10);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets/ETHUSDT/trades?limit=10');
  });
});

describe('getPositions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/positions', async () => {
    mockOkFetch([]);
    await getPositions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/positions');
    expect(init.method).toBe('GET');
  });
});

describe('getBalances (trading)', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/balances', async () => {
    mockOkFetch([]);
    await getBalances();
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/balances');
  });
});
