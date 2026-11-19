import { describe, test, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import LandingPage from './LandingPage';

const mockNavigate = vi.fn();

vi.mock('@features/trading/api', () => ({
  getTicker: vi.fn().mockResolvedValue({
    symbol: 'BTCUSDT',
    last_price: '50000',
    price_change_pct_24h: '2.5',
    high_24h: '51000',
    low_24h: '49000',
    volume_24h: '12000',
    quote_volume_24h: '600000000',
  }),
}));

vi.mock('@app/Router', () => ({
  navigate: (...args: unknown[]) => mockNavigate(...args),
}));

describe('LandingPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockNavigate.mockClear();
  });

  test('renders hero headline', () => {
    render(<LandingPage />);
    expect(screen.getByText(/Trade Crypto with/i)).toBeInTheDocument();
  });

  test('renders hero subtext', () => {
    render(<LandingPage />);
    expect(screen.getByText(/transparent exchange/i)).toBeInTheDocument();
  });

  test('renders Get Started call-to-action button', () => {
    render(<LandingPage />);
    expect(screen.getByRole('button', { name: 'Get Started' })).toBeInTheDocument();
  });

  test('renders Try Demo call-to-action button', () => {
    render(<LandingPage />);
    expect(screen.getByRole('button', { name: 'Try Demo' })).toBeInTheDocument();
  });

  test('renders trust badges', () => {
    render(<LandingPage />);
    expect(screen.getAllByText(/VASP Registered/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/100% Reserves/i).length).toBeGreaterThan(0);
  });

  test('renders feature cards', () => {
    render(<LandingPage />);
    expect(screen.getAllByText('Sub-Millisecond Matching').length).toBeGreaterThan(0);
    // "Proof of Reserves" appears in features section (and possibly footer)
    expect(screen.getAllByText('Proof of Reserves').length).toBeGreaterThan(0);
    expect(screen.getByText('Open-Source Engine')).toBeInTheDocument();
    expect(screen.getByText('Built in Ukraine')).toBeInTheDocument();
  });

  test('renders products section heading', () => {
    render(<LandingPage />);
    expect(screen.getByText('Our Products')).toBeInTheDocument();
  });

  test('renders product cards', () => {
    render(<LandingPage />);
    // These labels appear in both the products grid and footer links
    expect(screen.getAllByText('Spot Trading').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Perpetual Futures').length).toBeGreaterThan(0);
    expect(screen.getAllByText('Options').length).toBeGreaterThan(0);
  });

  test('renders stats section', () => {
    render(<LandingPage />);
    expect(screen.getByText('24h Volume')).toBeInTheDocument();
    expect(screen.getByText('Active Traders')).toBeInTheDocument();
    expect(screen.getByText('Trading Pairs')).toBeInTheDocument();
    expect(screen.getByText('Avg Latency')).toBeInTheDocument();
  });

  test('renders footer copyright', () => {
    render(<LandingPage />);
    expect(screen.getByText(/© 2026 Exchange/i)).toBeInTheDocument();
  });

  test('Get Started button navigates to auth page', async () => {
    render(<LandingPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Get Started' }));
    expect(mockNavigate).toHaveBeenCalledWith('auth');
  });

  test('Try Demo button navigates to trade page', async () => {
    render(<LandingPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Try Demo' }));
    expect(mockNavigate).toHaveBeenCalledWith('trade');
  });

  test('header Log In button navigates to auth', async () => {
    render(<LandingPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Log In' }));
    expect(mockNavigate).toHaveBeenCalledWith('auth');
  });

  test('header Register button navigates to auth', async () => {
    render(<LandingPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Register' }));
    expect(mockNavigate).toHaveBeenCalledWith('auth');
  });

  test('ticker section appears after data loads', async () => {
    render(<LandingPage />);
    await waitFor(() => {
      // BTC symbol is stripped to just BTC in the ticker
      expect(screen.getAllByText('BTC').length).toBeGreaterThan(0);
    });
  });

  test('positive price change shown with leading + in ticker', async () => {
    render(<LandingPage />);
    await waitFor(() => {
      expect(screen.getAllByText('+2.50%').length).toBeGreaterThan(0);
    });
  });

  test('renders header navigation links', () => {
    render(<LandingPage />);
    // Header nav is in the landing page's own header
    expect(screen.getByRole('navigation', { name: 'Landing navigation' })).toBeInTheDocument();
  });
});
