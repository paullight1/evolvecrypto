import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  unstakeFromPool: vi.fn(),
  claimRewards: vi.fn(),
  stakeInPool: vi.fn(),
}));

import { MyStakesTable } from './MyStakesTable';
import { unstakeFromPool, claimRewards } from '../../api';
import type { LaunchpoolStake } from '../../types';

const mockUnstake = vi.mocked(unstakeFromPool);
const mockClaim = vi.mocked(claimRewards);

const MOCK_STAKES: LaunchpoolStake[] = [
  {
    id: 'stake-1',
    pool_id: 'pool-1',
    reward_token: 'EXC',
    stake_token: 'BNB',
    staked_amount: '100',
    rewards_earned: '5.25',
    staked_at: 1700000000,
  },
  {
    id: 'stake-2',
    pool_id: 'pool-2',
    reward_token: 'XYZ',
    stake_token: 'ETH',
    staked_amount: '2',
    rewards_earned: '0.10',
    staked_at: 1700100000,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockUnstake.mockResolvedValue(undefined as never);
  mockClaim.mockResolvedValue(undefined as never);
});

describe('MyStakesTable', () => {
  it('renders stake rows with token names', async () => {
    await act(async () => {
      render(<MyStakesTable stakes={MOCK_STAKES} loading={false} onUpdated={vi.fn()} />);
    });

    expect(screen.getByText('BNB')).toBeInTheDocument();
    expect(screen.getByText('ETH')).toBeInTheDocument();
  });

  it('renders staked amounts', async () => {
    await act(async () => {
      render(<MyStakesTable stakes={MOCK_STAKES} loading={false} onUpdated={vi.fn()} />);
    });

    expect(screen.getByText('100')).toBeInTheDocument();
    expect(screen.getByText('2')).toBeInTheDocument();
  });

  it('renders Claim and Unstake buttons for each row', async () => {
    await act(async () => {
      render(<MyStakesTable stakes={MOCK_STAKES} loading={false} onUpdated={vi.fn()} />);
    });

    expect(screen.getAllByRole('button', { name: 'Claim' })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'Unstake' })).toHaveLength(2);
  });

  it('calls claimRewards and onUpdated when Claim is clicked', async () => {
    const user = userEvent.setup();
    const onUpdated = vi.fn();
    await act(async () => {
      render(<MyStakesTable stakes={MOCK_STAKES} loading={false} onUpdated={onUpdated} />);
    });

    await act(async () => {
      await user.click(screen.getAllByRole('button', { name: 'Claim' })[0]);
    });

    await waitFor(() => {
      expect(mockClaim).toHaveBeenCalledWith('pool-1');
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('calls unstakeFromPool and onUpdated when Unstake is clicked', async () => {
    const user = userEvent.setup();
    const onUpdated = vi.fn();
    await act(async () => {
      render(<MyStakesTable stakes={MOCK_STAKES} loading={false} onUpdated={onUpdated} />);
    });

    await act(async () => {
      await user.click(screen.getAllByRole('button', { name: 'Unstake' })[0]);
    });

    await waitFor(() => {
      expect(mockUnstake).toHaveBeenCalledWith({ pool_id: 'pool-1', amount: '100' });
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('shows empty state when no stakes', async () => {
    await act(async () => {
      render(<MyStakesTable stakes={[]} loading={false} onUpdated={vi.fn()} />);
    });

    expect(screen.getByText('No active stakes')).toBeInTheDocument();
  });
});
