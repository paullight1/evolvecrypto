import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { LeaderboardTable } from './LeaderboardTable';
import type { MasterTrader } from '../../types';

const MOCK_TRADERS: MasterTrader[] = [
  {
    user_id: 'u-1',
    display_name: 'AlphaTrade',
    total_pnl: '15000',
    win_rate: '68',
    followers_count: 120,
    profit_share_rate: '10',
    is_active: true,
  },
  {
    user_id: 'u-2',
    display_name: 'CryptoMaster',
    total_pnl: '-500',
    win_rate: '45',
    followers_count: 30,
    profit_share_rate: '10',
    is_active: true,
  },
];

describe('LeaderboardTable', () => {
  it('renders trader display names', () => {
    render(<LeaderboardTable traders={MOCK_TRADERS} onFollow={vi.fn()} />);

    expect(screen.getByText('AlphaTrade')).toBeInTheDocument();
    expect(screen.getByText('CryptoMaster')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<LeaderboardTable traders={MOCK_TRADERS} onFollow={vi.fn()} />);

    expect(screen.getByText('Trader')).toBeInTheDocument();
    expect(screen.getByText('Total PnL')).toBeInTheDocument();
    expect(screen.getByText('Win Rate')).toBeInTheDocument();
    expect(screen.getByText('Followers')).toBeInTheDocument();
  });

  it('renders a Follow button for each trader', () => {
    render(<LeaderboardTable traders={MOCK_TRADERS} onFollow={vi.fn()} />);

    const followBtns = screen.getAllByRole('button', { name: 'Follow' });
    expect(followBtns).toHaveLength(2);
  });

  it('clicking Follow fires onFollow with the trader object', async () => {
    const user = userEvent.setup();
    const onFollow = vi.fn();
    render(<LeaderboardTable traders={MOCK_TRADERS} onFollow={onFollow} />);

    const followBtns = screen.getAllByRole('button', { name: 'Follow' });
    await user.click(followBtns[0]);

    expect(onFollow).toHaveBeenCalledWith(
      expect.objectContaining({ user_id: 'u-1', display_name: 'AlphaTrade' }),
    );
  });

  it('shows empty state when no traders', () => {
    render(<LeaderboardTable traders={[]} onFollow={vi.fn()} />);

    expect(screen.getByText('No traders available')).toBeInTheDocument();
  });
});
