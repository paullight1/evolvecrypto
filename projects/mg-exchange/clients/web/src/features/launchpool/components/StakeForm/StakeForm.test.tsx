import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  stakeInPool: vi.fn(),
  unstakeFromPool: vi.fn(),
  claimRewards: vi.fn(),
}));

import { StakeForm } from './StakeForm';
import { stakeInPool } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { LaunchpoolPool } from '../../types';

const mockStakeInPool = vi.mocked(stakeInPool);

const MOCK_POOL: LaunchpoolPool = {
  id: 'pool-1',
  reward_token: 'EXC',
  stake_token: 'BNB',
  apy: '25.5%',
  total_staked: '100000',
  max_stake: '500000',
  start_time: new Date(Date.now() - 86_400_000).toISOString(),
  end_time: new Date(Date.now() + 30 * 86_400_000).toISOString(),
  status: 'active',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockStakeInPool.mockResolvedValue(undefined as never);
  useToastStore.setState({ toasts: [] });
});

describe('StakeForm', () => {
  it('renders nothing when pool is null', async () => {
    await act(async () => {
      render(<StakeForm pool={null} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders modal with pool info when pool is provided', async () => {
    await act(async () => {
      render(<StakeForm pool={MOCK_POOL} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('EXC')).toBeInTheDocument();
    expect(screen.getByText('25.5%')).toBeInTheDocument();
  });

  it('renders amount input for stake token', async () => {
    await act(async () => {
      render(<StakeForm pool={MOCK_POOL} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByLabelText('Amount (BNB)')).toBeInTheDocument();
  });

  it('shows error toast when amount is invalid (zero)', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<StakeForm pool={MOCK_POOL} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    await user.type(screen.getByLabelText('Amount (BNB)'), '0');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Stake' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });

    expect(mockStakeInPool).not.toHaveBeenCalled();
  });

  it('calls stakeInPool and onSuccess on valid submit', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    await act(async () => {
      render(<StakeForm pool={MOCK_POOL} onClose={vi.fn()} onSuccess={onSuccess} />);
    });

    await user.type(screen.getByLabelText('Amount (BNB)'), '50');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Stake' }));
    });

    await waitFor(() => {
      expect(mockStakeInPool).toHaveBeenCalledWith({ pool_id: 'pool-1', amount: '50' });
      expect(onSuccess).toHaveBeenCalled();
    });
  });
});
