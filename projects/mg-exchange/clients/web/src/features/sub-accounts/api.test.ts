import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { createSubAccount, getSubAccounts, getSubAccountBalances } from './api';

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

describe('createSubAccount', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/sub-accounts', async () => {
    mockOkFetch({ sub_account_id: 'sa-1', label: 'Trading Bot', email: 'bot@example.com' });
    await createSubAccount('Trading Bot', 'bot@example.com');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/sub-accounts');
    expect(init.method).toBe('POST');
  });

  it('sends label and email as JSON body', async () => {
    mockOkFetch({ sub_account_id: 'sa-2', label: 'Arb', email: 'arb@x.com' });
    await createSubAccount('Arb', 'arb@x.com');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ label: 'Arb', email: 'arb@x.com' });
  });

  it('returns sub account from server', async () => {
    const sa = { sub_account_id: 'sa-99', label: 'Test', email: 'test@x.com' };
    mockOkFetch(sa);
    const result = await createSubAccount('Test', 'test@x.com');
    expect(result).toEqual(sa);
  });
});

describe('getSubAccounts', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/sub-accounts', async () => {
    mockOkFetch([]);
    await getSubAccounts();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/sub-accounts');
    expect(init.method).toBe('GET');
  });

  it('returns sub accounts list from server', async () => {
    const accounts = [{ sub_account_id: 'sa-1', label: 'Bot', email: 'bot@x.com' }];
    mockOkFetch(accounts);
    const result = await getSubAccounts();
    expect(result).toEqual(accounts);
  });
});

describe('getSubAccountBalances', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/sub-accounts/{id}/balances', async () => {
    mockOkFetch([]);
    await getSubAccountBalances('sa-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/sub-accounts/sa-42/balances');
    expect(init.method).toBe('GET');
  });

  it('interpolates sub account id into path', async () => {
    mockOkFetch([]);
    await getSubAccountBalances('my-sub');
    const [url] = lastFetchArgs();
    expect(url).toContain('my-sub');
  });

  it('returns balances from server', async () => {
    const balances = [{ asset: 'USDT', available: '5000', locked: '0', total: '5000' }];
    mockOkFetch(balances);
    const result = await getSubAccountBalances('sa-1');
    expect(result).toEqual(balances);
  });
});
