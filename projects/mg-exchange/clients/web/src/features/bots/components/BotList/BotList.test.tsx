import { render, screen } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';

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

import { BotList } from './BotList';
import type { Bot } from '../../types';

const MOCK_BOTS: Bot[] = [
  { id: 'bot-1', type: 'dca', symbol: 'BTCUSDT', status: 'running', pnl: '100', created_at: 1700000000 },
  { id: 'bot-2', type: 'grid', symbol: 'ETHUSDT', status: 'paused', pnl: '-5', created_at: 1700000000 },
];

describe('BotList', () => {
  it('renders a BotCard for each bot', () => {
    render(<BotList bots={MOCK_BOTS} onUpdated={vi.fn()} />);

    expect(screen.getByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
  });

  it('shows empty state message when no bots', () => {
    render(<BotList bots={[]} onUpdated={vi.fn()} />);

    expect(screen.getByText(/No bots yet/i)).toBeInTheDocument();
  });

  it('renders all bot type badges', () => {
    render(<BotList bots={MOCK_BOTS} onUpdated={vi.fn()} />);

    expect(screen.getByText('DCA')).toBeInTheDocument();
    expect(screen.getByText('Grid')).toBeInTheDocument();
  });

  it('renders status badges for all bots', () => {
    render(<BotList bots={MOCK_BOTS} onUpdated={vi.fn()} />);

    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('paused')).toBeInTheDocument();
  });

  it('renders correct number of Pause/Resume buttons', () => {
    render(<BotList bots={MOCK_BOTS} onUpdated={vi.fn()} />);

    // running bot has Pause, paused bot has Resume
    expect(screen.getByRole('button', { name: 'Pause' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Resume' })).toBeInTheDocument();
  });
});
