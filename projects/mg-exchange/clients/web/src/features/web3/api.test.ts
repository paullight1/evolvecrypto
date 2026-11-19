import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getDexChains,
  getDexQuote,
  executeDexSwap,
  getSmartRoute,
  createWeb3Wallet,
  getWeb3Balances,
  getBridgeRoutes,
  getBridgeQuote,
  executeBridge,
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

describe('getDexChains', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/dex/chains', async () => {
    mockOkFetch([]);
    await getDexChains();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/dex/chains');
    expect(init.method).toBe('GET');
  });

  it('returns chains from server', async () => {
    const chains = [{ id: 'ethereum', name: 'Ethereum', native_token: 'ETH', rpc_url: '', explorer_url: '' }];
    mockOkFetch(chains);
    const result = await getDexChains();
    expect(result).toEqual(chains);
  });
});

describe('getDexQuote', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/dex/quote', async () => {
    mockOkFetch({ from_token: 'ETH', to_token: 'USDC', from_amount: '1', to_amount: '3000', price_impact: '0.1', fee: '0.003', route: [], expires_at: 9999 });
    await getDexQuote({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC', amount: '1' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/dex/quote');
    expect(init.method).toBe('POST');
  });

  it('sends quote request as JSON body', async () => {
    mockOkFetch({ from_token: 'BNB', to_token: 'BUSD', from_amount: '0.5', to_amount: '150', price_impact: '0.05', fee: '0.001', route: [], expires_at: 9999 });
    const req = { chain: 'bsc', from_token: 'BNB', to_token: 'BUSD', amount: '0.5' };
    await getDexQuote(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns quote from server', async () => {
    const quote = { from_token: 'ETH', to_token: 'USDC', from_amount: '1', to_amount: '3000', price_impact: '0.2', fee: '0.005', route: [], expires_at: 9999 };
    mockOkFetch(quote);
    const result = await getDexQuote({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC', amount: '1' });
    expect(result).toEqual(quote);
  });
});

describe('executeDexSwap', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/dex/swap', async () => {
    mockOkFetch({ tx_hash: '0xabc123' });
    await executeDexSwap({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC', amount: '1', slippage: '0.5' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/dex/swap');
    expect(init.method).toBe('POST');
  });

  it('sends swap request as JSON body', async () => {
    mockOkFetch({ tx_hash: '0xdef456' });
    const req = { chain: 'bsc', from_token: 'BNB', to_token: 'BUSD', amount: '1', slippage: '1' };
    await executeDexSwap(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns tx_hash from server', async () => {
    mockOkFetch({ tx_hash: '0xdeadbeef' });
    const result = await executeDexSwap({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC', amount: '1', slippage: '1' });
    expect(result).toEqual({ tx_hash: '0xdeadbeef' });
  });
});

describe('getSmartRoute', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/dex/route', async () => {
    mockOkFetch({ from_token: 'ETH', to_token: 'USDC', from_amount: '1', to_amount: '3100', price_impact: '0.05', fee: '0.002', route: [], expires_at: 9999 });
    await getSmartRoute({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC', amount: '1' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/dex/route');
    expect(init.method).toBe('POST');
  });

  it('sends route request as JSON body', async () => {
    mockOkFetch({ from_token: 'MATIC', to_token: 'USDT', from_amount: '100', to_amount: '98', price_impact: '0.01', fee: '0.001', route: [], expires_at: 9999 });
    const req = { chain: 'polygon', from_token: 'MATIC', to_token: 'USDT', amount: '100' };
    await getSmartRoute(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('createWeb3Wallet', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/web3/wallet', async () => {
    mockOkFetch({ address: '0xabc', chain: 'ethereum', created_at: '2026-01-01' });
    await createWeb3Wallet('ethereum');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/web3/wallet');
    expect(init.method).toBe('POST');
  });

  it('sends chain in request body', async () => {
    mockOkFetch({ address: '0x123', chain: 'bsc', created_at: '2026-01-01' });
    await createWeb3Wallet('bsc');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ chain: 'bsc' });
  });
});

describe('getWeb3Balances', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/web3/wallet', async () => {
    mockOkFetch([]);
    await getWeb3Balances();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/web3/wallet');
    expect(init.method).toBe('GET');
  });

  it('returns token balances from server', async () => {
    const balances = [{ token: '0x...', symbol: 'ETH', balance: '1.5', usd_value: '4500', chain: 'ethereum' }];
    mockOkFetch(balances);
    const result = await getWeb3Balances();
    expect(result).toEqual(balances);
  });
});

describe('getBridgeRoutes', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/web3/bridge/routes', async () => {
    mockOkFetch([]);
    await getBridgeRoutes({ from_chain: 'ethereum', to_chain: 'polygon', token: 'USDC' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/web3/bridge/routes');
    expect(init.method).toBe('POST');
  });

  it('sends bridge routes request as JSON body', async () => {
    mockOkFetch([]);
    const req = { from_chain: 'ethereum', to_chain: 'bsc', token: 'ETH' };
    await getBridgeRoutes(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getBridgeQuote', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/web3/bridge/quote', async () => {
    mockOkFetch({ route: {}, from_amount: '100', to_amount: '99.5', fee_usd: '0.5', expires_at: 9999 });
    await getBridgeQuote({ from_chain: 'ethereum', to_chain: 'polygon', token: 'USDC', amount: '100', bridge: 'hop' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/web3/bridge/quote');
    expect(init.method).toBe('POST');
  });

  it('sends bridge quote request as JSON body', async () => {
    mockOkFetch({ route: {}, from_amount: '50', to_amount: '49.8', fee_usd: '0.2', expires_at: 9999 });
    const req = { from_chain: 'ethereum', to_chain: 'bsc', token: 'ETH', amount: '0.05', bridge: 'synapse' };
    await getBridgeQuote(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('executeBridge', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/web3/bridge/execute', async () => {
    mockOkFetch({ tx_hash: '0xbridge123' });
    await executeBridge({ from_chain: 'ethereum', to_chain: 'polygon', token: 'USDC', amount: '100', bridge: 'hop', to_address: '0xabc' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/web3/bridge/execute');
    expect(init.method).toBe('POST');
  });

  it('sends bridge execute request as JSON body', async () => {
    mockOkFetch({ tx_hash: '0xexec456' });
    const req = { from_chain: 'bsc', to_chain: 'polygon', token: 'BNB', amount: '1', bridge: 'multichain', to_address: '0xdef' };
    await executeBridge(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns tx_hash from server', async () => {
    mockOkFetch({ tx_hash: '0xresult' });
    const result = await executeBridge({ from_chain: 'ethereum', to_chain: 'polygon', token: 'USDC', amount: '100', bridge: 'hop', to_address: '0x999' });
    expect(result).toEqual({ tx_hash: '0xresult' });
  });
});
