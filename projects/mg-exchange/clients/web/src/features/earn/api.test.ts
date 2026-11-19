import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getEarnProducts,
  earnSubscribe,
  earnRedeem,
  getEarnAllocations,
  stakingStake,
  stakingUnstake,
  stakingClaimRewards,
  getStakingBalance,
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

describe('getEarnProducts', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/earn/products', async () => {
    mockOkFetch([]);
    await getEarnProducts();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/products');
    expect(init.method).toBe('GET');
  });

  it('returns earn products from server', async () => {
    const products = [{ product_id: 'ep-1', asset: 'USDT', apy: '5.5' }];
    mockOkFetch(products);
    const result = await getEarnProducts();
    expect(result).toEqual(products);
  });
});

describe('earnSubscribe', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/earn/subscribe', async () => {
    mockOkFetch({ allocation_id: 'alloc-1' });
    await earnSubscribe({ product_id: 'ep-1', amount: '1000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/subscribe');
    expect(init.method).toBe('POST');
  });

  it('sends subscribe request as JSON body', async () => {
    mockOkFetch({ allocation_id: 'alloc-2' });
    const req = { product_id: 'ep-2', amount: '500' };
    await earnSubscribe(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('earnRedeem', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/earn/redeem', async () => {
    mockOkFetch(undefined, 204);
    await earnRedeem({ allocation_id: 'alloc-1' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/redeem');
    expect(init.method).toBe('POST');
  });

  it('sends redeem request as JSON body', async () => {
    mockOkFetch(undefined, 204);
    const req = { allocation_id: 'alloc-99' };
    await earnRedeem(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getEarnAllocations', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/earn/allocations', async () => {
    mockOkFetch([]);
    await getEarnAllocations();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/allocations');
    expect(init.method).toBe('GET');
  });
});

describe('stakingStake', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/staking/stake', async () => {
    mockOkFetch(undefined, 204);
    await stakingStake({ asset: 'BNB', amount: '10' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/staking/stake');
    expect(init.method).toBe('POST');
  });

  it('sends stake request as JSON body', async () => {
    mockOkFetch(undefined, 204);
    const req = { asset: 'ETH', amount: '2' };
    await stakingStake(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('stakingUnstake', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/staking/unstake', async () => {
    mockOkFetch(undefined, 204);
    await stakingUnstake({ asset: 'BNB', amount: '5' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/staking/unstake');
    expect(init.method).toBe('POST');
  });
});

describe('stakingClaimRewards', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/staking/claim', async () => {
    mockOkFetch(undefined, 204);
    await stakingClaimRewards('ETH');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/staking/claim-rewards');
    expect(init.method).toBe('POST');
  });

  it('sends asset in request body', async () => {
    mockOkFetch(undefined, 204);
    await stakingClaimRewards('SOL');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ asset: 'SOL' });
  });
});

describe('getStakingBalance', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/staking/balance with asset query param', async () => {
    mockOkFetch({ asset: 'ETH', staked: '5', rewards: '0.1' });
    await getStakingBalance('ETH');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/earn/staking/balance?asset=ETH');
    expect(init.method).toBe('GET');
  });

  it('interpolates asset into query string', async () => {
    mockOkFetch({ asset: 'BNB', staked: '10', rewards: '0.05' });
    await getStakingBalance('BNB');
    const [url] = lastFetchArgs();
    expect(url).toContain('asset=BNB');
  });
});
