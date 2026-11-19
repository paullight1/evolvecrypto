import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getPools: vi.fn(),
  stakeInPool: vi.fn(),
  unstakeFromPool: vi.fn(),
  claimRewards: vi.fn(),
  getMyStakes: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import LaunchpoolPage from './LaunchpoolPage';
import { getPools, stakeInPool, unstakeFromPool, claimRewards, getMyStakes } from './api';
import type { LaunchpoolPool, LaunchpoolStake } from './types';

const mockGetPools = vi.mocked(getPools);
const mockStakeInPool = vi.mocked(stakeInPool);
const mockUnstakeFromPool = vi.mocked(unstakeFromPool);
const mockClaimRewards = vi.mocked(claimRewards);
const mockGetMyStakes = vi.mocked(getMyStakes);

const MOCK_POOLS: LaunchpoolPool[] = [
  {
    id: 'pool-1',
    reward_token: 'EXC',
    stake_token: 'USDT',
    apy: '24.5%',
    total_staked: '1000000',
    max_stake: '50000',
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 30 * 86_400_000).toISOString(),
    status: 'active',
  },
];

const MOCK_STAKES: LaunchpoolStake[] = [
  {
    id: 'stake-1',
    pool_id: 'pool-1',
    reward_token: 'EXC',
    stake_token: 'USDT',
    staked_amount: '1000',
    rewards_earned: '10.5',
    staked_at: 1700000000,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetPools.mockResolvedValue(MOCK_POOLS);
  mockGetMyStakes.mockResolvedValue([]);
  mockStakeInPool.mockResolvedValue(undefined);
  mockUnstakeFromPool.mockResolvedValue(undefined);
  mockClaimRewards.mockResolvedValue(undefined);
});

describe('LaunchpoolPage', () => {
  it('renders pool cards with APY', async () => {
    render(<LaunchpoolPage />);

    expect(await screen.findByText('Earn EXC')).toBeInTheDocument();
    expect(screen.getByText('Stake USDT')).toBeInTheDocument();
    expect(screen.getByText('24.5%')).toBeInTheDocument();
  });

  it('stake form validates amount', async () => {
    render(<LaunchpoolPage />);

    const stakeBtn = await screen.findByRole('button', { name: 'Stake' });
    await userEvent.click(stakeBtn);

    // Modal opens — Amount field should be there
    const amountInput = await screen.findByLabelText('Amount (USDT)');
    // Enter invalid amount 0
    await userEvent.type(amountInput, '0');

    await act(async () => {
      await userEvent.click(screen.getAllByRole('button', { name: 'Stake' }).at(-1)!);
    });

    expect(mockStakeInPool).not.toHaveBeenCalled();
  });

  it('stake calls API', async () => {
    render(<LaunchpoolPage />);

    const stakeBtn = await screen.findByRole('button', { name: 'Stake' });
    await userEvent.click(stakeBtn);

    const amountInput = await screen.findByLabelText('Amount (USDT)');
    await userEvent.type(amountInput, '500');

    await act(async () => {
      await userEvent.click(screen.getAllByRole('button', { name: 'Stake' }).at(-1)!);
    });

    expect(mockStakeInPool).toHaveBeenCalledWith({ pool_id: 'pool-1', amount: '500' });
  });

  it('claim rewards calls API', async () => {
    mockGetMyStakes.mockResolvedValue(MOCK_STAKES);
    render(<LaunchpoolPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'My Stakes' }));

    const claimBtn = await screen.findByRole('button', { name: 'Claim' });
    await act(async () => {
      await userEvent.click(claimBtn);
    });

    expect(mockClaimRewards).toHaveBeenCalledWith('pool-1');
  });

  it('unstake calls API', async () => {
    mockGetMyStakes.mockResolvedValue(MOCK_STAKES);
    render(<LaunchpoolPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'My Stakes' }));

    const unstakeBtn = await screen.findByRole('button', { name: 'Unstake' });
    await act(async () => {
      await userEvent.click(unstakeBtn);
    });

    expect(mockUnstakeFromPool).toHaveBeenCalledWith({ pool_id: 'pool-1', amount: '1000' });
  });
});
