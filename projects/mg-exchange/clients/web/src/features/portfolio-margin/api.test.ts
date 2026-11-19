import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getPortfolioMarginStatus,
  enablePortfolioMargin,
  disablePortfolioMargin,
  getPortfolioAnalysis,
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

describe('getPortfolioMarginStatus', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/portfolio-margin', async () => {
    mockOkFetch({ enabled: false, margin_level: '0' });
    await getPortfolioMarginStatus();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/portfolio-margin');
    expect(init.method).toBe('GET');
  });

  it('returns portfolio margin status from server', async () => {
    const status = { enabled: true, margin_level: '4.2', maintenance_margin: '10000' };
    mockOkFetch(status);
    const result = await getPortfolioMarginStatus();
    expect(result).toEqual(status);
  });
});

describe('enablePortfolioMargin', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/portfolio-margin/enable', async () => {
    mockOkFetch({ enabled: true, margin_level: '5.0' });
    await enablePortfolioMargin();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/portfolio-margin/enable');
    expect(init.method).toBe('POST');
  });

  it('returns updated status from server', async () => {
    const status = { enabled: true, margin_level: '5.0' };
    mockOkFetch(status);
    const result = await enablePortfolioMargin();
    expect(result).toEqual(status);
  });
});

describe('disablePortfolioMargin', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/portfolio-margin/disable', async () => {
    mockOkFetch({ enabled: false, margin_level: '0' });
    await disablePortfolioMargin();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/portfolio-margin/disable');
    expect(init.method).toBe('POST');
  });
});

describe('getPortfolioAnalysis', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/portfolio-margin/analysis', async () => {
    mockOkFetch({ total_margin_required: '5000', available_margin: '15000' });
    await getPortfolioAnalysis();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/portfolio-margin/analysis');
    expect(init.method).toBe('GET');
  });

  it('returns portfolio analysis from server', async () => {
    const analysis = { total_margin_required: '8000', available_margin: '12000', risk_level: 'low' };
    mockOkFetch(analysis);
    const result = await getPortfolioAnalysis();
    expect(result).toEqual(analysis);
  });
});
