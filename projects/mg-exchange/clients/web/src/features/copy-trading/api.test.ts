import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getLeaderboard,
  followTrader,
  unfollowTrader,
  getMyFollowing,
  getCopyTradingSettings,
  updateSettings,
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

describe('getLeaderboard (copy-trading)', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/copy-trading/leaderboard', async () => {
    mockOkFetch([]);
    await getLeaderboard();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/copy-trading/leaderboard');
    expect(init.method).toBe('GET');
  });

  it('returns master traders from server', async () => {
    const traders = [{ user_id: 'u1', username: 'alice', roi: '150', followers: 42 }];
    mockOkFetch(traders);
    const result = await getLeaderboard();
    expect(result).toEqual(traders);
  });
});

describe('followTrader', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/copy-trading/follow', async () => {
    mockOkFetch({ relation_id: 'rel-1' });
    await followTrader({ master_id: 'trader-abc', allocation: '500' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/copy-trading/follow');
    expect(init.method).toBe('POST');
  });

  it('sends follow request as JSON body', async () => {
    mockOkFetch({ relation_id: 'rel-2' });
    const req = { master_id: 'top-trader', allocation: '1000' };
    await followTrader(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns relation_id from server', async () => {
    mockOkFetch({ relation_id: 'rel-xyz' });
    const result = await followTrader({ master_id: 'alice', allocation: '800' });
    expect(result).toEqual({ relation_id: 'rel-xyz' });
  });
});

describe('unfollowTrader', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends DELETE to /api/v1/copy-trading/follow/{relationId}', async () => {
    mockOkFetch(undefined, 204);
    await unfollowTrader('rel-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/copy-trading/follow/rel-42');
    expect(init.method).toBe('DELETE');
  });
});

describe('getMyFollowing', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/copy-trading/following', async () => {
    mockOkFetch([]);
    await getMyFollowing();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/copy-trading/following');
    expect(init.method).toBe('GET');
  });
});

describe('getCopyTradingSettings', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/copy-trading/settings', async () => {
    mockOkFetch({ max_copy_amount: '1000', risk_level: 'medium' });
    await getCopyTradingSettings();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/copy-trading/settings');
    expect(init.method).toBe('GET');
  });
});

describe('updateSettings (copy-trading)', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends PUT to /api/v1/copy-trading/settings', async () => {
    mockOkFetch({ max_copy_amount: '2000', risk_level: 'low' });
    await updateSettings({ max_copy_amount: '2000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/copy-trading/settings');
    expect(init.method).toBe('PUT');
  });

  it('sends partial settings as JSON body', async () => {
    mockOkFetch({ max_copy_amount: '500', risk_level: 'high' });
    await updateSettings({ risk_level: 'high' });
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ risk_level: 'high' });
  });
});
