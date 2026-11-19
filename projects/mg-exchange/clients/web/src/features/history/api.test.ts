import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getOrderHistory, getTradeHistory, getFundingHistory, getTaxReport } from './api';

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

describe('getOrderHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/orders/history with default limit', async () => {
    mockOkFetch([]);
    await getOrderHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/history?limit=50');
    expect(init.method).toBe('GET');
  });

  it('appends from and to params when provided', async () => {
    mockOkFetch([]);
    await getOrderHistory(1000000, 2000000, 100);
    const [url] = lastFetchArgs();
    expect(url).toContain('limit=100');
    expect(url).toContain('from=1000000');
    expect(url).toContain('to=2000000');
  });

  it('omits from/to params when undefined', async () => {
    mockOkFetch([]);
    await getOrderHistory(undefined, undefined, 25);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/orders/history?limit=25');
  });
});

describe('getTradeHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/trades/history with default limit', async () => {
    mockOkFetch([]);
    await getTradeHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/trades/history?limit=50');
    expect(init.method).toBe('GET');
  });

  it('appends from and to params when provided', async () => {
    mockOkFetch([]);
    await getTradeHistory(100, 200, 10);
    const [url] = lastFetchArgs();
    expect(url).toContain('from=100');
    expect(url).toContain('to=200');
    expect(url).toContain('limit=10');
  });
});

describe('getFundingHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/funding/history with default limit', async () => {
    mockOkFetch([]);
    await getFundingHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/funding/history?limit=50');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getFundingHistory(200);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/funding/history?limit=200');
  });
});

describe('getTaxReport', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/reports/tax', async () => {
    mockOkFetch({ year: 2025, total_gains: '5000', total_losses: '1000' });
    await getTaxReport(2025);
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/reports/tax');
    expect(init.method).toBe('POST');
  });

  it('sends year in request body', async () => {
    mockOkFetch({ year: 2024 });
    await getTaxReport(2024);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ year: 2024 });
  });

  it('returns tax report from server', async () => {
    const report = { year: 2026, total_gains: '10000', total_losses: '2000' };
    mockOkFetch(report);
    const result = await getTaxReport(2026);
    expect(result).toEqual(report);
  });
});
