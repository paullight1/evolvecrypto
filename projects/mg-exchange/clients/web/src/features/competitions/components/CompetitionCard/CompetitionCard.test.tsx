import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { CompetitionCard } from './CompetitionCard';
import type { Competition } from '../../types';

function makeCompetition(overrides: Partial<Competition> = {}): Competition {
  return {
    id: 'comp-1',
    name: 'Q1 Trading Championship',
    description: 'Compete for the best ROI this quarter.',
    metric: 'roi',
    prize_pool: '50000',
    prize_currency: 'USDT',
    status: 'active',
    participants: 1234,
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 7 * 86_400_000).toISOString(),
    is_joined: false,
    ...overrides,
  };
}

describe('CompetitionCard', () => {
  it('renders competition name and description', async () => {
    await act(async () => {
      render(<CompetitionCard competition={makeCompetition()} onJoin={vi.fn()} onViewLeaderboard={vi.fn()} />);
    });

    expect(screen.getByText('Q1 Trading Championship')).toBeInTheDocument();
    expect(screen.getByText('Compete for the best ROI this quarter.')).toBeInTheDocument();
  });

  it('shows prize pool', async () => {
    await act(async () => {
      render(<CompetitionCard competition={makeCompetition()} onJoin={vi.fn()} onViewLeaderboard={vi.fn()} />);
    });

    expect(screen.getByText(/50,000.*USDT/)).toBeInTheDocument();
  });

  it('shows status badge', async () => {
    await act(async () => {
      render(<CompetitionCard competition={makeCompetition({ status: 'active' })} onJoin={vi.fn()} onViewLeaderboard={vi.fn()} />);
    });

    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('shows Join button for active, unjoined competition and calls onJoin', async () => {
    const user = userEvent.setup();
    const onJoin = vi.fn();
    await act(async () => {
      render(<CompetitionCard competition={makeCompetition({ is_joined: false, status: 'active' })} onJoin={onJoin} onViewLeaderboard={vi.fn()} />);
    });

    const joinBtn = screen.getByRole('button', { name: 'Join' });
    await user.click(joinBtn);

    expect(onJoin).toHaveBeenCalledWith(expect.objectContaining({ id: 'comp-1' }));
  });

  it('shows Joined badge instead of Join button when already joined', async () => {
    await act(async () => {
      render(<CompetitionCard competition={makeCompetition({ is_joined: true })} onJoin={vi.fn()} onViewLeaderboard={vi.fn()} />);
    });

    expect(screen.queryByRole('button', { name: 'Join' })).not.toBeInTheDocument();
    expect(screen.getByText('Joined')).toBeInTheDocument();
  });

  it('calls onViewLeaderboard when Leaderboard button is clicked', async () => {
    const user = userEvent.setup();
    const onViewLeaderboard = vi.fn();
    await act(async () => {
      render(<CompetitionCard competition={makeCompetition()} onJoin={vi.fn()} onViewLeaderboard={onViewLeaderboard} />);
    });

    await user.click(screen.getByRole('button', { name: 'Leaderboard' }));

    expect(onViewLeaderboard).toHaveBeenCalledWith(expect.objectContaining({ id: 'comp-1' }));
  });
});
