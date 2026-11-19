import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getEarnProducts: vi.fn(),
  getEarnAllocations: vi.fn(),
  earnSubscribe: vi.fn(),
  earnRedeem: vi.fn(),
  stakingStake: vi.fn(),
  stakingUnstake: vi.fn(),
  stakingClaimRewards: vi.fn(),
  getStakingBalance: vi.fn(),
}));

import EarnPage from './EarnPage';
import {
  getEarnProducts,
  getEarnAllocations,
  earnSubscribe,
  getStakingBalance,
} from './api';
import type { EarnProduct, EarnAllocation, StakingBalance } from './types';

const mockGetProducts = vi.mocked(getEarnProducts);
const mockGetAllocations = vi.mocked(getEarnAllocations);
const mockSubscribe = vi.mocked(earnSubscribe);
const mockGetStakingBalance = vi.mocked(getStakingBalance);

const MOCK_PRODUCTS: EarnProduct[] = [
  {
    id: 'prod-1',
    coin: 'USDT',
    name: 'USDT Flexible',
    apy: '8.5%',
    productType: 'flexible',
    minAmount: '10',
    maxAmount: '1000000',
    lockDays: 0,
    supportsAutoCompound: true,
  },
  {
    id: 'prod-2',
    coin: 'BTC',
    name: 'BTC Fixed 30d',
    apy: '5.0%',
    productType: 'fixed_30d',
    minAmount: '0.001',
    maxAmount: '100',
    lockDays: 30,
    supportsAutoCompound: false,
  },
];

const MOCK_ALLOCATIONS: EarnAllocation[] = [
  {
    allocationId: 'alloc-1',
    productId: 'prod-1',
    coin: 'USDT',
    amount: '1000',
    earnedInterest: '2.34',
    status: 'active',
    startedAt: '2023-11-14T22:13:20.000Z',
    autoCompound: false,
  },
];

const MOCK_STAKING_BALANCE: StakingBalance = {
  asset: 'ETH',
  staked: '5.0',
  rewards_earned: '0.12',
  apy: '4.2%',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetProducts.mockResolvedValue(MOCK_PRODUCTS);
  mockGetAllocations.mockResolvedValue(MOCK_ALLOCATIONS);
  mockGetStakingBalance.mockResolvedValue(MOCK_STAKING_BALANCE);
});

describe('EarnPage', () => {
  it('renders title and tabs', async () => {
    await act(async () => { render(<EarnPage />); });
    expect(screen.getByText('Earn')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Savings' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Staking' })).toBeInTheDocument();
  });

  it('loads and displays earn products', async () => {
    await act(async () => { render(<EarnPage />); });
    const usdtElements = await screen.findAllByText('USDT');
    expect(usdtElements.length).toBeGreaterThan(0);
    expect(screen.getByText('8.5%')).toBeInTheDocument();
  });

  it('displays fixed product with lock period', async () => {
    await act(async () => { render(<EarnPage />); });
    await screen.findByText('BTC');
    expect(screen.getByText('30 days')).toBeInTheDocument();
  });

  it('displays allocations table', async () => {
    await act(async () => { render(<EarnPage />); });
    await screen.findByText('My Allocations');
    expect(screen.getByText('1000')).toBeInTheDocument();
  });

  it('opens subscribe modal when Subscribe is clicked', async () => {
    await act(async () => { render(<EarnPage />); });
    const subscribeBtns = await screen.findAllByRole('button', { name: 'Subscribe' });
    await userEvent.click(subscribeBtns[0]);
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Subscribe to Earn')).toBeInTheDocument();
  });

  it('validates amount in subscribe modal', async () => {
    await act(async () => { render(<EarnPage />); });
    const subscribeBtns = await screen.findAllByRole('button', { name: 'Subscribe' });
    await userEvent.click(subscribeBtns[0]);
    await userEvent.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));
    expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
  });

  it('calls earnSubscribe with correct args', async () => {
    mockSubscribe.mockResolvedValueOnce({ allocation_id: 'new-alloc' });
    await act(async () => { render(<EarnPage />); });
    const subscribeBtns = await screen.findAllByRole('button', { name: 'Subscribe' });
    await userEvent.click(subscribeBtns[0]);

    const input = screen.getByLabelText(/Amount/);
    await userEvent.type(input, '100');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));
    });

    expect(mockSubscribe).toHaveBeenCalledWith({
      productId: 'prod-1',
      amount: '100',
    });
  });

  it('shows staking panel on Staking tab', async () => {
    await act(async () => { render(<EarnPage />); });
    await userEvent.click(screen.getByRole('tab', { name: 'Staking' }));
    expect(screen.getByLabelText('Asset')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Stake/ })).toBeInTheDocument();
  });

  it('handles products load failure gracefully', async () => {
    mockGetProducts.mockRejectedValueOnce(new Error('Network error'));
    mockGetAllocations.mockRejectedValueOnce(new Error('Network error'));
    await act(async () => { render(<EarnPage />); });
    // Page should still render without crashing
    expect(screen.getByText('Earn')).toBeInTheDocument();
  });
});
