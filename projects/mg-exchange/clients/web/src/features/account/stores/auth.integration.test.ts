/**
 * auth.integration.test.ts
 *
 * Integration-level tests that exercise the auth store together with
 * sessionStorage, verifying the full read/write contract.
 */
import { describe, it, expect, beforeEach } from 'vitest';
import { useAuthStore } from './auth';

// jsdom throws on hash assignment unless we stub location
Object.defineProperty(window, 'location', {
  value: { hash: '' },
  writable: true,
});

function resetStore(): void {
  useAuthStore.setState({ isLoggedIn: false, userId: null, email: null });
}

// ---------------------------------------------------------------------------
// Token storage contract
// ---------------------------------------------------------------------------

describe('auth integration — login stores tokens in sessionStorage', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('access_token written to sessionStorage on login', () => {
    useAuthStore.getState().login('access_123', 'refresh_456', 'uid-1', 'user@test.com');
    expect(sessionStorage.getItem('access_token')).toBe('access_123');
  });

  it('refresh_token written to sessionStorage on login', () => {
    useAuthStore.getState().login('access_123', 'refresh_456', 'uid-1', 'user@test.com');
    expect(sessionStorage.getItem('refresh_token')).toBe('refresh_456');
  });

  it('both tokens present simultaneously after login', () => {
    useAuthStore.getState().login('a', 'r', 'u', 'e@x.com');
    expect(sessionStorage.getItem('access_token')).not.toBeNull();
    expect(sessionStorage.getItem('refresh_token')).not.toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Logout removes tokens
// ---------------------------------------------------------------------------

describe('auth integration — logout removes all tokens from sessionStorage', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('access_token removed after logout', () => {
    useAuthStore.getState().login('a', 'r', 'u', 'e@x.com');
    useAuthStore.getState().logout();
    expect(sessionStorage.getItem('access_token')).toBeNull();
  });

  it('refresh_token removed after logout', () => {
    useAuthStore.getState().login('a', 'r', 'u', 'e@x.com');
    useAuthStore.getState().logout();
    expect(sessionStorage.getItem('refresh_token')).toBeNull();
  });

  it('sessionStorage is empty after logout (no leftover keys)', () => {
    useAuthStore.getState().login('a', 'r', 'u', 'e@x.com');
    useAuthStore.getState().logout();
    expect(sessionStorage.length).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// Hydrate restores session
// ---------------------------------------------------------------------------

describe('auth integration — hydrate', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('hydrate sets isLoggedIn to true when access_token in sessionStorage', () => {
    sessionStorage.setItem('access_token', 'tok_abc');
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('hydrate leaves isLoggedIn false when sessionStorage is empty', () => {
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });

  it('hydrate does not affect userId (no JWT decode in current implementation)', () => {
    sessionStorage.setItem('access_token', 'tok');
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().userId).toBeNull();
  });

  it('calling hydrate twice does not double-set state', () => {
    sessionStorage.setItem('access_token', 'tok');
    useAuthStore.getState().hydrate();
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Round-trip: login → logout → hydrate
// ---------------------------------------------------------------------------

describe('auth integration — login → logout → hydrate round-trip', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('hydrate after logout leaves user logged out', () => {
    useAuthStore.getState().login('tok', 'ref', 'uid', 'a@b.com');
    useAuthStore.getState().logout();
    // Simulate page refresh: reset in-memory state, then hydrate
    resetStore();
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });

  it('hydrate after login (without logout) restores session', () => {
    useAuthStore.getState().login('tok', 'ref', 'uid', 'a@b.com');
    // Simulate page refresh: reset in-memory state only, tokens remain in storage
    resetStore();
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('second login after logout stores new credentials, not old ones', () => {
    useAuthStore.getState().login('old_access', 'old_refresh', 'uid1', 'old@x.com');
    useAuthStore.getState().logout();
    useAuthStore.getState().login('new_access', 'new_refresh', 'uid2', 'new@x.com');

    expect(sessionStorage.getItem('access_token')).toBe('new_access');
    expect(sessionStorage.getItem('refresh_token')).toBe('new_refresh');
    expect(useAuthStore.getState().email).toBe('new@x.com');
  });
});
