import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { PoolCard } from './PoolCard';
import type { LaunchpoolPool } from '../../types';

function makePool(overrides: Partial<LaunchpoolPool> = {}): LaunchpoolPool {
  return {
    id: 'pool-1',
    reward_token: 'EXC',
    stake_token: 'BNB',
    apy: '25.5%',
    total_staked: '100000',
    max_stake: '500000',
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 30 * 86_400_000).toISOString(),
    status: 'active',
    ...overrides,
  };
}

describe('PoolCard', () => {
  it('renders reward token label', async () => {
    await act(async () => {
      render(<PoolCard pool={makePool()} onStake={vi.fn()} />);
    });

    expect(screen.getByText('Earn EXC')).toBeInTheDocument();
    expect(screen.getByText('Stake BNB')).toBeInTheDocument();
  });

  it('shows APY value', async () => {
    await act(async () => {
      render(<PoolCard pool={makePool()} onStake={vi.fn()} />);
    });

    expect(screen.getByText('25.5%')).toBeInTheDocument();
  });

  it('shows farming period dates', async () => {
    const pool = makePool();
    await act(async () => {
      render(<PoolCard pool={pool} onStake={vi.fn()} />);
    });

    expect(screen.getByText(/Farming Period/)).toBeInTheDocument();
  });

  it('renders status badge for active pool', async () => {
    await act(async () => {
      render(<PoolCard pool={makePool({ status: 'active' })} onStake={vi.fn()} />);
    });

    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('calls onStake with pool when Stake button is clicked', async () => {
    const user = userEvent.setup();
    const onStake = vi.fn();
    await act(async () => {
      render(<PoolCard pool={makePool()} onStake={onStake} />);
    });

    await user.click(screen.getByRole('button', { name: 'Stake' }));

    expect(onStake).toHaveBeenCalledWith(expect.objectContaining({ id: 'pool-1' }));
  });

  it('shows Coming Soon button and disables it for upcoming pool', async () => {
    await act(async () => {
      render(<PoolCard pool={makePool({ status: 'upcoming' })} onStake={vi.fn()} />);
    });

    const btn = screen.getByRole('button', { name: 'Coming Soon' });
    expect(btn).toBeDisabled();
  });
});
