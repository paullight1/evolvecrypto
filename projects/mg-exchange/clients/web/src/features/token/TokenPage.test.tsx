import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getTokenSupply: vi.fn(),
  getBurnHistory: vi.fn(),
  getInsurancePool: vi.fn(),
  stakeInsurance: vi.fn(),
  unstakeInsurance: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import TokenPage from './TokenPage';
import { getTokenSupply, getBurnHistory, getInsurancePool, stakeInsurance, unstakeInsurance } from './api';
import type { TokenSupplyInfo, BurnRecord, InsuranceStakeInfo } from './types';

const mockGetTokenSupply = vi.mocked(getTokenSupply);
const mockGetBurnHistory = vi.mocked(getBurnHistory);
const mockGetInsurancePool = vi.mocked(getInsurancePool);
const mockStakeInsurance = vi.mocked(stakeInsurance);
const mockUnstakeInsurance = vi.mocked(unstakeInsurance);

const MOCK_SUPPLY: TokenSupplyInfo = {
  total_supply: '1000000000',
  circulating_supply: '450000000',
  burned_total: '50000000',
  price_usdt: '0.85',
  market_cap_usdt: '382500000',
};

const MOCK_BURNS: BurnRecord[] = [
  { id: 'burn-1', amount: '100000', tx_hash: '0xdeadbeef1234567890', burned_at: 1700000000, source: 'buyback' },
  { id: 'burn-2', amount: '50000', tx_hash: '0xcafebabedeadcode00', burned_at: 1700100000, source: 'fees' },
];

const MOCK_INSURANCE: InsuranceStakeInfo = {
  pool_total_staked: '20000000',
  user_staked: '5000',
  user_rewards_earned: '150',
  apy: '18.5%',
  lock_days: 30,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetTokenSupply.mockResolvedValue(MOCK_SUPPLY);
  mockGetBurnHistory.mockResolvedValue([]);
  mockGetInsurancePool.mockResolvedValue(MOCK_INSURANCE);
  mockStakeInsurance.mockResolvedValue(undefined);
  mockUnstakeInsurance.mockResolvedValue(undefined);
});

describe('TokenPage', () => {
  it('shows token supply info', async () => {
    render(<TokenPage />);

    // Overview tab shows supply stats
    expect(await screen.findByText('$0.85')).toBeInTheDocument();
    // circulating supply formatted
    expect(screen.getByText(/450,000,000/)).toBeInTheDocument();
  });

  it('burn history tab loads data', async () => {
    mockGetBurnHistory.mockResolvedValue(MOCK_BURNS);
    render(<TokenPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'Burn History' }));

    expect(mockGetBurnHistory).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('100000')).toBeInTheDocument();
  });

  it('insurance staking shows pool stats', async () => {
    render(<TokenPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'Insurance Staking' }));

    expect(await screen.findByText('Insurance Pool Stats')).toBeInTheDocument();
    expect(screen.getByText('18.5%')).toBeInTheDocument();
    // Lock period note: "Lock period: 30 days"
    expect(screen.getByText(/lock period/i)).toBeInTheDocument();
  });

  it('stake form validates amount', async () => {
    render(<TokenPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'Insurance Staking' }));
    await screen.findByText('Insurance Pool Stats');

    // Submit with empty amount — HTML5 required should block
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Stake' }));
    });

    expect(mockStakeInsurance).not.toHaveBeenCalled();
  });

  it('stake calls API', async () => {
    render(<TokenPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'Insurance Staking' }));
    await screen.findByText('Insurance Pool Stats');

    // The Stake form has label "Amount (EXC)" — find it
    const stakeInput = screen.getAllByLabelText('Amount (EXC)')[0];
    await userEvent.type(stakeInput, '1000');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Stake' }));
    });

    expect(mockStakeInsurance).toHaveBeenCalledWith('1000');
  });
});
