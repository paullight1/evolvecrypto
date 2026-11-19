import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { request, get, post, del, ApiError } from './client';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function mockFetch(
  body: unknown,
  options: { status?: number; ok?: boolean } = {},
): void {
  const status = options.status ?? 200;
  const ok = options.ok ?? status < 400;
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok,
      status,
      statusText: 'Error',
      json: () => Promise.resolve(body),
    }),
  );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ApiError', () => {
  it('has name "ApiError"', () => {
    const err = new ApiError(400, 'bad request');
    expect(err.name).toBe('ApiError');
  });

  it('exposes status code', () => {
    const err = new ApiError(404, 'not found');
    expect(err.status).toBe(404);
  });

  it('exposes message', () => {
    const err = new ApiError(500, 'server error');
    expect(err.message).toBe('server error');
  });

  it('is instance of Error', () => {
    expect(new ApiError(400, 'oops')).toBeInstanceOf(Error);
  });
});

describe('request — success paths', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('sends GET request to the correct URL', async () => {
    mockFetch({ ok: true });
    await request('GET', '/test');
    const fetchMock = vi.mocked(fetch);
    expect(fetchMock).toHaveBeenCalledOnce();
    const [url] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/v1/test');
  });

  it('uses GET method by default for get()', async () => {
    mockFetch({ data: 1 });
    await get('/ping');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe('GET');
  });

  it('sends POST method for post()', async () => {
    mockFetch({ id: '1' });
    await post('/orders', { symbol: 'BTCUSDT' });
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe('POST');
  });

  it('serialises body as JSON for post()', async () => {
    mockFetch({ id: '1' });
    await post('/orders', { symbol: 'BTCUSDT', qty: '0.01' });
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.body).toBe(JSON.stringify({ symbol: 'BTCUSDT', qty: '0.01' }));
  });

  it('sends DELETE method for del()', async () => {
    mockFetch(undefined, { status: 204, ok: true });
    await del('/orders/abc');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe('DELETE');
  });

  it('does not send body for GET requests', async () => {
    mockFetch({ items: [] });
    await get('/items');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.body).toBeUndefined();
  });

  it('returns undefined for 204 No Content', async () => {
    mockFetch(undefined, { status: 204, ok: true });
    const result = await request('DELETE', '/orders/xyz');
    expect(result).toBeUndefined();
  });

  it('includes Authorization header when access_token is present', async () => {
    sessionStorage.setItem('access_token', 'tok_abc');
    mockFetch({ data: true });
    await get('/protected');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = init.headers as Record<string, string>;
    expect(headers['Authorization']).toBe('Bearer tok_abc');
  });

  it('omits Authorization header when no token is stored', async () => {
    mockFetch({ data: true });
    await get('/public');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = init.headers as Record<string, string>;
    expect(headers['Authorization']).toBeUndefined();
  });

  it('always sends Content-Type: application/json', async () => {
    mockFetch({ ok: true });
    await get('/anything');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = init.headers as Record<string, string>;
    expect(headers['Content-Type']).toBe('application/json');
  });
});

describe('request — error paths', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('throws ApiError on non-ok response', async () => {
    mockFetch({ error: 'Not found' }, { status: 404, ok: false });
    await expect(get('/missing')).rejects.toBeInstanceOf(ApiError);
  });

  it('ApiError carries the correct status code', async () => {
    mockFetch({ error: 'Forbidden' }, { status: 403, ok: false });
    const err = await get('/secret').catch((e: unknown) => e);
    expect((err as ApiError).status).toBe(403);
  });

  it('extracts error message from JSON body', async () => {
    mockFetch({ error: 'Insufficient balance' }, { status: 400, ok: false });
    const err = await post('/orders', {}).catch((e: unknown) => e);
    expect((err as ApiError).message).toBe('Insufficient balance');
  });

  it('uses "Request failed" when JSON has no error field', async () => {
    mockFetch({}, { status: 500, ok: false });
    const err = await get('/boom').catch((e: unknown) => e);
    expect((err as ApiError).message).toBe('Request failed');
  });

  it('on 401 with no refresh token: clears sessionStorage tokens', async () => {
    sessionStorage.setItem('access_token', 'old_tok');
    // First call: 401. tryRefreshToken will also fail (no refresh token) so clearSession runs.
    mockFetch({ error: 'Unauthorized' }, { status: 401, ok: false });
    await expect(get('/private')).rejects.toBeInstanceOf(ApiError);
    expect(sessionStorage.getItem('access_token')).toBeNull();
  });

  it('on 401 with no refresh token: throws ApiError with status 401', async () => {
    mockFetch({ error: 'Unauthorized' }, { status: 401, ok: false });
    const err = await get('/private').catch((e: unknown) => e);
    expect((err as ApiError).status).toBe(401);
    expect((err as ApiError).message).toBe('Session expired');
  });
});

// ---------------------------------------------------------------------------
// Network errors
// ---------------------------------------------------------------------------

describe('request — network errors', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('throws when fetch rejects (network failure)', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')));
    await expect(get('/anything')).rejects.toThrow();
  });

  it('thrown error on network failure is not swallowed silently', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('Failed to fetch')));
    const err = await get('/anything').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(Error);
  });
});

// ---------------------------------------------------------------------------
// Server error codes
// ---------------------------------------------------------------------------

