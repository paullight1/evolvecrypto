import { describe, test, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import DashboardPage from './DashboardPage';

vi.mock('./api', () => ({
  getWalletOverview: vi.fn().mockResolvedValue(null),
  getRecentOrders: vi.fn().mockResolvedValue([]),
  getOpenPositions: vi.fn().mockResolvedValue([]),
}));

vi.mock('./components/PortfolioSummary', () => ({
  PortfolioSummary: () => <div data-testid="portfolio-summary" />,
}));
vi.mock('./components/QuickActions', () => ({
  QuickActions: () => <div data-testid="quick-actions" />,
}));
vi.mock('./components/RecentOrdersList', () => ({
  RecentOrdersList: () => <div data-testid="recent-orders" />,
}));
vi.mock('./components/OpenPositionsSummary', () => ({
  OpenPositionsSummary: () => <div data-testid="open-positions" />,
}));

describe('DashboardPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  test('renders page title', () => {
    render(<DashboardPage />);
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
  });

  test('renders portfolio summary', () => {
    render(<DashboardPage />);
    expect(screen.getByTestId('portfolio-summary')).toBeInTheDocument();
  });

  test('renders quick actions', () => {
    render(<DashboardPage />);
    expect(screen.getByTestId('quick-actions')).toBeInTheDocument();
  });

  test('calls all data APIs on mount', async () => {
    const { getWalletOverview, getRecentOrders, getOpenPositions } = await import('./api');
    render(<DashboardPage />);
    expect(getWalletOverview).toHaveBeenCalledTimes(1);
    expect(getRecentOrders).toHaveBeenCalledTimes(1);
    expect(getOpenPositions).toHaveBeenCalledTimes(1);
  });

  test('renders recent orders and positions sections', () => {
    render(<DashboardPage />);
    expect(screen.getByTestId('recent-orders')).toBeInTheDocument();
    expect(screen.getByTestId('open-positions')).toBeInTheDocument();
  });
});
