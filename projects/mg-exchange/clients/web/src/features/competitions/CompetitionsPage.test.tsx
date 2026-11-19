import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getCompetitions: vi.fn(),
  getLeaderboard: vi.fn(),
  joinCompetition: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import CompetitionsPage from './CompetitionsPage';
import { getCompetitions, joinCompetition, getLeaderboard } from './api';
import type { Competition } from './types';

const mockGetCompetitions = vi.mocked(getCompetitions);
const mockJoinCompetition = vi.mocked(joinCompetition);
const mockGetLeaderboard = vi.mocked(getLeaderboard);

const ACTIVE_COMPETITIONS: Competition[] = [
  {
    id: 'comp-1',
    name: 'Q1 PnL Challenge',
    description: 'Best PnL wins',
    metric: 'pnl',
    prize_pool: '10000',
    prize_currency: 'USDT',
    status: 'active',
    participants: 250,
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 86_400_000).toISOString(),
    is_joined: false,
  },
  {
    id: 'comp-2',
    name: 'Volume King',
    description: 'Most volume wins',
    metric: 'volume',
    prize_pool: '5000',
    prize_currency: 'USDT',
    status: 'active',
    participants: 180,
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 86_400_000).toISOString(),
    is_joined: true,
  },
];

const PAST_COMPETITIONS: Competition[] = [
  {
    id: 'comp-3',
    name: 'Winter Championship',
    description: 'Best ROI',
    metric: 'roi',
    prize_pool: '20000',
    prize_currency: 'USDT',
    status: 'ended',
    participants: 500,
    start_time: new Date(Date.now() - 30 * 86_400_000).toISOString(),
    end_time: new Date(Date.now() - 86_400_000).toISOString(),
    is_joined: false,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetCompetitions.mockResolvedValue([...ACTIVE_COMPETITIONS, ...PAST_COMPETITIONS]);
  mockGetLeaderboard.mockResolvedValue([]);
  mockJoinCompetition.mockResolvedValue(undefined);
});

describe('CompetitionsPage', () => {
  it('renders active competitions', async () => {
    render(<CompetitionsPage />);

    expect(await screen.findByText('Q1 PnL Challenge')).toBeInTheDocument();
    expect(screen.getByText('Volume King')).toBeInTheDocument();
  });

  it('join button calls joinCompetition API', async () => {
    mockJoinCompetition.mockResolvedValue(undefined);
    render(<CompetitionsPage />);

    // Wait for competitions to load, then click Join on the unjoined competition
    const joinBtn = await screen.findByRole('button', { name: 'Join' });
    await act(async () => {
      await userEvent.click(joinBtn);
    });

    // ConfirmDialog appears — confirm it
    const confirmBtn = await screen.findByRole('button', { name: 'Join Competition' });
    await act(async () => {
      await userEvent.click(confirmBtn);
    });

    expect(mockJoinCompetition).toHaveBeenCalledWith({ competition_id: 'comp-1' });
  });

  it('leaderboard loads on competition click', async () => {
    mockGetLeaderboard.mockResolvedValue([
      { rank: 1, user_id: 'u-1', nickname: 'Trader1', score: '5000', prize: '1000 USDT' },
    ]);
    render(<CompetitionsPage />);

    const leaderboardBtn = (await screen.findAllByRole('button', { name: 'Leaderboard' }))[0];
    await act(async () => {
      await userEvent.click(leaderboardBtn);
    });

    expect(mockGetLeaderboard).toHaveBeenCalledWith('comp-1');
    expect(await screen.findByText('Trader1')).toBeInTheDocument();
  });

  it('shows past competitions in separate tab', async () => {
    render(<CompetitionsPage />);

    // Wait for data, switch to Past tab
    await screen.findByText('Q1 PnL Challenge');
    await userEvent.click(screen.getByRole('tab', { name: 'Past' }));

    await waitFor(() => {
      expect(screen.getByText('Winter Championship')).toBeInTheDocument();
    });
    // Active competitions should no longer be visible
    expect(screen.queryByText('Q1 PnL Challenge')).not.toBeInTheDocument();
  });

  it('Upcoming tab shows no results when no upcoming competitions', async () => {
    // Mock returns only active and ended — nothing with status 'upcoming'
    render(<CompetitionsPage />);

    await screen.findByText('Q1 PnL Challenge');
    await userEvent.click(screen.getByRole('tab', { name: 'Upcoming' }));

    await waitFor(() => {
      // Active and past items should not be visible
      expect(screen.queryByText('Q1 PnL Challenge')).not.toBeInTheDocument();
      expect(screen.queryByText('Winter Championship')).not.toBeInTheDocument();
    });
  });

  it('already-joined competition shows joined state (no Join button)', async () => {
    render(<CompetitionsPage />);

    await screen.findByText('Volume King');

    // comp-2 has is_joined: true — the card should not offer a Join button for it
    // The page renders two cards; only one should have a Join button
    const joinBtns = screen.getAllByRole('button', { name: 'Join' });
    expect(joinBtns).toHaveLength(1);
  });

  it('shows Competitions page title', async () => {
    render(<CompetitionsPage />);
    expect(screen.getByText('Competitions')).toBeInTheDocument();
  });

  it('Active tab is selected by default', async () => {
    render(<CompetitionsPage />);

    // Active competitions visible immediately after load
    expect(await screen.findByText('Q1 PnL Challenge')).toBeInTheDocument();
    expect(screen.queryByText('Winter Championship')).not.toBeInTheDocument();
  });
});
