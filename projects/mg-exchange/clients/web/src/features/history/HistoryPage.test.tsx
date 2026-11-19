import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getOrderHistory: vi.fn(),
  getTradeHistory: vi.fn(),
  getFundingHistory: vi.fn(),
  getTaxReport: vi.fn(),
}));

import HistoryPage from './HistoryPage';
import { getOrderHistory, getTradeHistory, getFundingHistory } from './api';
import type { TradeRecord } from './types';
import type { OpenOrder } from '@features/trading/types';

const mockGetOrderHistory = vi.mocked(getOrderHistory);
const mockGetTradeHistory = vi.mocked(getTradeHistory);
const mockGetFundingHistory = vi.mocked(getFundingHistory);

const MOCK_ORDERS: OpenOrder[] = [
  {
    order_id: 'ord-hist-1',
    symbol: 'BTCUSDT',
    side: 'buy',
    order_type: 'limit',
    price: '64000',
    qty: '0.2',
    filled_qty: '0.2',
    status: 'filled',
    created_at: new Date().toISOString(),
  },
  {
    order_id: 'ord-hist-2',
    symbol: 'ETHUSDT',
    side: 'sell',
    order_type: 'market',
    price: null,
    qty: '1',
    filled_qty: '1',
    status: 'filled',
    created_at: new Date().toISOString(),
  },
];

const MOCK_TRADES: TradeRecord[] = [
  {
    trade_id: 'trade-1',
    symbol: 'BTCUSDT',
    side: 'buy',
    price: '64000',
    qty: '0.2',
    fee: '0.00012',
    role: 'taker',
    created_at: 1700000000,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetOrderHistory.mockResolvedValue(MOCK_ORDERS);
  mockGetTradeHistory.mockResolvedValue(MOCK_TRADES);
  mockGetFundingHistory.mockResolvedValue([]);
});

describe('HistoryPage', () => {
  it('renders title, tabs, and toolbar', async () => {
    await act(async () => { render(<HistoryPage />); });
    expect(screen.getByText('History')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Orders' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Trades' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Funding' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Filter' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Export CSV' })).toBeInTheDocument();
  });

  it('orders tab loads and displays order history', async () => {
    await act(async () => { render(<HistoryPage />); });

    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
    expect(mockGetOrderHistory).toHaveBeenCalledWith(undefined, undefined);
  });

  it('trades tab loads trade history from API', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<HistoryPage />); });

    await user.click(screen.getByRole('tab', { name: 'Trades' }));

    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('64000')).toBeInTheDocument();
    expect(screen.getByText('0.00012')).toBeInTheDocument();
    expect(mockGetTradeHistory).toHaveBeenCalledWith(undefined, undefined);
  });

  it('switching tabs changes the API call made', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<HistoryPage />); });

    // Default: orders
    expect(mockGetOrderHistory).toHaveBeenCalledTimes(1);
    expect(mockGetTradeHistory).not.toHaveBeenCalled();

    await user.click(screen.getByRole('tab', { name: 'Trades' }));
    expect(mockGetTradeHistory).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole('tab', { name: 'Funding' }));
    expect(mockGetFundingHistory).toHaveBeenCalledTimes(1);
  });

  it('Export CSV button is present and clickable', async () => {
    await act(async () => { render(<HistoryPage />); });
    await screen.findByText('BTCUSDT');

    // Just verify the button is present and does not throw
    const exportBtn = screen.getByRole('button', { name: 'Export CSV' });
    expect(exportBtn).toBeInTheDocument();
    expect(exportBtn).not.toBeDisabled();
  });

  it('shows empty state text when API returns no data', async () => {
    mockGetOrderHistory.mockResolvedValueOnce([]);
    await act(async () => { render(<HistoryPage />); });

    expect(await screen.findByText('No orders')).toBeInTheDocument();
  });

  it('shows empty trades state when trades API returns empty', async () => {
    mockGetTradeHistory.mockResolvedValueOnce([]);
    const user = userEvent.setup();
    await act(async () => { render(<HistoryPage />); });

    await user.click(screen.getByRole('tab', { name: 'Trades' }));

    expect(await screen.findByText('No trades')).toBeInTheDocument();
  });
});
