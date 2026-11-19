import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createTwap, createVwap, getAlgoOrders, cancelAlgo } from './api';

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

describe('createTwap', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/algo/twap', async () => {
    mockOkFetch({ id: 'a1', type: 'twap', status: 'running' });
    await createTwap({ symbol: 'BTCUSDT', side: 'buy', qty: '1', duration_seconds: 3600, slices: 10 });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/algo/twap');
    expect(init.method).toBe('POST');
  });

  it('sends TWAP request as JSON body', async () => {
    mockOkFetch({ id: 'a2', type: 'twap', status: 'running' });
    const req = { symbol: 'ETHUSDT', side: 'sell' as const, qty: '10', duration_seconds: 1800, slices: 6 };
    await createTwap(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns the algo order from the server', async () => {
    const order = { id: 'a3', type: 'twap', status: 'running', symbol: 'BTCUSDT' };
    mockOkFetch(order);
    const result = await createTwap({ symbol: 'BTCUSDT', side: 'buy', qty: '1', duration_seconds: 600, slices: 5 });
    expect(result).toEqual(order);
  });
});

describe('createVwap', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/algo/vwap', async () => {
    mockOkFetch({ id: 'v1', type: 'vwap', status: 'running' });
    await createVwap({ symbol: 'BTCUSDT', side: 'buy', qty: '2', duration_seconds: 7200 });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/algo/vwap');
    expect(init.method).toBe('POST');
  });

  it('sends VWAP request as JSON body', async () => {
    mockOkFetch({ id: 'v2', type: 'vwap', status: 'running' });
    const req = { symbol: 'SOLUSDT', side: 'sell' as const, qty: '100', duration_seconds: 3600 };
    await createVwap(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getAlgoOrders', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/algo/orders without filter', async () => {
    mockOkFetch([]);
    await getAlgoOrders();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/algo/orders');
    expect(init.method).toBe('GET');
  });

  it('appends status query param when provided', async () => {
    mockOkFetch([]);
    await getAlgoOrders('running');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/algo/orders?status=running');
  });

  it('appends completed status correctly', async () => {
    mockOkFetch([]);
    await getAlgoOrders('completed');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/algo/orders?status=completed');
  });
});

describe('cancelAlgo', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends DELETE to /api/v1/algo/orders/{id}', async () => {
    mockOkFetch(undefined, 204);
    await cancelAlgo('algo-99');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/algo/orders/algo-99');
    expect(init.method).toBe('DELETE');
  });

  it('interpolates the algo id into the path', async () => {
    mockOkFetch(undefined, 204);
    await cancelAlgo('my-twap-id');
    const [url] = lastFetchArgs();
    expect(url).toContain('my-twap-id');
  });
});
