import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getLeveragedTokens, purchaseLt, redeemLt } from './api';

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

describe('getLeveragedTokens', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/leveraged-tokens', async () => {
    mockOkFetch([]);
    await getLeveragedTokens();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/leveraged-tokens');
    expect(init.method).toBe('GET');
  });

  it('returns leveraged tokens from server', async () => {
    const tokens = [{ id: 'lt-btc3l', symbol: 'BTC3L', underlying: 'BTC', leverage: '3', nav: '65000', change_24h: '1.5', total_supply: '100000', management_fee: '0.001' }];
    mockOkFetch(tokens);
    const result = await getLeveragedTokens();
    expect(result).toEqual(tokens);
  });
});

describe('purchaseLt', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/leveraged-tokens/purchase', async () => {
    mockOkFetch({ tokens_received: '1.5' });
    await purchaseLt({ token_id: 'lt-btc3l', usdt_amount: '100' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/leveraged-tokens/purchase');
    expect(init.method).toBe('POST');
  });

  it('sends purchase request as JSON body', async () => {
    mockOkFetch({ tokens_received: '3.0' });
    const req = { token_id: 'lt-eth3s', usdt_amount: '200' };
    await purchaseLt(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns tokens_received from server', async () => {
    mockOkFetch({ tokens_received: '5.25' });
    const result = await purchaseLt({ token_id: 'lt-btc3l', usdt_amount: '350' });
    expect(result).toEqual({ tokens_received: '5.25' });
  });
});

describe('redeemLt', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/leveraged-tokens/redeem', async () => {
    mockOkFetch({ usdt_received: '650' });
    await redeemLt({ token_id: 'lt-btc3l', token_amount: '10' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/leveraged-tokens/redeem');
    expect(init.method).toBe('POST');
  });

  it('sends redeem request as JSON body', async () => {
    mockOkFetch({ usdt_received: '300' });
    const req = { token_id: 'lt-eth3s', token_amount: '5' };
    await redeemLt(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns usdt_received from server', async () => {
    mockOkFetch({ usdt_received: '1200' });
    const result = await redeemLt({ token_id: 'lt-btc3l', token_amount: '18' });
    expect(result).toEqual({ usdt_received: '1200' });
  });
});
