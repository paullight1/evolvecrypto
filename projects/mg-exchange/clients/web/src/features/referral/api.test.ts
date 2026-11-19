import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getReferralStats, getCommissions, getReferralTree, sendInvite } from './api';

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

describe('getReferralStats', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/referral', async () => {
    mockOkFetch({ referral_code: 'REF123', total_referrals: 5, total_earned: '250' });
    await getReferralStats();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/referral');
    expect(init.method).toBe('GET');
  });

  it('returns normalised referral stats from server', async () => {
    // Backend returns raw shape, api.ts normalises to ReferralStats contract
    mockOkFetch({ referral_code: 'ABCDE', total_referred: 10, commission_rate: '0.20' });
    const result = await getReferralStats();
    expect(result.referral_code).toBe('ABCDE');
    expect(result.total_referrals).toBe(10);
    expect(result.commission_rate).toBe('0.20');
    // Normalised fields filled with defaults
    expect(result.referral_url).toBe('');
    expect(result.active_referrals).toBe(0);
    expect(result.total_commissions_usdt).toBe('0');
  });
});

describe('getCommissions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/referral/commissions', async () => {
    mockOkFetch([]);
    await getCommissions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/referral/commissions');
    expect(init.method).toBe('GET');
  });

  it('returns commission list from server', async () => {
    const comms = [{ id: 'c1', amount: '10', asset: 'USDT', date: '2026-01-01' }];
    mockOkFetch(comms);
    const result = await getCommissions();
    expect(result).toEqual(comms);
  });
});

describe('getReferralTree', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/referral/tree', async () => {
    mockOkFetch([]);
    await getReferralTree();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/referral/tree');
    expect(init.method).toBe('GET');
  });
});

describe('sendInvite', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/referral/invite', async () => {
    mockOkFetch(undefined, 204);
    await sendInvite('friend@example.com');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/referral/invite');
    expect(init.method).toBe('POST');
  });

  it('sends email in request body', async () => {
    mockOkFetch(undefined, 204);
    await sendInvite('another@test.com');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ email: 'another@test.com' });
  });
});
