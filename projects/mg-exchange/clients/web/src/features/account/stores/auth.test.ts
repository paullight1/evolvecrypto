import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useAuthStore } from './auth';

// Prevent window.location.hash assignment from throwing in jsdom
Object.defineProperty(window, 'location', {
  value: { hash: '' },
  writable: true,
});

function resetStore(): void {
  useAuthStore.setState({ isLoggedIn: false, userId: null, email: null });
}

describe('useAuthStore — initial state', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('is not logged in initially', () => {
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });

  it('userId is null initially', () => {
    expect(useAuthStore.getState().userId).toBeNull();
  });

  it('email is null initially', () => {
    expect(useAuthStore.getState().email).toBeNull();
  });
});

describe('useAuthStore — login', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('sets isLoggedIn to true', () => {
    useAuthStore.getState().login('tok_access', 'tok_refresh', 'uid_1', 'user@test.com');
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('stores userId', () => {
    useAuthStore.getState().login('tok_access', 'tok_refresh', 'uid_42', 'a@b.com');
    expect(useAuthStore.getState().userId).toBe('uid_42');
  });

  it('stores email', () => {
    useAuthStore.getState().login('tok_access', 'tok_refresh', 'uid_1', 'hello@world.com');
    expect(useAuthStore.getState().email).toBe('hello@world.com');
  });

  it('writes access_token to sessionStorage', () => {
    useAuthStore.getState().login('my_access', 'my_refresh', 'u1', 'x@y.com');
    expect(sessionStorage.getItem('access_token')).toBe('my_access');
  });

  it('writes refresh_token to sessionStorage', () => {
    useAuthStore.getState().login('my_access', 'my_refresh', 'u1', 'x@y.com');
    expect(sessionStorage.getItem('refresh_token')).toBe('my_refresh');
  });
});

describe('useAuthStore — logout', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('clears isLoggedIn', () => {
    useAuthStore.getState().login('tok', 'ref', 'uid', 'a@b.com');
    useAuthStore.getState().logout();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });

  it('clears userId', () => {
    useAuthStore.getState().login('tok', 'ref', 'uid_99', 'a@b.com');
    useAuthStore.getState().logout();
    expect(useAuthStore.getState().userId).toBeNull();
  });

  it('clears email', () => {
    useAuthStore.getState().login('tok', 'ref', 'uid', 'email@test.com');
    useAuthStore.getState().logout();
    expect(useAuthStore.getState().email).toBeNull();
  });

  it('removes access_token from sessionStorage', () => {
    sessionStorage.setItem('access_token', 'stored_tok');
    useAuthStore.getState().logout();
    expect(sessionStorage.getItem('access_token')).toBeNull();
  });

  it('removes refresh_token from sessionStorage', () => {
    sessionStorage.setItem('refresh_token', 'stored_ref');
    useAuthStore.getState().logout();
    expect(sessionStorage.getItem('refresh_token')).toBeNull();
  });
});

describe('useAuthStore — hydrate', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('sets isLoggedIn to true when access_token exists in sessionStorage', () => {
    sessionStorage.setItem('access_token', 'existing_tok');
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('stays logged out when no access_token in sessionStorage', () => {
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });

  it('does not modify userId on hydrate (no JWT decode)', () => {
    sessionStorage.setItem('access_token', 'tok');
    useAuthStore.getState().hydrate();
    // userId is not set by hydrate — the store comment documents this limitation
    expect(useAuthStore.getState().userId).toBeNull();
  });

  // === Token Expiry ===

  it('hydrate treats any non-null token as valid (no expiry check)', () => {
    // The store does not decode JWTs or validate expiry — it only checks presence.
    // A token string that looks expired is still treated as logged in.
    // This is documented in the store comment: "A real app would decode the JWT or fetch /me"
    const expiredLookingToken = 'eyJhbGciOiJIUzI1NiJ9.eyJleHAiOjF9.signature';
    sessionStorage.setItem('access_token', expiredLookingToken);
    useAuthStore.getState().hydrate();
    // Current implementation: any truthy token → isLoggedIn = true
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('hydrate after manual token removal leaves user logged out', () => {
    sessionStorage.setItem('access_token', 'some_token');
    // Manually remove the token (simulating expiry clearing by another tab)
    sessionStorage.removeItem('access_token');
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });
});

// === Multiple Login/Logout Cycles ===

describe('useAuthStore — multiple login/logout cycles', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
  });

  it('login after logout works correctly with no state leaks', () => {
    // First cycle
    useAuthStore.getState().login('tok1', 'ref1', 'uid_first', 'first@example.com');
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
    expect(useAuthStore.getState().userId).toBe('uid_first');

    useAuthStore.getState().logout();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
    expect(useAuthStore.getState().userId).toBeNull();
    expect(useAuthStore.getState().email).toBeNull();

    // Second cycle — different credentials
    useAuthStore.getState().login('tok2', 'ref2', 'uid_second', 'second@example.com');
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
    expect(useAuthStore.getState().userId).toBe('uid_second');
    expect(useAuthStore.getState().email).toBe('second@example.com');

    // Verify sessionStorage has new tokens, not old ones
    expect(sessionStorage.getItem('access_token')).toBe('tok2');
    expect(sessionStorage.getItem('refresh_token')).toBe('ref2');
  });

  it('logout clears sessionStorage so hydrate after logout leaves user logged out', () => {
    useAuthStore.getState().login('tok', 'ref', 'uid', 'a@b.com');
    useAuthStore.getState().logout();
    // Reset in-memory state to simulate page refresh
    resetStore();
    useAuthStore.getState().hydrate();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });

  it('three consecutive logins only retain the last session', () => {
    useAuthStore.getState().login('tok1', 'ref1', 'uid1', 'a@example.com');
    useAuthStore.getState().login('tok2', 'ref2', 'uid2', 'b@example.com');
    useAuthStore.getState().login('tok3', 'ref3', 'uid3', 'c@example.com');

    expect(useAuthStore.getState().userId).toBe('uid3');
    expect(useAuthStore.getState().email).toBe('c@example.com');
    expect(sessionStorage.getItem('access_token')).toBe('tok3');
  });
});
