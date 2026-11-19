import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  login,
  register,
  refreshToken,
  setup2FA,
  enable2FA,
  disable2FA,
  getApiKeys,
  createApiKey,
  revokeApiKey,
  getSessions,
  revokeSession,
  getSettings,
  updateSettings,
  changePassword,
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

describe('login', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/auth/login', async () => {
    mockOkFetch({ access_token: 'tok', refresh_token: 'ref' });
    await login({ email: 'a@b.com', password: 'pass' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/auth/login');
    expect(init.method).toBe('POST');
  });

  it('sends credentials as JSON body', async () => {
    mockOkFetch({ access_token: 'tok', refresh_token: 'ref' });
    const req = { email: 'user@x.com', password: 'secret' };
    await login(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns the login response from the server', async () => {
    const resp = { access_token: 'abc', refresh_token: 'xyz' };
    mockOkFetch(resp);
    const result = await login({ email: 'a@b.com', password: 'p' });
    expect(result).toEqual(resp);
  });
});

describe('register', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/auth/register', async () => {
    mockOkFetch({ user_id: 'u1' });
    await register({ email: 'new@x.com', password: 'pass', username: 'alice' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/auth/register');
    expect(init.method).toBe('POST');
  });

  it('sends registration data as JSON', async () => {
    mockOkFetch({ user_id: 'u2' });
    const req = { email: 'b@x.com', password: '123', username: 'bob' };
    await register(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('refreshToken', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/auth/refresh', async () => {
    mockOkFetch({ access_token: 'new-tok' });
    await refreshToken('old-refresh');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/auth/refresh');
    expect(init.method).toBe('POST');
  });

  it('sends refresh_token in body', async () => {
    mockOkFetch({ access_token: 'new-tok' });
    await refreshToken('my-refresh-token');
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ refresh_token: 'my-refresh-token' });
  });
});

describe('setup2FA', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/2fa/setup', async () => {
    mockOkFetch({ qr_url: 'otpauth://...', secret: 'ABC' });
    await setup2FA();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/2fa/setup');
    expect(init.method).toBe('POST');
  });
});

describe('enable2FA', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/2fa/enable with code', async () => {
    mockOkFetch(undefined, 204);
    await enable2FA('123456');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/2fa/enable');
    expect(init.method).toBe('POST');
    expect(JSON.parse(init.body as string)).toEqual({ code: '123456' });
  });
});

describe('disable2FA', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/2fa/disable with code', async () => {
    mockOkFetch(undefined, 204);
    await disable2FA('654321');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/2fa/disable');
    expect(JSON.parse(init.body as string)).toEqual({ code: '654321' });
  });
});

describe('getApiKeys', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/api-keys', async () => {
    mockOkFetch([]);
    await getApiKeys();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/api-keys');
    expect(init.method).toBe('GET');
  });
});

describe('createApiKey', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/api-keys', async () => {
    mockOkFetch({ key: 'k1', secret: 's1', label: 'test' });
    await createApiKey({ label: 'test', permissions: ['read'] });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/api-keys');
    expect(init.method).toBe('POST');
  });

  it('sends payload as JSON body', async () => {
    mockOkFetch({ key: 'k2', secret: 's2', label: 'trade' });
    const req = { label: 'trade', permissions: ['read', 'trade'] };
    await createApiKey(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});

describe('revokeApiKey', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends DELETE to /api/v1/account/api-keys/{key}', async () => {
    mockOkFetch(undefined, 204);
    await revokeApiKey('my-api-key');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/api-keys/my-api-key');
    expect(init.method).toBe('DELETE');
  });
});

describe('getSessions', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/sessions', async () => {
    mockOkFetch([]);
    await getSessions();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/sessions');
    expect(init.method).toBe('GET');
  });
});

describe('revokeSession', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends DELETE to /api/v1/account/sessions/{id}', async () => {
    mockOkFetch(undefined, 204);
    await revokeSession('sess-42');
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/sessions/sess-42');
    expect(init.method).toBe('DELETE');
  });
});

describe('getSettings', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/account/settings', async () => {
    mockOkFetch({ language: 'en', notifications_enabled: true });
    await getSettings();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/settings');
    expect(init.method).toBe('GET');
  });
});

describe('updateSettings', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/settings', async () => {
    mockOkFetch({ language: 'zh', notifications_enabled: false });
    await updateSettings({ language: 'zh' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/settings');
    expect(init.method).toBe('POST');
  });

  it('sends partial settings as JSON', async () => {
    mockOkFetch({ language: 'en', notifications_enabled: false });
    await updateSettings({ notifications_enabled: false });
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual({ notifications_enabled: false });
  });
});

describe('changePassword', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/account/password', async () => {
    mockOkFetch(undefined, 204);
    await changePassword({ current_password: 'old', new_password: 'new' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/account/password');
    expect(init.method).toBe('POST');
  });

  it('sends password payload as JSON', async () => {
    mockOkFetch(undefined, 204);
    const req = { current_password: 'oldpass', new_password: 'newpass' };
    await changePassword(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });
});
