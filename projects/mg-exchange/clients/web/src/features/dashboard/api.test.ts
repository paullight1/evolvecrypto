import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getWalletOverview, getRecentOrders, getOpenPositions } from './api';

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

describe('getWalletOverview', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/wallet', async () => {
    mockOkFetch({ total_equity_usd: '10000' });
    await getWalletOverview();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/wallet');
    expect(init.method).toBe('GET');
  });

  it('returns wallet overview from server', async () => {
    const overview = { total_usdt: '50000', pnl_24h: '-200' };
    mockOkFetch(overview);
    const result = await getWalletOverview();
    expect(result).toEqual(overview);
  });
});

describe('getRecentOrders', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/orders/history with default limit of 5', async () => {
    mockOkFetch([]);
    await getRecentOrders();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/history?limit=5');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit when provided', async () => {
    mockOkFetch([]);
    await getRecentOrders(10);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/history?limit=10');
  });
});

describe('getOpenPositions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/positions', async () => {
    mockOkFetch([]);
    await getOpenPositions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/positions');
    expect(init.method).toBe('GET');
  });

  it('returns positions from server', async () => {
    const positions = [{ symbol: 'BTCUSDT', size: '0.1', entry_price: '65000' }];
    mockOkFetch(positions);
    const result = await getOpenPositions();
    expect(result).toEqual(positions);
  });
});
