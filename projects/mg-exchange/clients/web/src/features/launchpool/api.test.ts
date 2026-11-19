import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getPools, stakeInPool, unstakeFromPool, claimRewards, getMyStakes } from './api';

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

describe('getPools', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/launchpool/pools', async () => {
    mockOkFetch([]);
    await getPools();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpool/pools');
    expect(init.method).toBe('GET');
  });

  it('returns pools from server', async () => {
    const pools = [{ id: 'pool-1', reward_token: 'NTK', stake_token: 'BNB', apy: '120', total_staked: '1000000', max_stake: '100000', start_time: '', end_time: '', status: 'active' }];
    mockOkFetch(pools);
    const result = await getPools();
    expect(result).toEqual(pools);
  });
});

describe('stakeInPool', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/launchpool/stake', async () => {
    mockOkFetch(undefined, 204);
    await stakeInPool({ pool_id: 'pool-1', amount: '1000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpool/stake');
    expect(init.method).toBe('POST');
  });

  it('sends stake request as JSON body', async () => {
    mockOkFetch(undefined, 204);
    const req = { pool_id: 'pool-2', amount: '500' };
    await stakeInPool(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('unstakeFromPool', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/launchpool/unstake', async () => {
    mockOkFetch(undefined, 204);
    await unstakeFromPool({ pool_id: 'pool-1', amount: '500' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpool/unstake');
    expect(init.method).toBe('POST');
  });

  it('sends unstake request as JSON body', async () => {
    mockOkFetch(undefined, 204);
    const req = { pool_id: 'pool-3', amount: '200' };
    await unstakeFromPool(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('claimRewards', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/launchpool/claim', async () => {
    mockOkFetch(undefined, 204);
    await claimRewards('pool-1');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpool/claim');
    expect(init.method).toBe('POST');
  });

  it('sends pool_id in request body', async () => {
    mockOkFetch(undefined, 204);
    await claimRewards('my-pool');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ pool_id: 'my-pool' });
  });
});

describe('getMyStakes', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/launchpool/my-stakes', async () => {
    mockOkFetch([]);
    await getMyStakes();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpool/my-stakes');
    expect(init.method).toBe('GET');
  });

  it('returns user stakes from server', async () => {
    const stakes = [{ id: 's1', pool_id: 'pool-1', reward_token: 'NTK', stake_token: 'BNB', staked_amount: '1000', rewards_earned: '50', staked_at: 1000000 }];
    mockOkFetch(stakes);
    const result = await getMyStakes();
    expect(result).toEqual(stakes);
  });
});
