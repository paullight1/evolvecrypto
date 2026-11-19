import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getDeliveryInstruments,
  placeDeliveryOrder,
  getDeliveryPositions,
  getDeliveryPrice,
  getSettlementPrices,
} from './api';

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

describe('getDeliveryInstruments', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/futures/delivery/instruments', async () => {
    mockOkFetch([]);
    await getDeliveryInstruments();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/futures/delivery/instruments');
    expect(init.method).toBe('GET');
  });

  it('returns instruments from server', async () => {
    const instruments = [{ symbol: 'BTC-2026Q1', expiry: '2026-03-31', settlement: 'cash' }];
    mockOkFetch(instruments);
    const result = await getDeliveryInstruments();
    expect(result).toEqual(instruments);
  });
});

describe('placeDeliveryOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/futures/delivery/order', async () => {
    mockOkFetch({ order_id: 'do-1', status: 'open' });
    await placeDeliveryOrder({ symbol: 'BTC-2026Q1', side: 'buy', qty: '1', order_type: 'limit', price: '70000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/futures/delivery/order');
    expect(init.method).toBe('POST');
  });

  it('sends order request as JSON body', async () => {
    mockOkFetch({ order_id: 'do-2', status: 'open' });
    const req = { symbol: 'ETH-2026Q1', side: 'sell' as const, qty: '10', order_type: 'market' as const };
    await placeDeliveryOrder(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getDeliveryPositions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/futures/delivery/positions', async () => {
    mockOkFetch([]);
    await getDeliveryPositions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/futures/delivery/positions');
    expect(init.method).toBe('GET');
  });
});

describe('getDeliveryPrice', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/futures/delivery/price/{symbol}', async () => {
    mockOkFetch({ symbol: 'BTC-2026Q1', delivery_price: '68000' });
    await getDeliveryPrice('BTC-2026Q1');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/futures/delivery/price/BTC-2026Q1');
    expect(init.method).toBe('GET');
  });

  it('interpolates symbol into path', async () => {
    mockOkFetch({ symbol: 'ETH-2026Q2', delivery_price: '3500' });
    await getDeliveryPrice('ETH-2026Q2');
    const [url] = lastFetchArgs();
    expect(url).toContain('ETH-2026Q2');
  });
});

describe('getSettlementPrices', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/market/delivery-prices with default limit', async () => {
    mockOkFetch([]);
    await getSettlementPrices();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/market/delivery-prices?limit=20');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getSettlementPrices(50);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/market/delivery-prices?limit=50');
  });
});
