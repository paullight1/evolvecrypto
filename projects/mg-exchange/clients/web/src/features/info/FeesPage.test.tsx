import { describe, test, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import FeesPage from './FeesPage';

const MOCK_SCHEDULE = {
  vip_tiers: [
    { level: 0, name: 'Regular', min_volume_30d: '0', min_balance: '0', maker_fee: '0.001', taker_fee: '0.002' },
    { level: 1, name: 'VIP1', min_volume_30d: '1000000', min_balance: '0', maker_fee: '0.0008', taker_fee: '0.0016' },
    { level: 2, name: 'VIP2', min_volume_30d: '5000000', min_balance: '0', maker_fee: '0.0006', taker_fee: '0.0012' },
  ],
  withdrawal_fees: { BTC: '0.0005', ETH: '0.005', USDT: '1.0' },
};

const MOCK_USER_INFO = {
  tier_level: 1,
  tier_name: 'VIP1',
  maker_fee: '0.0008',
  taker_fee: '0.0016',
  volume_30d: '2500000',
};

vi.mock('./api', () => ({
  getFeeSchedule: vi.fn(() => Promise.resolve(MOCK_SCHEDULE)),
  getUserFeeInfo: vi.fn(() => Promise.resolve(MOCK_USER_INFO)),
}));

describe('FeesPage', () => {
  beforeEach(() => { vi.clearAllMocks(); });

  test('renders page title', async () => {
    render(<FeesPage />);
    expect(screen.getByText('Fee Schedule')).toBeInTheDocument();
  });

  test('renders VIP tiers from API', async () => {
    render(<FeesPage />);
    await waitFor(() => {
      expect(screen.getByText('Regular')).toBeInTheDocument();
      expect(screen.getAllByText('VIP1').length).toBeGreaterThan(0);
      expect(screen.getByText('VIP2')).toBeInTheDocument();
    });
  });

  test('shows user fee info with tier name', async () => {
    render(<FeesPage />);
    await waitFor(() => {
      // VIP1 appears in both the user info box and the tier table
      expect(screen.getAllByText(/VIP1/).length).toBeGreaterThanOrEqual(2);
    });
  });

  test('shows explanation subtitle', async () => {
    render(<FeesPage />);
    await waitFor(() => {
      expect(screen.getByText(/30-day rolling trading volume/i)).toBeInTheDocument();
    });
  });

  test('renders fee table with maker/taker columns', async () => {
    render(<FeesPage />);
    await waitFor(() => {
      expect(screen.getByText('Maker Fee')).toBeInTheDocument();
      expect(screen.getByText('Taker Fee')).toBeInTheDocument();
    });
  });
});
