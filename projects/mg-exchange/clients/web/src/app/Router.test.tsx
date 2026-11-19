import { render, screen, act } from '@testing-library/react';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

// Mock layout components to transparent wrappers so tests focus on routing
vi.mock('./layouts/FullPageLayout', () => ({
  FullPageLayout: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

vi.mock('./layouts/TerminalLayout', () => ({
  TerminalLayout: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="terminal-layout">{children}</div>
  ),
}));

vi.mock('./layouts/DashboardLayout', () => ({
  DashboardLayout: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="dashboard-layout">{children}</div>
  ),
}));

// Mock all lazy-loaded feature modules to simple components
vi.mock('@features/info/LandingPage', () => ({
  default: () => <div>Landing Page</div>,
}));

vi.mock('@features/account/AuthPage', () => ({
  default: () => <div>Auth Page</div>,
}));

vi.mock('@features/trading/TradingTerminal', () => ({
  default: () => <div>Trading Terminal</div>,
}));

vi.mock('@features/wallet/WalletPage', () => ({
  default: () => <div>Wallet Page</div>,
}));

vi.mock('@app/placeholders/Placeholder', () => ({
  default: () => <div>Placeholder</div>,
}));

vi.mock('@features/options/OptionsPage', () => ({
  default: () => <div>Options Page</div>,
}));

vi.mock('@features/delivery/DeliveryPage', () => ({
  default: () => <div>Delivery Page</div>,
}));

vi.mock('@features/margin/MarginPage', () => ({
  default: () => <div>Margin Page</div>,
}));

vi.mock('@features/portfolio-margin/PortfolioMarginPage', () => ({
  default: () => <div>Portfolio Margin Page</div>,
}));

vi.mock('@features/markets/MarketsPage', () => ({
  default: () => <div>Markets Page</div>,
}));

import { Router, navigate } from './Router';

// Helper: set pathname for History API router
function setPath(path: string) {
  window.history.replaceState(null, '', path ? `/${path}` : '/');
}

describe('Router', () => {
  beforeEach(() => {
    setPath('');
  });

  afterEach(() => {
    setPath('');
  });

  it('renders auth page at /auth', async () => {
    setPath('auth');
    render(<Router />);
    await screen.findByText('Auth Page');
  });

  it('renders landing page at /', async () => {
    setPath('');
    render(<Router />);
    await screen.findByText('Landing Page');
  });

  it('renders trading terminal at /trade', async () => {
    setPath('trade');
    render(<Router />);
    await screen.findByText('Trading Terminal');
  });

  it('navigate() changes pathname to /auth', () => {
    navigate('auth');
    expect(window.location.pathname).toBe('/auth');
  });

  it('navigate() with empty string goes to /', () => {
    navigate('auth');
    navigate('');
    expect(window.location.pathname).toBe('/');
  });

  it('renders options page at /options', async () => {
    setPath('options');
    render(<Router />);
    await screen.findByText('Options Page');
  });

  it('renders delivery page at /delivery', async () => {
    setPath('delivery');
    render(<Router />);
    await screen.findByText('Delivery Page');
  });

  it('renders margin page at /margin', async () => {
    setPath('margin');
    render(<Router />);
    await screen.findByText('Margin Page');
  });

  it('renders portfolio margin page at /portfolio-margin', async () => {
    setPath('portfolio-margin');
    render(<Router />);
    await screen.findByText('Portfolio Margin Page');
  });

  it('reacts to popstate event and switches view', async () => {
    setPath('');
    render(<Router />);
    await screen.findByText('Landing Page');

    act(() => {
      window.history.pushState(null, '', '/auth');
      window.dispatchEvent(new PopStateEvent('popstate'));
    });

    await screen.findByText('Auth Page');
  });

  it('wraps landing in fullpage layout (no wrapper element)', async () => {
    setPath('');
    render(<Router />);
    await screen.findByText('Landing Page');
    expect(screen.queryByTestId('terminal-layout')).not.toBeInTheDocument();
    expect(screen.queryByTestId('dashboard-layout')).not.toBeInTheDocument();
  });

  it('wraps trade in terminal layout', async () => {
    setPath('trade');
    render(<Router />);
    await screen.findByText('Trading Terminal');
    expect(screen.getByTestId('terminal-layout')).toBeInTheDocument();
  });

  it('wraps markets in dashboard layout', async () => {
    setPath('markets');
    render(<Router />);
    await screen.findByText('Markets Page');
    expect(screen.getByTestId('dashboard-layout')).toBeInTheDocument();
  });
});
