import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getLeaderboard: vi.fn(),
}));

import { LeaderboardTable } from './LeaderboardTable';
import { getLeaderboard } from '../../api';
import type { Competition, CompetitionEntry } from '../../types';

const mockGetLeaderboard = vi.mocked(getLeaderboard);

const MOCK_COMPETITION: Competition = {
  id: 'comp-1',
  name: 'Q1 Championship',
  description: '',
  metric: 'roi',
  prize_pool: '50000',
  prize_currency: 'USDT',
  status: 'active',
  participants: 10,
  start_time: new Date().toISOString(),
  end_time: new Date().toISOString(),
  is_joined: false,
};

const MOCK_ENTRIES: CompetitionEntry[] = [
  { rank: 1, user_id: 'u-1', nickname: 'Alice', score: '250.5', prize: '10000 USDT' },
  { rank: 2, user_id: 'u-2', nickname: 'Bob', score: '180.2', prize: '5000 USDT' },
  { rank: 3, user_id: 'u-3', nickname: 'Carol', score: '120.0', prize: '2500 USDT' },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetLeaderboard.mockResolvedValue(MOCK_ENTRIES);
});

describe('LeaderboardTable', () => {
  it('does not render modal when competition is null', async () => {
    await act(async () => {
      render(<LeaderboardTable competition={null} onClose={vi.fn()} />);
    });

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders leaderboard entries sorted by rank', async () => {
    await act(async () => {
      render(<LeaderboardTable competition={MOCK_COMPETITION} onClose={vi.fn()} />);
    });

    await waitFor(() => {
      expect(screen.getByText('Alice')).toBeInTheDocument();
      expect(screen.getByText('Bob')).toBeInTheDocument();
      expect(screen.getByText('Carol')).toBeInTheDocument();
    });
  });

  it('renders score and prize columns', async () => {
    await act(async () => {
      render(<LeaderboardTable competition={MOCK_COMPETITION} onClose={vi.fn()} />);
    });

    await waitFor(() => {
      expect(screen.getByText('250.5')).toBeInTheDocument();
      expect(screen.getByText('10000 USDT')).toBeInTheDocument();
    });
  });

  it('calls onClose when modal is closed', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    await act(async () => {
      render(<LeaderboardTable competition={MOCK_COMPETITION} onClose={onClose} />);
    });

    await waitFor(() => screen.getByRole('dialog'));

    await user.click(screen.getByRole('button', { name: 'Close modal' }));

    expect(onClose).toHaveBeenCalled();
  });

  it('loads leaderboard data when competition changes', async () => {
    await act(async () => {
      render(<LeaderboardTable competition={MOCK_COMPETITION} onClose={vi.fn()} />);
    });

    await waitFor(() => {
      expect(mockGetLeaderboard).toHaveBeenCalledWith('comp-1');
    });
  });
});
