import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getMyBots, createGridBot, createDcaBot, pauseBot, stopBot } from './api';

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

describe('getMyBots', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/bots', async () => {
    mockOkFetch([]);
    await getMyBots();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/bots');
    expect(init.method).toBe('GET');
  });

  it('returns bot list from server', async () => {
    const bots = [{ id: 'b1', type: 'grid', status: 'running' }];
    mockOkFetch(bots);
    const result = await getMyBots();
    expect(result).toEqual(bots);
  });
});

describe('createGridBot', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/bots/grid', async () => {
    mockOkFetch({ bot_id: 'grid-1' });
    await createGridBot({ symbol: 'BTCUSDT', lower_price: '60000', upper_price: '70000', grid_count: 10, investment: '1000' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/bots/grid');
    expect(init.method).toBe('POST');
  });

  it('sends grid bot config as JSON body', async () => {
    mockOkFetch({ bot_id: 'grid-2' });
    const req = { symbol: 'ETHUSDT', lower_price: '3000', upper_price: '4000', grid_count: 5, investment: '500' };
    await createGridBot(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns bot_id from server', async () => {
    mockOkFetch({ bot_id: 'grid-99' });
    const result = await createGridBot({ symbol: 'BTCUSDT', lower_price: '60000', upper_price: '70000', grid_count: 10, investment: '1000' });
    expect(result).toEqual({ bot_id: 'grid-99' });
  });
});

describe('createDcaBot', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/bots/dca', async () => {
    mockOkFetch({ bot_id: 'dca-1' });
    await createDcaBot({ symbol: 'BTCUSDT', amount: '100', interval_hours: 24, total_orders: 30 });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/bots/dca');
    expect(init.method).toBe('POST');
  });

  it('sends DCA config as JSON body', async () => {
    mockOkFetch({ bot_id: 'dca-2' });
    const req = { symbol: 'SOLUSDT', amount: '50', interval_hours: 1, total_orders: 100 };
    await createDcaBot(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('pauseBot', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/bots/{botId}/pause', async () => {
    mockOkFetch(undefined, 204);
    await pauseBot('bot-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/bots/bot-42/pause');
    expect(init.method).toBe('POST');
  });
});

describe('stopBot', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/bots/{botId}/stop', async () => {
    mockOkFetch(undefined, 204);
    await stopBot('bot-99');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/bots/bot-99/stop');
    expect(init.method).toBe('POST');
  });

  it('interpolates botId into path', async () => {
    mockOkFetch(undefined, 204);
    await stopBot('my-grid-bot');
    const [url] = lastFetchArgs();
    expect(url).toContain('my-grid-bot');
  });
});