describe('request — server errors', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('handles 500 with JSON error body', async () => {
    mockFetch({ error: 'Internal server error' }, { status: 500, ok: false });
    const err = await get('/boom').catch((e: unknown) => e);
    expect((err as ApiError).status).toBe(500);
    expect((err as ApiError).message).toBe('Internal server error');
  });

  it('handles 500 with non-JSON body (falls back to statusText)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: false,
        status: 500,
        statusText: 'Internal Server Error',
        json: () => Promise.reject(new SyntaxError('Unexpected token')),
      }),
    );
    const err = await get('/html-error').catch((e: unknown) => e);
    expect((err as ApiError).status).toBe(500);
    // json() failed, so falls back to statusText via the .catch(() => ({ error: res.statusText })) path
    expect((err as ApiError).message).toBe('Internal Server Error');
  });

  it('handles 503 Service Unavailable', async () => {
    mockFetch({ error: 'Service temporarily unavailable' }, { status: 503, ok: false });
    const err = await get('/api').catch((e: unknown) => e);
    expect((err as ApiError).status).toBe(503);
    expect((err as ApiError).message).toBe('Service temporarily unavailable');
  });

  it('handles 429 Rate Limited', async () => {
    mockFetch({ error: 'Too many requests' }, { status: 429, ok: false });
    const err = await get('/api').catch((e: unknown) => e);
    expect((err as ApiError).status).toBe(429);
    expect((err as ApiError).message).toBe('Too many requests');
  });
});

// ---------------------------------------------------------------------------
// Token refresh flow
// ---------------------------------------------------------------------------

describe('request — 401 token refresh flow', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('401 with valid refresh token retries and succeeds', async () => {
    sessionStorage.setItem('refresh_token', 'valid_refresh');
    const successData = { result: 'ok' };

    let callCount = 0;
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((url: string) => {
        // /auth/refresh returns new tokens
        if ((url as string).includes('/auth/refresh')) {
          return Promise.resolve({
            ok: true,
            status: 200,
            statusText: 'OK',
            json: () => Promise.resolve({ access_token: 'new_token' }),
          });
        }
        callCount++;
        // First protected call → 401; second (retry) → 200
        if (callCount === 1) {
          return Promise.resolve({
            ok: false,
            status: 401,
            statusText: 'Unauthorized',
            json: () => Promise.resolve({ error: 'Unauthorized' }),
          });
        }
        return Promise.resolve({
          ok: true,
          status: 200,
          statusText: 'OK',
          json: () => Promise.resolve(successData),
        });
      }),
    );

    const result = await get<typeof successData>('/protected');
    expect(result).toEqual(successData);
    // New access token should be stored
    expect(sessionStorage.getItem('access_token')).toBe('new_token');
  });

  it('401 with no refresh token clears session and throws', async () => {
    // No refresh_token in sessionStorage
    mockFetch({ error: 'Unauthorized' }, { status: 401, ok: false });

    sessionStorage.setItem('access_token', 'old_token');
    const err = await get('/private').catch((e: unknown) => e);

    expect((err as ApiError).status).toBe(401);
    expect(sessionStorage.getItem('access_token')).toBeNull();
  });

  it('401 where refresh also fails clears session', async () => {
    sessionStorage.setItem('access_token', 'old_token');
    sessionStorage.setItem('refresh_token', 'bad_refresh');

    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((url: string) => {
        if ((url as string).includes('/auth/refresh')) {
          return Promise.resolve({
            ok: false,
            status: 401,
            statusText: 'Unauthorized',
            json: () => Promise.resolve({ error: 'Invalid refresh token' }),
          });
        }
        return Promise.resolve({
          ok: false,
          status: 401,
          statusText: 'Unauthorized',
          json: () => Promise.resolve({ error: 'Unauthorized' }),
        });
      }),
    );

    await expect(get('/private')).rejects.toBeInstanceOf(ApiError);
    expect(sessionStorage.getItem('access_token')).toBeNull();
  });

  it('refresh response with new refresh_token updates sessionStorage', async () => {
    sessionStorage.setItem('refresh_token', 'old_refresh');

    let protectedCallCount = 0;
    vi.stubGlobal(
      'fetch',
      vi.fn().mockImplementation((url: string) => {
        if ((url as string).includes('/auth/refresh')) {
          return Promise.resolve({
            ok: true,
            status: 200,
            statusText: 'OK',
            json: () =>
              Promise.resolve({ access_token: 'new_access', refresh_token: 'new_refresh' }),
          });
        }
        protectedCallCount++;
        if (protectedCallCount === 1) {
          return Promise.resolve({
            ok: false,
            status: 401,
            statusText: 'Unauthorized',
            json: () => Promise.resolve({ error: 'Unauthorized' }),
          });
        }
        return Promise.resolve({
          ok: true,
          status: 200,
          statusText: 'OK',
          json: () => Promise.resolve({ data: true }),
        });
      }),
    );

    await get('/protected');
    expect(sessionStorage.getItem('refresh_token')).toBe('new_refresh');
  });
});

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

describe('request — edge cases', () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('handles malformed JSON on 200 response by propagating the parse error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        ok: true,
        status: 200,
        statusText: 'OK',
        json: () => Promise.reject(new SyntaxError('Unexpected token < in JSON at position 0')),
      }),
    );
    await expect(get('/bad-json')).rejects.toThrow(SyntaxError);
  });

  it('does not send Authorization header when no token present', async () => {
    mockFetch({ ok: true });
    await get('/public');
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = init.headers as Record<string, string>;
    expect(headers['Authorization']).toBeUndefined();
  });

  it('always sends Content-Type: application/json on all requests', async () => {
    mockFetch({ ok: true });
    await post('/orders', { qty: '1' });
    const fetchMock = vi.mocked(fetch);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = init.headers as Record<string, string>;
    expect(headers['Content-Type']).toBe('application/json');
  });

  it('404 with no error field in body uses "Request failed" fallback', async () => {
    mockFetch({ message: 'not here' }, { status: 404, ok: false });
    const err = await get('/ghost').catch((e: unknown) => e);
    expect((err as ApiError).message).toBe('Request failed');
  });
});
