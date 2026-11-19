import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// ---------------------------------------------------------------------------
// Module mocks — must come before imports that reference them
// ---------------------------------------------------------------------------

vi.mock('./Router', () => ({
  Router: () => <div data-testid="router-outlet">Router</div>,
  navigate: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  ToastContainer: () => <div data-testid="toast-container" />,
}));

vi.mock('@shared/ui/ErrorBoundary', () => ({
  ErrorBoundary: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

// ---------------------------------------------------------------------------
// Imports (after mocks)
// ---------------------------------------------------------------------------

import { App } from './App';
import { useAuthStore } from '@features/account/stores/auth';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function resetStore(): void {
  useAuthStore.setState({ isLoggedIn: false, userId: null, email: null });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('App', () => {
  beforeEach(() => {
    sessionStorage.clear();
    resetStore();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the router outlet', () => {
    render(<App />);
    expect(screen.getByTestId('router-outlet')).toBeInTheDocument();
  });

  it('renders the toast container', () => {
    render(<App />);
    expect(screen.getByTestId('toast-container')).toBeInTheDocument();
  });

  it('hydrates auth from sessionStorage on mount', () => {
    sessionStorage.setItem('access_token', 'stored_tok');
    resetStore();

    render(<App />);

    // After hydrate(), isLoggedIn should be true
    expect(useAuthStore.getState().isLoggedIn).toBe(true);
  });

  it('does not crash when there is no stored token', () => {
    sessionStorage.clear();
    resetStore();

    expect(() => render(<App />)).not.toThrow();
    expect(useAuthStore.getState().isLoggedIn).toBe(false);
  });
});
