import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getLeaderboard: vi.fn(),
  followTrader: vi.fn(),
  unfollowTrader: vi.fn(),
  getMyFollowing: vi.fn(),
  getCopyTradingSettings: vi.fn(),
  updateSettings: vi.fn(),
}));

import CopyTradingPage from './CopyTradingPage';
import { getLeaderboard, followTrader, getMyFollowing, unfollowTrader } from './api';
import type { MasterTrader, FollowRelation } from './types';

const mockGetLeaderboard = vi.mocked(getLeaderboard);
const mockFollowTrader = vi.mocked(followTrader);
const mockGetMyFollowing = vi.mocked(getMyFollowing);
const mockUnfollowTrader = vi.mocked(unfollowTrader);

const MOCK_TRADERS: MasterTrader[] = [
  {
    user_id: 'trader-1',
    display_name: 'Alpha Trader',
    total_pnl: '50000',
    win_rate: '68',
    followers_count: 420,
    profit_share_rate: '10',
    is_active: true,
  },
  {
    user_id: 'trader-2',
    display_name: 'Beta Trader',
    total_pnl: '-2000',
    win_rate: '45',
    followers_count: 80,
    profit_share_rate: '10',
    is_active: true,
  },
];

const MOCK_FOLLOWING: FollowRelation[] = [
  {
    relation_id: 'rel-1',
    master_id: 'Alpha Trader',
    allocation: '5000',
    used_allocation: '0',
    proportional: false,
    stop_loss_pct: '0',
    total_pnl: '625',
    is_active: true,
    created_at: '2023-11-14T22:13:20.000Z',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetLeaderboard.mockResolvedValue(MOCK_TRADERS);
  mockGetMyFollowing.mockResolvedValue(MOCK_FOLLOWING);
});

describe('CopyTradingPage', () => {
  it('renders title and tabs', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    expect(screen.getByText('Copy Trading')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Leaderboard' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Following' })).toBeInTheDocument();
  });

  it('loads and shows leaderboard', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    expect(await screen.findByText('Alpha Trader')).toBeInTheDocument();
    expect(screen.getByText('Beta Trader')).toBeInTheDocument();
  });

  it('displays win rate values', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    await screen.findByText('Alpha Trader');
    expect(screen.getByText('68%')).toBeInTheDocument();
    expect(screen.getByText('45%')).toBeInTheDocument();
  });

  it('opens follow modal when Follow is clicked', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    const followBtns = await screen.findAllByRole('button', { name: 'Follow' });
    await userEvent.click(followBtns[0]);
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Follow Trader')).toBeInTheDocument();
    // Alpha Trader appears in both the table and the modal — verify dialog has it
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveTextContent('Alpha Trader');
  });

  it('validates allocation in follow modal', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    const followBtns = await screen.findAllByRole('button', { name: 'Follow' });
    await userEvent.click(followBtns[0]);
    await userEvent.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    expect(screen.getByText('Enter a valid allocation amount')).toBeInTheDocument();
  });

  it('validates stop loss percentage', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    const followBtns = await screen.findAllByRole('button', { name: 'Follow' });
    await userEvent.click(followBtns[0]);
    await userEvent.type(screen.getByLabelText('Allocation (USDT)'), '1000');
    await userEvent.type(screen.getByLabelText('Stop Loss % (optional)'), '150');
    await userEvent.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    expect(screen.getByText('Stop loss must be between 0 and 100')).toBeInTheDocument();
  });

  it('calls followTrader with correct args', async () => {
    mockFollowTrader.mockResolvedValueOnce({ relation_id: 'new-rel' });
    await act(async () => { render(<CopyTradingPage />); });
    const followBtns = await screen.findAllByRole('button', { name: 'Follow' });
    await userEvent.click(followBtns[0]);
    await userEvent.type(screen.getByLabelText('Allocation (USDT)'), '2000');
    await userEvent.type(screen.getByLabelText('Stop Loss % (optional)'), '15');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Confirm Follow' }));
    });
    expect(mockFollowTrader).toHaveBeenCalledWith({
      master_id: 'trader-1',
      allocation: '2000',
      stop_loss_pct: '15',
    });
  });

  it('shows following list on Following tab', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    await userEvent.click(screen.getByRole('tab', { name: 'Following' }));
    expect(await screen.findByText('Alpha Trader')).toBeInTheDocument();
    expect(screen.getByText('5000 USDT')).toBeInTheDocument();
  });

  it('shows positive pnl in following list', async () => {
    await act(async () => { render(<CopyTradingPage />); });
    await userEvent.click(screen.getByRole('tab', { name: 'Following' }));
    await screen.findByText('Alpha Trader');
    expect(screen.getByText('+625 USDT')).toBeInTheDocument();
  });

  it('calls unfollowTrader after confirmation', async () => {
    mockUnfollowTrader.mockResolvedValueOnce(undefined);
    await act(async () => { render(<CopyTradingPage />); });
    await userEvent.click(screen.getByRole('tab', { name: 'Following' }));
    await screen.findByText('Alpha Trader');
    // Click the Unfollow button in the list card
    const unfollowBtns = screen.getAllByRole('button', { name: 'Unfollow' });
    await userEvent.click(unfollowBtns[0]);
    // Confirm dialog appears with title
    expect(screen.getByText('Unfollow Trader')).toBeInTheDocument();
    // Click the confirm button (last Unfollow button is in the dialog)
    const allUnfollowBtns = screen.getAllByRole('button', { name: 'Unfollow' });
    await act(async () => {
      await userEvent.click(allUnfollowBtns[allUnfollowBtns.length - 1]);
    });
    expect(mockUnfollowTrader).toHaveBeenCalledWith('rel-1');
  });

  it('shows empty state in following tab when no relations', async () => {
    mockGetMyFollowing.mockResolvedValueOnce([]);
    await act(async () => { render(<CopyTradingPage />); });
    await userEvent.click(screen.getByRole('tab', { name: 'Following' }));
    expect(
      await screen.findByText(/You are not following any traders/),
    ).toBeInTheDocument();
  });

  it('handles leaderboard load failure gracefully', async () => {
    mockGetLeaderboard.mockRejectedValueOnce(new Error('Network error'));
    await act(async () => { render(<CopyTradingPage />); });
    expect(screen.getByText('Copy Trading')).toBeInTheDocument();
  });
});
