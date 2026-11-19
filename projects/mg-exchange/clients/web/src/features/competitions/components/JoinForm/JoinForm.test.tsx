import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  joinCompetition: vi.fn(),
}));

import { JoinForm } from './JoinForm';
import { joinCompetition } from '../../api';
import type { Competition } from '../../types';

const mockJoinCompetition = vi.mocked(joinCompetition);

const MOCK_COMPETITION: Competition = {
  id: 'comp-1',
  name: 'Q1 Trading Championship',
  description: 'Best ROI wins.',
  metric: 'roi',
  prize_pool: '50000',
  prize_currency: 'USDT',
  status: 'active',
  participants: 500,
  start_time: new Date(Date.now() - 86_400_000).toISOString(),
  end_time: new Date(Date.now() + 7 * 86_400_000).toISOString(),
  is_joined: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockJoinCompetition.mockResolvedValue(undefined as never);
});

describe('JoinForm', () => {
  it('renders nothing when competition is null', async () => {
    await act(async () => {
      render(<JoinForm competition={null} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders the confirm dialog with competition name', async () => {
    await act(async () => {
      render(<JoinForm competition={MOCK_COMPETITION} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByRole('dialog', { name: /Q1 Trading Championship/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Join Competition' })).toBeInTheDocument();
  });

  it('calls joinCompetition and onSuccess when confirmed', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    const onClose = vi.fn();
    await act(async () => {
      render(<JoinForm competition={MOCK_COMPETITION} onClose={onClose} onSuccess={onSuccess} />);
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Join Competition' }));
    });

    await waitFor(() => {
      expect(mockJoinCompetition).toHaveBeenCalledWith({ competition_id: 'comp-1' });
      expect(onSuccess).toHaveBeenCalled();
    });
  });

  it('calls onClose when Cancel is clicked', async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    await act(async () => {
      render(<JoinForm competition={MOCK_COMPETITION} onClose={onClose} onSuccess={vi.fn()} />);
    });

    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(onClose).toHaveBeenCalled();
  });
});
