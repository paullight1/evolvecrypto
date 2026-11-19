import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getSpreadInstruments, placeSpreadOrder } from './api';

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

describe('getSpreadInstruments', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/spread/instruments', async () => {
    mockOkFetch([]);
    await getSpreadInstruments();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/spread/instruments');
    expect(init.method).toBe('GET');
  });

  it('returns spread instruments from server', async () => {
    const instruments = [{ id: 'sp-1', name: 'BTC Q1Q2', near_leg: 'BTC-2026Q1', far_leg: 'BTC-2026Q2', spread_value: '500', bid: '490', ask: '510', last: '500', volume_24h: '100' }];
    mockOkFetch(instruments);
    const result = await getSpreadInstruments();
    expect(result).toEqual(instruments);
  });
});

describe('placeSpreadOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/spread/order', async () => {
    mockOkFetch({ order_id: 'sp-1', status: 'open' });
    await placeSpreadOrder({ instrument_id: 'sp-btc-q1q2', side: 'buy', qty: '1', price: '500', order_type: 'limit' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/spread/order');
    expect(init.method).toBe('POST');
  });

  it('sends spread order as JSON body', async () => {
    mockOkFetch({ order_id: 'sp-2', status: 'open' });
    const req = { instrument_id: 'sp-eth-q1q2', side: 'sell' as const, qty: '5', order_type: 'market' as const };
    await placeSpreadOrder(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns order response from server', async () => {
    const resp = { order_id: 'sp-xyz', status: 'filled' };
    mockOkFetch(resp);
    const result = await placeSpreadOrder({ instrument_id: 'sp-btc', side: 'buy', qty: '2', order_type: 'limit', price: '100' });
    expect(result).toEqual(resp);
  });
});
