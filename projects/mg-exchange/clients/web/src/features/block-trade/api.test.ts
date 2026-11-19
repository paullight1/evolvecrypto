import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createRfq, getRfqQuotes, executeRfq, getBlockTradeHistory } from './api';

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

describe('createRfq', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/block-trade/rfq', async () => {
    mockOkFetch({ id: 'rfq-1', status: 'open' });
    await createRfq({ symbol: 'BTCUSDT', side: 'buy', qty: '10', notional_usd: '650000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/block-trade/rfq');
    expect(init.method).toBe('POST');
  });

  it('sends RFQ request body as JSON', async () => {
    mockOkFetch({ id: 'rfq-2', status: 'open' });
    const req = { symbol: 'ETHUSDT', side: 'sell' as const, qty: '100', notional_usd: '300000' };
    await createRfq(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns the RFQ from the server', async () => {
    const rfq = { id: 'rfq-3', status: 'open', symbol: 'BTCUSDT' };
    mockOkFetch(rfq);
    const result = await createRfq({ symbol: 'BTCUSDT', side: 'buy', qty: '5', notional_usd: '325000' });
    expect(result).toEqual(rfq);
  });
});

describe('getRfqQuotes', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/block-trade/rfq/{rfqId}/quote', async () => {
    mockOkFetch([]);
    await getRfqQuotes('rfq-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/block-trade/rfq/rfq-42/quote');
    expect(init.method).toBe('GET');
  });

  it('interpolates rfqId into the path', async () => {
    mockOkFetch([]);
    await getRfqQuotes('my-rfq');
    const [url] = lastFetchArgs();
    expect(url).toContain('my-rfq');
  });
});

describe('executeRfq', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/block-trade/rfq/{rfqId}/execute', async () => {
    mockOkFetch({ trade_id: 'bt-1', status: 'filled' });
    await executeRfq('rfq-10', 'quote-5');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/block-trade/rfq/rfq-10/execute');
    expect(init.method).toBe('POST');
  });

  it('sends quote_id in request body', async () => {
    mockOkFetch({ trade_id: 'bt-2', status: 'filled' });
    await executeRfq('rfq-11', 'quote-99');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ quote_id: 'quote-99' });
  });
});

describe('getBlockTradeHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/block-trade/history with default limit', async () => {
    mockOkFetch([]);
    await getBlockTradeHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/block-trade/history?limit=20');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getBlockTradeHistory(50);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/block-trade/history?limit=50');
  });
});
