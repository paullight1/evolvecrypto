import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  toggleSpotMargin,
  getMarginStatus,
  borrowMargin,
  repayMargin,
  getActiveBorrows,
  getInterestHistory,
  getBorrowableAssets,
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

describe('toggleSpotMargin', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/margin/toggle', async () => {
    mockOkFetch({ enabled: true, margin_level: '2.5' });
    await toggleSpotMargin(true);
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/toggle');
    expect(init.method).toBe('POST');
  });

  it('sends enable flag in request body', async () => {
    mockOkFetch({ enabled: false, margin_level: '0' });
    await toggleSpotMargin(false);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ enable: false });
  });
});

describe('getMarginStatus', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/margin/status', async () => {
    mockOkFetch({ enabled: true, margin_level: '3.0' });
    await getMarginStatus();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/status');
    expect(init.method).toBe('GET');
  });
});

describe('borrowMargin', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/margin/borrow', async () => {
    mockOkFetch(undefined, 204);
    await borrowMargin({ asset: 'USDT', amount: '1000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/borrow');
    expect(init.method).toBe('POST');
  });

  it('sends borrow request as JSON body', async () => {
    mockOkFetch(undefined, 204);
    const req = { asset: 'BTC', amount: '0.1' };
    await borrowMargin(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('repayMargin', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/margin/repay', async () => {
    mockOkFetch(undefined, 204);
    await repayMargin({ asset: 'USDT', amount: '500' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/repay');
    expect(init.method).toBe('POST');
  });

  it('sends repay request as JSON body', async () => {
    mockOkFetch(undefined, 204);
    const req = { asset: 'ETH', amount: '1.5' };
    await repayMargin(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getActiveBorrows', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/margin/borrows', async () => {
    mockOkFetch([]);
    await getActiveBorrows();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/borrows');
    expect(init.method).toBe('GET');
  });
});

describe('getInterestHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/margin/interest-history with default limit', async () => {
    mockOkFetch([]);
    await getInterestHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/interest-history?limit=50');
    expect(init.method).toBe('GET');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getInterestHistory(100);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/interest-history?limit=100');
  });
});

describe('getBorrowableAssets', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/margin/borrowable', async () => {
    mockOkFetch([]);
    await getBorrowableAssets();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/margin/borrowable');
    expect(init.method).toBe('GET');
  });

  it('returns borrowable assets from server', async () => {
    const assets = [{ asset: 'BTC', max_borrowable: '1.5', interest_rate: '0.0001' }];
    mockOkFetch(assets);
    const result = await getBorrowableAssets();
    expect(result).toEqual(assets);
  });
});
