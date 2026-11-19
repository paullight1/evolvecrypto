import { render, screen, within, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  stakingStake: vi.fn(),
  stakingUnstake: vi.fn(),
  stakingClaimRewards: vi.fn(),
  getStakingBalance: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { StakingPanel } from './StakingPanel';
import { stakingStake, stakingUnstake, getStakingBalance } from '../../api';

const mockStakingStake = vi.mocked(stakingStake);
const mockStakingUnstake = vi.mocked(stakingUnstake);
const mockGetStakingBalance = vi.mocked(getStakingBalance);

const MOCK_BALANCE = {
  asset: 'ETH',
  staked: '2.5',
  rewards_earned: '0.05',
  apy: '4.2%',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetStakingBalance.mockResolvedValue(MOCK_BALANCE);
  mockStakingStake.mockResolvedValue(undefined);
  mockStakingUnstake.mockResolvedValue(undefined);
});

describe('StakingPanel', () => {
  it('renders Stake and Unstake sections', async () => {
    await act(async () => { render(<StakingPanel />); });

    expect(screen.getByText('Stake')).toBeInTheDocument();
    expect(screen.getByText('Unstake')).toBeInTheDocument();
  });

  it('shows staking balance after load', async () => {
    await act(async () => { render(<StakingPanel />); });

    await waitFor(() => {
      expect(screen.getByText('2.5 ETH')).toBeInTheDocument();
    });
  });

  it('Stake button calls stakingStake with valid amount', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<StakingPanel />); });

    await waitFor(() => expect(mockGetStakingBalance).toHaveBeenCalled());

    // Both forms share the same label text — scope to the Stake card section
    const stakeHeading = screen.getByText('Stake');
    const stakeSection = stakeHeading.closest('div') as HTMLElement;
    const stakeInput = within(stakeSection).getByPlaceholderText('0.00');
    await user.type(stakeInput, '1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Stake ETH' }));
    });

    await waitFor(() => {
      expect(mockStakingStake).toHaveBeenCalledWith({ asset: 'ETH', amount: '1' });
    });
  });

  it('Unstake button calls stakingUnstake with valid amount', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<StakingPanel />); });

    await waitFor(() => expect(mockGetStakingBalance).toHaveBeenCalled());

    // Scope to the Unstake card section
    const unstakeHeading = screen.getByText('Unstake');
    const unstakeSection = unstakeHeading.closest('div') as HTMLElement;
    const unstakeInput = within(unstakeSection).getByPlaceholderText('0.00');
    await user.type(unstakeInput, '0.5');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Unstake ETH' }));
    });

    await waitFor(() => {
      expect(mockStakingUnstake).toHaveBeenCalledWith({ asset: 'ETH', amount: '0.5' });
    });
  });

  it('asset selector is rendered with default ETH', async () => {
    await act(async () => { render(<StakingPanel />); });

    const assetSelect = screen.getByLabelText('Asset') as HTMLSelectElement;
    expect(assetSelect.value).toBe('ETH');
  });
});
