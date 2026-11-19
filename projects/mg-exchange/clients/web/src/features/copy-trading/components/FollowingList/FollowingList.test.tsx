import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  unfollowTrader: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { FollowingList } from './FollowingList';
import { unfollowTrader } from '../../api';
import type { FollowRelation } from '../../types';

const mockUnfollowTrader = vi.mocked(unfollowTrader);

const MOCK_RELATIONS: FollowRelation[] = [
  {
    relation_id: 'rel-1',
    master_id: 'AlphaTrade',
    allocation: '1000',
    used_allocation: '0',
    proportional: false,
    stop_loss_pct: '0',
    total_pnl: '150',
    is_active: true,
    created_at: '2023-11-14T22:13:20.000Z',
  },
  {
    relation_id: 'rel-2',
    master_id: 'CryptoMaster',
    allocation: '500',
    used_allocation: '0',
    proportional: false,
    stop_loss_pct: '0',
    total_pnl: '-20',
    is_active: false,
    created_at: '2023-11-14T22:13:20.000Z',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockUnfollowTrader.mockResolvedValue(undefined);
});

describe('FollowingList', () => {
  it('renders followed trader names', () => {
    render(<FollowingList relations={MOCK_RELATIONS} onUpdated={vi.fn()} />);

    expect(screen.getByText('AlphaTrade')).toBeInTheDocument();
    expect(screen.getByText('CryptoMaster')).toBeInTheDocument();
  });

  it('shows Unfollow button only for active relations', () => {
    render(<FollowingList relations={MOCK_RELATIONS} onUpdated={vi.fn()} />);

    const unfollowBtns = screen.getAllByRole('button', { name: 'Unfollow' });
    expect(unfollowBtns).toHaveLength(1);
  });

  it('clicking Unfollow opens confirm dialog', async () => {
    const user = userEvent.setup();
    render(<FollowingList relations={MOCK_RELATIONS} onUpdated={vi.fn()} />);

    await user.click(screen.getByRole('button', { name: 'Unfollow' }));

    expect(screen.getByText('Unfollow Trader')).toBeInTheDocument();
  });

  it('confirming unfollow calls unfollowTrader and fires onUpdated', async () => {
    const onUpdated = vi.fn();
    const user = userEvent.setup();
    render(<FollowingList relations={MOCK_RELATIONS} onUpdated={onUpdated} />);

    // First click opens the confirm dialog (only 1 unfollow button at this point)
    await user.click(screen.getByRole('button', { name: 'Unfollow' }));

    // Now both the original button and the confirm button exist
    const unfollowBtns = screen.getAllByRole('button', { name: 'Unfollow' });
    // Click the confirm dialog button (last one = primary confirm)
    await act(async () => {
      await user.click(unfollowBtns[unfollowBtns.length - 1]);
    });

    await waitFor(() => {
      expect(mockUnfollowTrader).toHaveBeenCalledWith('rel-1');
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('shows empty message when no relations', () => {
    render(<FollowingList relations={[]} onUpdated={vi.fn()} />);

    expect(screen.getByText(/not following any traders/i)).toBeInTheDocument();
  });
});
