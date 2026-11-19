import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  createRfq: vi.fn(),
  getRfqQuotes: vi.fn(),
  executeRfq: vi.fn(),
  getBlockTradeHistory: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import BlockTradePage from './BlockTradePage';
import { createRfq, getRfqQuotes, executeRfq, getBlockTradeHistory } from './api';
import type { RfqQuote, BlockTrade } from './types';

const mockCreateRfq = vi.mocked(createRfq);
const mockGetRfqQuotes = vi.mocked(getRfqQuotes);
const mockExecuteRfq = vi.mocked(executeRfq);
const mockGetBlockTradeHistory = vi.mocked(getBlockTradeHistory);

const MOCK_QUOTES: RfqQuote[] = [
  {
    id: 'q-1',
    rfq_id: 'rfq-1',
    maker: 'MarketMaker1',
    price: '65000.00',
    qty: '1.5',
    notional_usd: '97500',
    side: 'buy',
    expires_at: new Date(Date.now() + 60_000).toISOString(),
    status: 'pending',
  },
];

const MOCK_TRADES: BlockTrade[] = [
  {
    id: 'bt-1',
    symbol: 'BTCUSDT',
    side: 'buy',
    qty: '1.5',
    price: '65000.00',
    notional_usd: '97500',
    counterparty: 'MarketMaker1',
    executed_at: new Date().toISOString(),
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetRfqQuotes.mockResolvedValue([]);
  mockGetBlockTradeHistory.mockResolvedValue([]);
});

describe('BlockTradePage', () => {
  it('RFQ form enforces minimum $100K notional', async () => {
    render(<BlockTradePage />);

    // Fill in quantity and a notional below minimum
    await userEvent.type(screen.getByLabelText('Quantity'), '1');
    await userEvent.type(screen.getByLabelText('Notional (USD)'), '50000');

    // Inline validation error should appear for below-min notional
    expect(screen.getByText('Minimum $100,000')).toBeInTheDocument();
  });

  it('submit RFQ calls createRfq API', async () => {
    mockCreateRfq.mockResolvedValue({
      id: 'rfq-2', symbol: 'BTCUSDT', side: 'buy', qty: '2', notional_usd: '130000',
      status: 'open', quotes: [], created_at: new Date().toISOString(),
    });
    render(<BlockTradePage />);

    await userEvent.type(screen.getByLabelText('Quantity'), '2');
    await userEvent.type(screen.getByLabelText('Notional (USD)'), '130000');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Submit RFQ' }));
    });

    expect(mockCreateRfq).toHaveBeenCalledWith({
      symbol: 'BTCUSDT',
      side: 'buy',
      qty: '2',
      notional_usd: '130000',
    });
  });

  it('quote list shows Execute button for pending quotes', async () => {
    mockGetRfqQuotes.mockResolvedValue(MOCK_QUOTES);
    render(<BlockTradePage />);

    // Navigate to Active Quotes tab
    await userEvent.click(screen.getByRole('tab', { name: 'Active Quotes' }));

    // Type an RFQ ID and click Load to trigger fetch
    await userEvent.type(screen.getByPlaceholderText('Enter RFQ ID'), 'rfq-1');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Load' }));
    });

    expect(await screen.findByRole('button', { name: 'Execute' })).toBeInTheDocument();
  });

  it('execute calls executeRfq API', async () => {
    mockGetRfqQuotes.mockResolvedValue(MOCK_QUOTES);
    mockExecuteRfq.mockResolvedValue(MOCK_TRADES[0]);
    render(<BlockTradePage />);

    await userEvent.click(screen.getByRole('tab', { name: 'Active Quotes' }));
    await userEvent.type(screen.getByPlaceholderText('Enter RFQ ID'), 'rfq-1');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Load' }));
    });

    const executeBtn = await screen.findByRole('button', { name: 'Execute' });
    await act(async () => {
      await userEvent.click(executeBtn);
    });

    expect(mockExecuteRfq).toHaveBeenCalledWith('rfq-1', 'q-1');
  });

  it('history tab loads block trades', async () => {
    mockGetBlockTradeHistory.mockResolvedValue(MOCK_TRADES);
    render(<BlockTradePage />);

    await act(async () => {
      await userEvent.click(screen.getByRole('tab', { name: 'History' }));
    });

    expect(mockGetBlockTradeHistory).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
  });
});
