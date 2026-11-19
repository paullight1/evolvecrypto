import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  pauseBot: vi.fn(),
  stopBot: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { BotCard } from './BotCard';
import { pauseBot, stopBot } from '../../api';
import type { Bot } from '../../types';

const mockPauseBot = vi.mocked(pauseBot);
const mockStopBot = vi.mocked(stopBot);

const RUNNING_BOT: Bot = {
  bot_id: 'bot-1',
  type: 'dca',
  symbol: 'BTCUSDT',
  status: 'active',
  realized_pnl: '150.50',
  created_at: '2023-11-14T22:13:20.000Z',
};

const PAUSED_BOT: Bot = {
  bot_id: 'bot-2',
  type: 'grid',
  symbol: 'ETHUSDT',
  status: 'paused',
  realized_pnl: '-20.00',
  created_at: '2023-11-14T22:13:20.000Z',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockPauseBot.mockResolvedValue(undefined);
  mockStopBot.mockResolvedValue(undefined);
});

describe('BotCard', () => {
  it('renders symbol and bot type badge', () => {
    render(<BotCard bot={RUNNING_BOT} onUpdated={vi.fn()} />);

    expect(screen.getByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('DCA')).toBeInTheDocument();
  });

  it('renders status badge for active bot', () => {
    render(<BotCard bot={RUNNING_BOT} onUpdated={vi.fn()} />);

    expect(screen.getByText('active')).toBeInTheDocument();
  });

  it('Pause button calls pauseBot with bot id', async () => {
    const onUpdated = vi.fn();
    const user = userEvent.setup();
    render(<BotCard bot={RUNNING_BOT} onUpdated={onUpdated} />);

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Pause' }));
    });

    await waitFor(() => {
      expect(mockPauseBot).toHaveBeenCalledWith('bot-1');
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('shows Resume button when bot is paused', () => {
    render(<BotCard bot={PAUSED_BOT} onUpdated={vi.fn()} />);

    expect(screen.getByRole('button', { name: 'Resume' })).toBeInTheDocument();
  });

  it('Stop button opens confirm dialog and calls stopBot on confirm', async () => {
    const onUpdated = vi.fn();
    const user = userEvent.setup();
    render(<BotCard bot={RUNNING_BOT} onUpdated={onUpdated} />);

    await user.click(screen.getByRole('button', { name: 'Stop' }));

    // Confirm dialog should appear — both dialog title and confirm button say "Stop Bot"
    expect(screen.getAllByText('Stop Bot').length).toBeGreaterThanOrEqual(1);

    const confirmButtons = screen.getAllByRole('button', { name: 'Stop Bot' });
    await act(async () => {
      await user.click(confirmButtons[0]);
    });

    await waitFor(() => {
      expect(mockStopBot).toHaveBeenCalledWith('bot-1');
      expect(onUpdated).toHaveBeenCalled();
    });
  });

  it('renders PnL value with plus sign for positive', () => {
    render(<BotCard bot={RUNNING_BOT} onUpdated={vi.fn()} />);

    expect(screen.getByText('+150.50 USDT')).toBeInTheDocument();
  });
});
