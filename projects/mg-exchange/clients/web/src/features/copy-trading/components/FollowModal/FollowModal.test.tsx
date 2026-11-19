import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  followTrader: vi.fn(),
  getLeaderboard: vi.fn(),
  unfollowTrader: vi.fn(),
  getMyFollowing: vi.fn(),
  getCopyTradingSettings: vi.fn(),
  updateSettings: vi.fn(),
}));

import { FollowModal } from './FollowModal';
import { followTrader } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { MasterTrader } from '../../types';

const mockFollowTrader = vi.mocked(followTrader);

const MOCK_TRADER: MasterTrader = {
  user_id: 'trader-1',
  display_name: 'AlphaTrader',
  total_pnl: '50000',
  win_rate: '68',
  followers_count: 250,
  profit_share_rate: '10',
  is_active: true,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockFollowTrader.mockResolvedValue({ relation_id: 'rel-1' });
  useToastStore.setState({ toasts: [] });
});

describe('FollowModal', () => {
  it('does not render when trader is null', async () => {
    await act(async () => {
      render(<FollowModal trader={null} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.queryByText('Follow Trader')).not.toBeInTheDocument();
  });

  it('renders modal title when trader is provided', async () => {
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByText('Follow Trader')).toBeInTheDocument();
  });

  it('shows trader display name', async () => {
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByText('AlphaTrader')).toBeInTheDocument();
  });

  it('shows ROI 30d as N/A (field removed from backend)', async () => {
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getAllByText('N/A').length).toBeGreaterThanOrEqual(1);
  });

  it('shows trader win rate', async () => {
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByText('68%')).toBeInTheDocument();
  });

  it('shows max drawdown as N/A (field removed from backend)', async () => {
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getAllByText('N/A').length).toBeGreaterThanOrEqual(2);
  });

  it('renders allocation input', async () => {
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByLabelText(/Allocation/i)).toBeInTheDocument();
  });

  it('shows validation error when allocation is 0', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    // Leave allocation empty and click submit
    await user.click(screen.getByRole('button', { name: 'Confirm Follow' }));

    await waitFor(() => {
      expect(screen.getByText('Enter a valid allocation amount')).toBeInTheDocument();
    });

    expect(mockFollowTrader).not.toHaveBeenCalled();
  });

  it('calls followTrader API with correct params on valid submit', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();

    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={onSuccess} />);
    });

    const allocationInput = screen.getByLabelText(/Allocation/i);
    await user.type(allocationInput, '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    });

    await waitFor(() => {
      expect(mockFollowTrader).toHaveBeenCalledWith(
        expect.objectContaining({
          master_id: 'trader-1',
          allocation: '1000',
        }),
      );
    });
  });

  it('calls onSuccess after successfully following a trader', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();

    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={onSuccess} />);
    });

    const allocationInput = screen.getByLabelText(/Allocation/i);
    await user.type(allocationInput, '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    });

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalled();
    });
  });

  it('shows success toast after following', async () => {
    const user = userEvent.setup();

    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    const allocationInput = screen.getByLabelText(/Allocation/i);
    await user.type(allocationInput, '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'success' && t.message.includes('AlphaTrader'))).toBe(true);
    });
  });

  it('shows error toast on API failure', async () => {
    mockFollowTrader.mockRejectedValue(new Error('Server error'));
    const user = userEvent.setup();

    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    const allocationInput = screen.getByLabelText(/Allocation/i);
    await user.type(allocationInput, '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });
  });

  it('shows stop loss validation error when stop loss pct is > 100', async () => {
    const user = userEvent.setup();

    await act(async () => {
      render(<FollowModal trader={MOCK_TRADER} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    const allocationInput = screen.getByLabelText(/Allocation/i);
    await user.type(allocationInput, '1000');

    const stopLossInput = screen.getByLabelText(/Stop Loss/i);
    await user.type(stopLossInput, '150');

    await user.click(screen.getByRole('button', { name: 'Confirm Follow' }));

    await waitFor(() => {
      expect(screen.getByText('Stop loss must be between 0 and 100')).toBeInTheDocument();
    });

    expect(mockFollowTrader).not.toHaveBeenCalled();
  });
});
