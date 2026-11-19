import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getAdminStats,
  getKycPending,
  getAmlAlerts,
  getPendingWithdrawals,
  approveKyc,
  rejectKyc,
  approveWithdrawal,
  rejectWithdrawal,
  getSystemHealth,
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

describe('getAdminStats', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/admin/stats', async () => {
    mockOkFetch({ total_users: 100, total_volume: '1000000' });
    await getAdminStats();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/stats');
    expect(init.method).toBe('GET');
  });

  it('returns admin stats from the server', async () => {
    const stats = { total_users: 500, total_volume: '5000000' };
    mockOkFetch(stats);
    const result = await getAdminStats();
    expect(result).toEqual(stats);
  });
});

describe('getKycPending', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/admin/kyc/pending', async () => {
    mockOkFetch([]);
    await getKycPending();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/kyc/pending');
    expect(init.method).toBe('GET');
  });
});

describe('getAmlAlerts', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/admin/aml/alerts', async () => {
    mockOkFetch([]);
    await getAmlAlerts();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/aml/alerts');
    expect(init.method).toBe('GET');
  });
});

describe('getPendingWithdrawals', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/admin/withdrawals/pending', async () => {
    mockOkFetch([]);
    await getPendingWithdrawals();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/withdrawals/pending');
    expect(init.method).toBe('GET');
  });
});

describe('approveKyc', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/admin/kyc/{userId}/approve', async () => {
    mockOkFetch(undefined, 204);
    await approveKyc('user-123', 2);
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/kyc/user-123/approve');
    expect(init.method).toBe('POST');
  });

  it('sends level in request body', async () => {
    mockOkFetch(undefined, 204);
    await approveKyc('u-99', 3);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ level: 3 });
  });
});

describe('rejectKyc', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/admin/kyc/{userId}/reject', async () => {
    mockOkFetch(undefined, 204);
    await rejectKyc('user-456', 'docs incomplete');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/kyc/user-456/reject');
    expect(init.method).toBe('POST');
  });

  it('sends reason in request body', async () => {
    mockOkFetch(undefined, 204);
    await rejectKyc('u-77', 'invalid id');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ reason: 'invalid id' });
  });
});

describe('approveWithdrawal', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/admin/withdrawals/{id}/approve', async () => {
    mockOkFetch(undefined, 204);
    await approveWithdrawal('wd-001');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/withdrawals/wd-001/approve');
    expect(init.method).toBe('POST');
  });
});

describe('rejectWithdrawal', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/admin/withdrawals/{id}/reject', async () => {
    mockOkFetch(undefined, 204);
    await rejectWithdrawal('wd-002', 'suspicious');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/admin/withdrawals/wd-002/reject');
    expect(init.method).toBe('POST');
  });

  it('sends reason in request body', async () => {
    mockOkFetch(undefined, 204);
    await rejectWithdrawal('wd-003', 'AML flag');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ reason: 'AML flag' });
  });
});

describe('getSystemHealth', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/health/detailed', async () => {
    mockOkFetch({ status: 'ok', services: [] });
    await getSystemHealth();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/health/detailed');
    expect(init.method).toBe('GET');
  });
});
