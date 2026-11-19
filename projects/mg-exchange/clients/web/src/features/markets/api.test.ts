import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getAllMarkets } from './api';

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

describe('getAllMarkets', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/markets', async () => {
    mockOkFetch([]);
    await getAllMarkets();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/markets');
    expect(init.method).toBe('GET');
  });

  it('returns market list from server', async () => {
    const markets = [
      { symbol: 'BTCUSDT', base_asset: 'BTC', quote_asset: 'USDT', market_type: 'spot' },
      { symbol: 'ETHUSDT', base_asset: 'ETH', quote_asset: 'USDT', market_type: 'linear' },
    ];
    mockOkFetch(markets);
    const result = await getAllMarkets();
    expect(result).toEqual(markets);
  });

  it('returns empty array when no markets available', async () => {
    mockOkFetch([]);
    const result = await getAllMarkets();
    expect(result).toEqual([]);
  });
});
