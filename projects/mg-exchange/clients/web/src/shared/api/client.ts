const BASE = '/api/v1';

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

async function tryRefreshToken(): Promise<boolean> {
  const refreshToken = sessionStorage.getItem('refresh_token');
  if (!refreshToken) return false;

  try {
    const res = await fetch(`${BASE}/auth/refresh`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });
    if (!res.ok) return false;
    const data = (await res.json()) as { access_token: string; refresh_token?: string };
    sessionStorage.setItem('access_token', data.access_token);
    if (data.refresh_token) {
      sessionStorage.setItem('refresh_token', data.refresh_token);
    }
    return true;
  } catch {
    return false;
  }
}

function clearSession(): void {
  sessionStorage.removeItem('access_token');
  sessionStorage.removeItem('refresh_token');
  window.location.href = '/auth';
}

export async function request<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  const token = sessionStorage.getItem('access_token');
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });

  if (res.status === 401) {
    const refreshed = await tryRefreshToken();
    if (refreshed) {
      // Retry once with the new token
      return request<T>(method, path, body);
    }
    clearSession();
    throw new ApiError(401, 'Session expired');
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: res.statusText })) as { error?: string };
    throw new ApiError(res.status, err.error ?? 'Request failed');
  }

  if (res.status === 204) return undefined as T;
  return res.json() as Promise<T>;
}

// Convenience wrappers
export const get = <T>(path: string): Promise<T> => request<T>('GET', path);
export const post = <T>(path: string, body?: unknown): Promise<T> => request<T>('POST', path, body);
export const put = <T>(path: string, body?: unknown): Promise<T> => request<T>('PUT', path, body);
export const patch = <T>(path: string, body?: unknown): Promise<T> => request<T>('PATCH', path, body);
export const del = <T>(path: string): Promise<T> => request<T>('DELETE', path);
