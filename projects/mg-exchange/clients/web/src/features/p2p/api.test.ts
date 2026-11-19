import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  getP2PAds,
  createP2POrder,
  markPaid,
  releaseOrder,
  disputeOrder,
  cancelOrder,
  getMyOrders,
  createAd,
  getMyAds,
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

describe('getP2PAds', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/p2p/ads with side param', async () => {
    mockOkFetch([]);
    await getP2PAds('buy');
    const [url, init] = lastFetchArgs();
    expect(url).toContain('/api/v1/p2p/ads');
    expect(url).toContain('side=buy');
    expect(init.method).toBe('GET');
  });

  it('appends asset and fiat params when provided', async () => {
    mockOkFetch([]);
    await getP2PAds('sell', 'BTC', 'USD');
    const [url] = lastFetchArgs();
    expect(url).toContain('side=sell');
    expect(url).toContain('asset=BTC');
    expect(url).toContain('fiat=USD');
  });

  it('omits optional params when not provided', async () => {
    mockOkFetch([]);
    await getP2PAds('buy');
    const [url] = lastFetchArgs();
    expect(url).not.toContain('asset=');
    expect(url).not.toContain('fiat=');
  });
});

describe('createP2POrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/p2p/orders', async () => {
    mockOkFetch({ id: 'p2p-1', status: 'created' });
    await createP2POrder({ ad_id: 'ad-1', crypto_amount: '0.001', payment_method: 'bank_transfer' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/orders');
    expect(init.method).toBe('POST');
  });

  it('sends order request as JSON body', async () => {
    mockOkFetch({ id: 'p2p-2', status: 'created' });
    const req = { ad_id: 'ad-5', crypto_amount: '0.01', payment_method: 'paypal' as const };
    await createP2POrder(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('markPaid', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/p2p/orders/{id}/mark-paid', async () => {
    mockOkFetch(undefined, 204);
    await markPaid('order-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/orders/order-42/mark-paid');
    expect(init.method).toBe('POST');
  });
});

describe('releaseOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/p2p/orders/{id}/release', async () => {
    mockOkFetch(undefined, 204);
    await releaseOrder('order-55');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/orders/order-55/release');
    expect(init.method).toBe('POST');
  });
});

describe('disputeOrder', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/p2p/orders/{id}/dispute', async () => {
    mockOkFetch(undefined, 204);
    await disputeOrder('order-77');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/orders/order-77/dispute');
    expect(init.method).toBe('POST');
  });
});

describe('cancelOrder (p2p)', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/p2p/orders/{id}/cancel', async () => {
    mockOkFetch(undefined, 204);
    await cancelOrder('order-99');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/orders/order-99/cancel');
    expect(init.method).toBe('POST');
  });
});

describe('getMyOrders', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/p2p/orders', async () => {
    mockOkFetch([]);
    await getMyOrders();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/orders');
    expect(init.method).toBe('GET');
  });
});

describe('createAd', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/p2p/ads', async () => {
    mockOkFetch({ id: 'ad-1', status: 'active' });
    await createAd({ side: 'buy', asset: 'BTC', fiat: 'USD', price: '65000', min_amount: '100', max_amount: '5000', total_amount: '0.1', payment_methods: ['bank_transfer'] });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/ads');
    expect(init.method).toBe('POST');
  });

  it('sends ad request as JSON body', async () => {
    mockOkFetch({ id: 'ad-2', status: 'active' });
    const req = { side: 'sell' as const, asset: 'ETH', fiat: 'EUR', price: '3000', min_amount: '50', max_amount: '2000', total_amount: '1', payment_methods: ['sepa' as const] };
    await createAd(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('getMyAds', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/p2p/ads/my', async () => {
    mockOkFetch([]);
    await getMyAds();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/p2p/ads/my');
    expect(init.method).toBe('GET');
  });
});
