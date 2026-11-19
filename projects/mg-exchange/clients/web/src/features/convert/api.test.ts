import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getConvertQuote, executeConvert, getConvertHistory } from './api';

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

describe('getConvertQuote', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/convert/quote', async () => {
    mockOkFetch({ quote_id: 'q1', rate: '65000', expires_at: 1234567890 });
    await getConvertQuote('BTC', 'USDT', '0.1');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/convert/quote');
    expect(init.method).toBe('POST');
  });

  it('sends from_asset, to_asset, amount in body', async () => {
    mockOkFetch({ quote_id: 'q2', rate: '3000', expires_at: 1234567890 });
    await getConvertQuote('ETH', 'USDT', '1.5');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ from_asset: 'ETH', to_asset: 'USDT', amount: '1.5' });
  });

  it('returns the quote from the server', async () => {
    const quote = { quote_id: 'q3', rate: '100', expires_at: 9999 };
    mockOkFetch(quote);
    const result = await getConvertQuote('SOL', 'USDT', '10');
    expect(result).toEqual(quote);
  });
});

describe('executeConvert', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/convert/execute', async () => {
    mockOkFetch({ id: 'conv-1', status: 'completed' });
    await executeConvert('quote-abc');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/convert/execute');
    expect(init.method).toBe('POST');
  });

  it('sends quote_id in request body', async () => {
    mockOkFetch({ id: 'conv-2', status: 'completed' });
    await executeConvert('my-quote-id');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ quote_id: 'my-quote-id' });
  });

  it('returns conversion result from server', async () => {
    const resp = { id: 'conv-3', status: 'completed' };
    mockOkFetch(resp);
    const result = await executeConvert('q-xyz');
    expect(result).toEqual(resp);
  });
});

describe('getConvertHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/convert/history with default limit', async () => {
    mockOkFetch([]);
    await getConvertHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/convert/history?limit=20');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getConvertHistory(50);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/convert/history?limit=50');
  });
});
