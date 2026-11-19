import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getMyBots: vi.fn(),
  createGridBot: vi.fn(),
  createDcaBot: vi.fn(),
  pauseBot: vi.fn(),
  stopBot: vi.fn(),
}));

import BotsPage from './BotsPage';
import { getMyBots, createGridBot, createDcaBot, pauseBot } from './api';
import type { Bot } from './types';

const mockGetMyBots = vi.mocked(getMyBots);
const mockCreateGridBot = vi.mocked(createGridBot);
const mockCreateDcaBot = vi.mocked(createDcaBot);
const mockPauseBot = vi.mocked(pauseBot);

const MOCK_BOTS: Bot[] = [
  {
    bot_id: 'bot-1',
    type: 'grid',
    symbol: 'BTCUSDT',
    status: 'active',
    realized_pnl: '120.50',
    created_at: '2023-11-14T22:13:20.000Z',
  },
  {
    bot_id: 'bot-2',
    type: 'dca',
    symbol: 'ETHUSDT',
    status: 'paused',
    realized_pnl: '-15.00',
    created_at: '2023-11-14T22:14:00.000Z',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetMyBots.mockResolvedValue(MOCK_BOTS);
});

describe('BotsPage', () => {
  it('renders title and tabs', () => {
    render(<BotsPage />);
    expect(screen.getByText('Trading Bots')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Marketplace' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'My Bots' })).toBeInTheDocument();
  });

  it('shows bot type cards in marketplace tab', () => {
    render(<BotsPage />);
    expect(screen.getByText('Grid Bot')).toBeInTheDocument();
    expect(screen.getByText('DCA Bot')).toBeInTheDocument();
  });

  it('navigates to Grid Bot form on click', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    expect(screen.getByText('Create Grid Bot', { selector: 'h2' })).toBeInTheDocument();
    expect(screen.getByLabelText('Upper Price')).toBeInTheDocument();
  });

  it('navigates to DCA Bot form on click', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create DCA Bot' }));
    expect(screen.getByText('Create DCA Bot', { selector: 'h2' })).toBeInTheDocument();
    expect(screen.getByLabelText('Amount per order (USDT)')).toBeInTheDocument();
  });

  it('back button returns to marketplace', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    await userEvent.click(screen.getByText('← Back to Marketplace'));
    expect(screen.getByText('Grid Bot')).toBeInTheDocument();
    expect(screen.getByText('DCA Bot')).toBeInTheDocument();
  });

  it('validates grid bot form — upper must be > lower', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    await userEvent.type(screen.getByLabelText('Upper Price'), '50000');
    await userEvent.type(screen.getByLabelText('Lower Price'), '60000');
    await userEvent.type(screen.getByLabelText('Investment (USDT)'), '1000');
    await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    expect(screen.getByText('Upper price must be greater than lower price')).toBeInTheDocument();
  });

  it('shows preview when grid bot values are valid', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    await userEvent.type(screen.getByLabelText('Upper Price'), '60000');
    await userEvent.type(screen.getByLabelText('Lower Price'), '50000');
    await userEvent.clear(screen.getByLabelText('Grid Count'));
    await userEvent.type(screen.getByLabelText('Grid Count'), '10');
    await userEvent.type(screen.getByLabelText('Investment (USDT)'), '1000');
    expect(screen.getByText('Preview')).toBeInTheDocument();
    expect(screen.getByText('Grid Spacing')).toBeInTheDocument();
  });

  it('calls createGridBot with correct data', async () => {
    mockCreateGridBot.mockResolvedValueOnce({ bot_id: 'new-bot' });
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    await userEvent.type(screen.getByLabelText('Upper Price'), '60000');
    await userEvent.type(screen.getByLabelText('Lower Price'), '50000');
    await userEvent.clear(screen.getByLabelText('Grid Count'));
    await userEvent.type(screen.getByLabelText('Grid Count'), '10');
    await userEvent.type(screen.getByLabelText('Investment (USDT)'), '1000');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Create Grid Bot' }));
    });
    expect(mockCreateGridBot).toHaveBeenCalledWith({
      symbol: 'BTCUSDT',
      upper_price: '60000',
      lower_price: '50000',
      grid_count: 10,
      investment: '1000',
    });
  });

  it('calls createDcaBot with correct data', async () => {
    mockCreateDcaBot.mockResolvedValueOnce({ bot_id: 'new-dca' });
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('button', { name: 'Create DCA Bot' }));
    await userEvent.type(screen.getByLabelText('Amount per order (USDT)'), '100');
    await userEvent.clear(screen.getByLabelText('Total Orders'));
    await userEvent.type(screen.getByLabelText('Total Orders'), '12');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Create DCA Bot' }));
    });
    expect(mockCreateDcaBot).toHaveBeenCalledWith({
      symbol: 'BTCUSDT',
      amount_per_order: '100',
      interval_secs: 86400,
      total_orders: 12,
    });
  });

  it('shows my bots on My Bots tab', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'My Bots' }));
    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
  });

  it('shows positive pnl for running bot', async () => {
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'My Bots' }));
    await screen.findByText('BTCUSDT');
    expect(screen.getByText('+120.50 USDT')).toBeInTheDocument();
  });

  it('calls pauseBot when Pause is clicked', async () => {
    mockPauseBot.mockResolvedValueOnce(undefined);
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'My Bots' }));
    const pauseBtns = await screen.findAllByRole('button', { name: 'Pause' });
    await act(async () => {
      await userEvent.click(pauseBtns[0]);
    });
    expect(mockPauseBot).toHaveBeenCalledWith('bot-1');
  });

  it('handles getMyBots failure gracefully', async () => {
    mockGetMyBots.mockRejectedValueOnce(new Error('Network error'));
    render(<BotsPage />);
    await userEvent.click(screen.getByRole('tab', { name: 'My Bots' }));
    await screen.findByText('No bots yet. Create one from the Marketplace tab.');
  });
});
