import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getCompetitions, getLeaderboard, joinCompetition } from './api';

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

describe('getCompetitions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/competitions', async () => {
    mockOkFetch([]);
    await getCompetitions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/competitions');
    expect(init.method).toBe('GET');
  });

  it('returns competitions from server', async () => {
    const comps = [{ competition_id: 'c1', name: 'Trading Cup', status: 'active' }];
    mockOkFetch(comps);
    const result = await getCompetitions();
    expect(result).toEqual(comps);
  });
});

describe('getLeaderboard', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/competitions/{id}/leaderboard', async () => {
    mockOkFetch([]);
    await getLeaderboard('comp-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/competitions/comp-42/leaderboard');
    expect(init.method).toBe('GET');
  });

  it('interpolates competition id into path', async () => {
    mockOkFetch([]);
    await getLeaderboard('trading-cup-2026');
    const [url] = lastFetchArgs();
    expect(url).toContain('trading-cup-2026');
  });

  it('returns leaderboard entries from server', async () => {
    const entries = [{ rank: 1, user: 'alice', pnl: '5000' }];
    mockOkFetch(entries);
    const result = await getLeaderboard('c1');
    expect(result).toEqual(entries);
  });
});

describe('joinCompetition', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/competitions/{id}/join', async () => {
    mockOkFetch(undefined, 204);
    await joinCompetition({ competition_id: 'comp-5' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/competitions/comp-5/join');
    expect(init.method).toBe('POST');
  });

  it('interpolates competition_id from the join request', async () => {
    mockOkFetch(undefined, 204);
    await joinCompetition({ competition_id: 'my-comp' });
    const [url] = lastFetchArgs();
    expect(url).toContain('my-comp');
  });
});
