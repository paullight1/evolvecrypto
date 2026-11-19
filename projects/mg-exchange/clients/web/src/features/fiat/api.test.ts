import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getFiatMethods, getFiatRate, placeFiatOrder, getFiatHistory } from './api';

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

describe('getFiatMethods', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/fiat/methods', async () => {
    mockOkFetch([]);
    await getFiatMethods();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/fiat/methods');
    expect(init.method).toBe('GET');
  });

  it('returns fiat methods from server', async () => {
    const methods = [{ id: 'bank', name: 'Bank Transfer', type: 'bank_transfer', currencies: ['USD', 'EUR'], min_amount: '10', max_amount: '10000', fee_pct: '1.5' }];
    mockOkFetch(methods);
    const result = await getFiatMethods();
    expect(result).toEqual(methods);
  });
});

describe('getFiatRate', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/fiat/rates with fiat and crypto params', async () => {
    mockOkFetch({ fiat_currency: 'USD', crypto_asset: 'BTC', rate: '65000', last_updated: '2026-01-01' });
    await getFiatRate('USD', 'BTC');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/fiat/rates?fiat=USD&crypto=BTC');
    expect(init.method).toBe('GET');
  });

  it('interpolates fiat and crypto correctly', async () => {
    mockOkFetch({ fiat_currency: 'EUR', crypto_asset: 'ETH', rate: '3000', last_updated: '2026-01-01' });
    await getFiatRate('EUR', 'ETH');
    const [url] = lastFetchArgs();
    expect(url).toContain('fiat=EUR');
    expect(url).toContain('crypto=ETH');
  });
});

describe('placeFiatOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/fiat/deposit', async () => {
    mockOkFetch({ id: 'fiat-1', status: 'pending' });
    await placeFiatOrder({ type: 'buy', fiat_currency: 'USD', crypto_asset: 'BTC', fiat_amount: '100', payment_method_id: 'bank-1' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/fiat/deposit');
    expect(init.method).toBe('POST');
  });

  it('sends fiat order request as JSON body', async () => {
    mockOkFetch({ id: 'fiat-2', status: 'pending' });
    const req = { type: 'sell' as const, fiat_currency: 'EUR', crypto_asset: 'ETH', fiat_amount: '500', payment_method_id: 'card-1' };
    await placeFiatOrder(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getFiatHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/fiat/history with default limit', async () => {
    mockOkFetch([]);
    await getFiatHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/fiat/history?limit=20');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getFiatHistory(100);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/fiat/history?limit=100');
  });
});
