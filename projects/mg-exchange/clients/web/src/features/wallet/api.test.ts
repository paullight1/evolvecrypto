import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getBalances,
  getDepositAddress,
  submitWithdrawal,
  getDeposits,
  getWithdrawals,
  getTransfers,
  internalTransfer,
  getWithdrawalFee,
} from './api';

// ---------------------------------------------------------------------------
// Fetch mock helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('getBalances', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/balances', async () => {
    mockOkFetch([]);
    await getBalances();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/balances');
    expect(init.method).toBe('GET');
  });

  it('returns the balance array from the server', async () => {
    const balances = [{ asset: 'BTC', available: '1.0', locked: '0', total: '1.0' }];
    mockOkFetch(balances);
    const result = await getBalances();
    expect(result).toEqual(balances);
  });
});

describe('getDepositAddress', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/deposit-address with asset and network in body', async () => {
    mockOkFetch({ address: 'bc1q...', network: 'BTC', min_deposit: '0.0001' });
    await getDepositAddress('BTC', 'mainnet');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/deposit-address');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ asset: 'BTC', network: 'mainnet' });
  });

  it('sends asset and network in the request body for ETH/ERC20', async () => {
    mockOkFetch({ address: '0xabc', network: 'ERC20', min_deposit: '0.01' });
    await getDepositAddress('ETH', 'ERC20');
    const [, init] = lastFetchArgs();
    const body = JSON.parse(init.body as string) as { asset: string; network: string };
    expect(body.asset).toBe('ETH');
    expect(body.network).toBe('ERC20');
  });

  it('returns DepositAddress from server', async () => {
    const addr = { address: 'bc1q123', network: 'BTC', min_deposit: '0.0001', memo: undefined };
    mockOkFetch(addr);
    const result = await getDepositAddress('BTC', 'BTC');
    expect(result.address).toBe('bc1q123');
  });
});

describe('submitWithdrawal', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/withdraw', async () => {
    mockOkFetch({ id: 'wd-001' });
    await submitWithdrawal({ asset: 'BTC', network: 'BTC', address: 'bc1q...', amount: '0.1' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/withdraw');
    expect(init.method).toBe('POST');
  });

  it('sends the withdrawal request body as JSON', async () => {
    mockOkFetch({ id: 'wd-002' });
    const req = { asset: 'ETH', network: 'ERC20', address: '0xfoo', amount: '1.5' };
    await submitWithdrawal(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns the withdrawal id from the server', async () => {
    mockOkFetch({ id: 'wd-xyz' });
    const result = await submitWithdrawal({ asset: 'USDT', network: 'TRC20', address: 'T...', amount: '100' });
    expect(result).toEqual({ id: 'wd-xyz' });
  });
});

describe('getDeposits', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/deposits with default limit', async () => {
    mockOkFetch([]);
    await getDeposits();
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/deposits?limit=20');
  });

  it('uses custom limit', async () => {
    mockOkFetch([]);
    await getDeposits(50);
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/deposits?limit=50');
  });
});

describe('getWithdrawals', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/withdrawals with default limit', async () => {
    mockOkFetch([]);
    await getWithdrawals();
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/withdrawals?limit=20');
  });
});

describe('getTransfers', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/transfers with default limit', async () => {
    mockOkFetch([]);
    await getTransfers();
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/transfers?limit=20');
  });
});

describe('internalTransfer', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/transfer', async () => {
    mockOkFetch({ id: 'tr-001' });
    await internalTransfer({ asset: 'USDT', amount: '100', from_account: 'spot', to_account: 'futures' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/transfer');
    expect(init.method).toBe('POST');
  });

  it('sends transfer body as JSON', async () => {
    mockOkFetch({ id: 'tr-002' });
    const req = { asset: 'BTC', amount: '0.5', from_account: 'spot', to_account: 'futures' };
    await internalTransfer(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getWithdrawalFee', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/fees with asset and network params', async () => {
    mockOkFetch({ fee: '0.0005', min_amount: '0.001', max_amount: '10' });
    await getWithdrawalFee('BTC', 'BTC');
    const [url] = lastFetchArgs();
    expect(url).toBe('/api/v1/fees?asset=BTC&network=BTC');
  });

  it('returns fee structure from server', async () => {
    const fee = { fee: '1.00', min_amount: '10', max_amount: '50000' };
    mockOkFetch(fee);
    const result = await getWithdrawalFee('USDT', 'TRC20');
    expect(result).toEqual(fee);
  });
});
