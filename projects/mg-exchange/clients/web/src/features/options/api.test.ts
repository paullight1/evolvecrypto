import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getOptionInstruments,
  getOptionTickers,
  getOptionMarkPrices,
  getOptionPositions,
  getOptionGreeks,
  placeOptionOrder,
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

describe('getOptionInstruments', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/options/instruments with underlying', async () => {
    mockOkFetch({ instruments: [] });
    await getOptionInstruments('BTC');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/options/instruments?underlying=BTC');
    expect(init.method).toBe('GET');
  });

  it('interpolates underlying correctly', async () => {
    mockOkFetch({ instruments: [] });
    await getOptionInstruments('ETH');
    const [url] = lastFetchArgs();
    expect(url).toContain('underlying=ETH');
  });

  it('returns instruments from server', async () => {
    const instruments = [{ symbol: 'BTC-20260101-70000-C', optionType: 'Call', strike: '70000' }];
    mockOkFetch({ instruments });
    const result = await getOptionInstruments('BTC');
    expect(result).toEqual(instruments);
  });
});

describe('getOptionTickers', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/options/ticker with underlying', async () => {
    mockOkFetch({ tickers: [] });
    await getOptionTickers('BTC');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/options/ticker?underlying=BTC');
    expect(init.method).toBe('GET');
  });
});

describe('getOptionMarkPrices', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/options/mark-price with underlying', async () => {
    mockOkFetch([]);
    await getOptionMarkPrices('ETH');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/options/mark-price?underlying=ETH');
    expect(init.method).toBe('GET');
  });

  it('returns mark prices record from server', async () => {
    const prices = [
      { symbol: 'BTC-20260101-70000-C', markPrice: '1500', iv: '0.65', delta: '0.5', gamma: '0.001', theta: '-5', vega: '10' },
    ];
    mockOkFetch(prices);
    const result = await getOptionMarkPrices('BTC');
    expect(result).toEqual(prices);
  });
});

describe('getOptionPositions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/options/positions', async () => {
    mockOkFetch({ positions: [] });
    await getOptionPositions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/options/positions');
    expect(init.method).toBe('GET');
  });
});

describe('getOptionGreeks', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/options/greeks', async () => {
    mockOkFetch({ totalDelta: '0.5', totalGamma: '0.01', totalVega: '100', totalTheta: '-10' });
    await getOptionGreeks();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/options/greeks');
    expect(init.method).toBe('GET');
  });
});

describe('placeOptionOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/options/order', async () => {
    mockOkFetch({ orderId: 'opt-1', status: 'open' });
    await placeOptionOrder({ symbol: 'BTC-20260101-70000-C', side: 'buy', qty: '1', premium: '1500' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/options/order');
    expect(init.method).toBe('POST');
  });

  it('sends option order as JSON body', async () => {
    mockOkFetch({ orderId: 'opt-2', status: 'open' });
    const req = { symbol: 'ETH-20260201-3500-P', side: 'sell' as const, qty: '5', premium: '0' };
    await placeOptionOrder(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns order response from server', async () => {
    const resp = { orderId: 'opt-xyz', status: 'filled' };
    mockOkFetch(resp);
    const result = await placeOptionOrder({ symbol: 'BTC-20260101-70000-C', side: 'buy', qty: '1', premium: '1000' });
    expect(result).toEqual(resp);
  });
});
