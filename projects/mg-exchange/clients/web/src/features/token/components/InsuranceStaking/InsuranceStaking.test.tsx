import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  stakeInsurance: vi.fn(),
  unstakeInsurance: vi.fn(),
}));

import { InsuranceStaking } from './InsuranceStaking';
import { stakeInsurance, unstakeInsurance } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { InsuranceStakeInfo } from '../../types';

const mockStake = vi.mocked(stakeInsurance);
const mockUnstake = vi.mocked(unstakeInsurance);

const MOCK_INFO: InsuranceStakeInfo = {
  pool_total_staked: '5000000',
  user_staked: '1000',
  user_rewards_earned: '25.5',
  apy: '18.5%',
  lock_days: 7,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockStake.mockResolvedValue(undefined as never);
  mockUnstake.mockResolvedValue(undefined as never);
  useToastStore.setState({ toasts: [] });
});

describe('InsuranceStaking', () => {
  it('renders pool stats when info is provided', async () => {
    await act(async () => {
      render(<InsuranceStaking info={MOCK_INFO} onUpdated={vi.fn()} />);
    });

    expect(screen.getByText(/5,000,000.*EXC/)).toBeInTheDocument();
    expect(screen.getByText('1000 EXC')).toBeInTheDocument();
    expect(screen.getByText('18.5%')).toBeInTheDocument();
  });

  it('renders stake and unstake forms', async () => {
    await act(async () => {
      render(<InsuranceStaking info={MOCK_INFO} onUpdated={vi.fn()} />);
    });

    expect(screen.getByText('Stake EXC')).toBeInTheDocument();
    expect(screen.getByText('Unstake EXC')).toBeInTheDocument();
  });

  it('calls stakeInsurance and onUpdated when stake form is submitted', async () => {
    const user = userEvent.setup();
    const onUpdated = vi.fn();
    await act(async () => {
      render(<InsuranceStaking info={MOCK_INFO} onUpdated={onUpdated} />);
    });

    // Find the Stake button's parent form and type in its input
    const stakeBtn = screen.getByRole('button', { name: 'Stake' });
    const stakeForm = stakeBtn.closest('form')!;
    const stakeInput = stakeForm.querySelector('input')!;
    await user.type(stakeInput, '500');

    await act(async () => {
      await user.click(stakeBtn);
    });

    await waitFor(() => {
      expect(mockStake).toHaveBeenCalledWith('500');
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('calls unstakeInsurance and onUpdated when unstake form is submitted', async () => {
    const user = userEvent.setup();
    const onUpdated = vi.fn();
    await act(async () => {
      render(<InsuranceStaking info={MOCK_INFO} onUpdated={onUpdated} />);
    });

    // Find the Unstake button's parent form and type in its input
    const unstakeBtn = screen.getByRole('button', { name: 'Unstake' });
    const unstakeForm = unstakeBtn.closest('form')!;
    const unstakeInput = unstakeForm.querySelector('input')!;
    await user.type(unstakeInput, '200');

    await act(async () => {
      await user.click(unstakeBtn);
    });

    await waitFor(() => {
      expect(mockUnstake).toHaveBeenCalledWith('200');
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('shows placeholder dashes when info is null', async () => {
    await act(async () => {
      render(<InsuranceStaking info={null} onUpdated={vi.fn()} />);
    });

    expect(screen.getAllByText('—').length).toBeGreaterThan(0);
  });
});
