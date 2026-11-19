import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getBurnHistory, getInsurancePool, getTokenSupply, stakeInsurance, unstakeInsurance } from './api';

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

describe('getBurnHistory', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/token/burns', async () => {
    mockOkFetch([]);
    await getBurnHistory();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/token/burns');
    expect(init.method).toBe('GET');
  });

  it('returns burn history from server', async () => {
    const records = [{ tx_hash: '0xabc', amount: '1000000', date: '2026-01-01' }];
    mockOkFetch(records);
    const result = await getBurnHistory();
    expect(result).toEqual(records);
  });
});

describe('getInsurancePool', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/token/insurance-pool', async () => {
    mockOkFetch({ total_staked: '10000000', apy: '8.5', my_stake: '0' });
    await getInsurancePool();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/token/insurance-pool');
    expect(init.method).toBe('GET');
  });

  it('returns insurance pool info from server', async () => {
    const info = { total_staked: '50000000', apy: '10.0', my_stake: '5000' };
    mockOkFetch(info);
    const result = await getInsurancePool();
    expect(result).toEqual(info);
  });
});

describe('getTokenSupply', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/token/supply', async () => {
    mockOkFetch({ total_supply: '1000000000', circulating_supply: '500000000', burned: '50000000' });
    await getTokenSupply();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/token/supply');
    expect(init.method).toBe('GET');
  });
});

describe('stakeInsurance', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/token/insurance-pool/stake', async () => {
    mockOkFetch(undefined, 204);
    await stakeInsurance('5000');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/token/insurance-pool/stake');
    expect(init.method).toBe('POST');
  });

  it('sends amount in request body', async () => {
    mockOkFetch(undefined, 204);
    await stakeInsurance('10000');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ amount: '10000' });
  });
});

describe('unstakeInsurance', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/token/insurance-pool/unstake', async () => {
    mockOkFetch(undefined, 204);
    await unstakeInsurance('2500');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/token/insurance-pool/unstake');
    expect(init.method).toBe('POST');
  });

  it('sends amount in request body', async () => {
    mockOkFetch(undefined, 204);
    await unstakeInsurance('1000');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ amount: '1000' });
  });
});
