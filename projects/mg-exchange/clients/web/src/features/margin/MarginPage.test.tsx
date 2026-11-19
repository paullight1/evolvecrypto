import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  toggleSpotMargin: vi.fn(),
  getMarginStatus: vi.fn(),
  borrowMargin: vi.fn(),
  repayMargin: vi.fn(),
  getActiveBorrows: vi.fn(),
  getInterestHistory: vi.fn(),
  getBorrowableAssets: vi.fn(),
}));

import MarginPage from './MarginPage';
import {
  getMarginStatus,
  borrowMargin,
  getActiveBorrows,
  getBorrowableAssets,
} from './api';
import type { MarginStatus, MarginBorrow, BorrowableAsset } from './types';

const mockGetMarginStatus = vi.mocked(getMarginStatus);
const mockBorrowMargin = vi.mocked(borrowMargin);
const mockGetActiveBorrows = vi.mocked(getActiveBorrows);
const mockGetBorrowableAssets = vi.mocked(getBorrowableAssets);

const MOCK_STATUS: MarginStatus = {
  enabled: true,
  margin_level: '2.5',
  available_margin: '5000',
  used_margin: '2000',
  total_assets: '10000',
  total_liabilities: '2000',
};

const MOCK_BORROWABLE: BorrowableAsset[] = [
  { asset: 'USDT', max_borrow: '10000', hourly_rate: '0.001', daily_rate: '0.024' },
  { asset: 'BTC', max_borrow: '1', hourly_rate: '0.0005', daily_rate: '0.012' },
];

const MOCK_BORROWS: MarginBorrow[] = [
  { asset: 'USDT', amount: '1000', interest_rate: '0.001', interest_accrued: '0.5', created_at: 1700000000 },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetMarginStatus.mockResolvedValue(MOCK_STATUS);
  mockGetBorrowableAssets.mockResolvedValue(MOCK_BORROWABLE);
  mockGetActiveBorrows.mockResolvedValue(MOCK_BORROWS);
  mockBorrowMargin.mockResolvedValue(undefined);
});

describe('MarginPage', () => {
  it('renders title, margin status card, and tabs', async () => {
    await act(async () => { render(<MarginPage />); });

    expect(screen.getByText('Spot Margin')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Borrow' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Repay' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Active Borrows' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Interest History' })).toBeInTheDocument();
  });

  it('shows margin status card with data from API', async () => {
    await act(async () => { render(<MarginPage />); });

    // MarginStatusCard renders margin_level
    expect(await screen.findByText('2.5x')).toBeInTheDocument();
    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('borrow form shows asset options and rate info', async () => {
    await act(async () => { render(<MarginPage />); });

    // BorrowForm fetches borrowable assets; once loaded it renders rate info
    expect(await screen.findByText('Hourly rate:')).toBeInTheDocument();
    expect(screen.getByLabelText('Amount')).toBeInTheDocument();
    expect(screen.getByLabelText('Asset')).toBeInTheDocument();
  });

  it('borrow form validates empty amount', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<MarginPage />); });

    await screen.findByText('Hourly rate:'); // assets loaded

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Borrow' }));
    });

    expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
    expect(mockBorrowMargin).not.toHaveBeenCalled();
  });

  it('borrow submit calls borrowMargin with correct params', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<MarginPage />); });

    await screen.findByText('Hourly rate:');

    const amountInput = screen.getByLabelText('Amount');
    await user.clear(amountInput);
    await user.type(amountInput, '500');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Borrow' }));
    });

    expect(mockBorrowMargin).toHaveBeenCalledWith({ asset: 'USDT', amount: '500' });
  });

  it('active borrows tab renders borrow records', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<MarginPage />); });

    await user.click(screen.getByRole('tab', { name: 'Active Borrows' }));

    expect(await screen.findByText('USDT')).toBeInTheDocument();
    expect(screen.getByText('1000')).toBeInTheDocument();
    expect(mockGetActiveBorrows).toHaveBeenCalledTimes(1);
  });

  it('shows empty state in active borrows when no borrows exist', async () => {
    mockGetActiveBorrows.mockResolvedValueOnce([]);
    const user = userEvent.setup();
    await act(async () => { render(<MarginPage />); });

    await user.click(screen.getByRole('tab', { name: 'Active Borrows' }));

    expect(await screen.findByText('No active borrows')).toBeInTheDocument();
  });
});
