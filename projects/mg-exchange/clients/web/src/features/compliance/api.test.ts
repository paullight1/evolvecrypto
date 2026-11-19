import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getKycStatus, startKyc } from './api';

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

describe('getKycStatus', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/kyc/status', async () => {
    mockOkFetch({ level: 0, status: 'none' });
    await getKycStatus();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/kyc/status');
    expect(init.method).toBe('GET');
  });

  it('returns kyc status from server', async () => {
    const status = { level: 1, status: 'approved', verified_at: '2026-01-01' };
    mockOkFetch(status);
    const result = await getKycStatus();
    expect(result).toEqual(status);
  });
});

describe('startKyc', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/kyc/start', async () => {
    mockOkFetch({ message: 'KYC started' });
    await startKyc(1);
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/kyc/start');
    expect(init.method).toBe('POST');
  });

  it('sends level in request body', async () => {
    mockOkFetch({ message: 'KYC level 2 started' });
    await startKyc(2);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ level: 2 });
  });

  it('returns message from server', async () => {
    mockOkFetch({ message: 'Please upload documents' });
    const result = await startKyc(1);
    expect(result).toEqual({ message: 'Please upload documents' });
  });
});
