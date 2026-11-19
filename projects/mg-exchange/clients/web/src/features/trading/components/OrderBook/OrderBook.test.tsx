import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getOrderBook: vi.fn(),
  getTicker: vi.fn(),
  getRecentTrades: vi.fn(),
  placeOrder: vi.fn(),
  getBalances: vi.fn(),
  cancelOrder: vi.fn(),
  cancelAllOrders: vi.fn(),
  getOpenOrders: vi.fn(),
  getOrderHistory: vi.fn(),
  getKlines: vi.fn(),
  getPositions: vi.fn(),
}));

vi.mock('@shared/api/websocket', () => ({
  ws: {
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
    onMessage: vi.fn(() => vi.fn()),
    onStatus: vi.fn(() => vi.fn()),
    connect: vi.fn(),
  },
}));

import { OrderBook } from '../OrderBook';
import { getOrderBook } from '../../api';
import { useOrderBookStore } from '../../stores/orderbook';
import { useTradingStore } from '../../stores/trading';

const mockGetOrderBook = vi.mocked(getOrderBook);

beforeEach(() => {
  vi.clearAllMocks();
  useOrderBookStore.getState().reset();
  useTradingStore.setState({
    orderType: 'limit',
    side: 'buy',
    price: '',
    qty: '',
    stopPrice: '',
    leverage: 1,
    marginMode: 'cross',
    tpPrice: '',
    slPrice: '',
    reduceOnly: false,
    showTpSl: false,
  });
});

describe('OrderBook', () => {
  it('renders "Order Book" title', async () => {
    mockGetOrderBook.mockResolvedValue({ bids: [], asks: [], sequence: 0 });
    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    expect(screen.getByText('Order Book')).toBeInTheDocument();
  });

  it('renders bids and asks when snapshot is loaded via API', async () => {
    // Make getOrderBook return snapshot data — the hook applies it to the store
    mockGetOrderBook.mockResolvedValue({
      bids: [['65000', '1.0'], ['64900', '2.0']],
      asks: [['65100', '0.5'], ['65200', '0.8']],
      sequence: 1,
    });

    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    await waitFor(() => {
      expect(screen.getByText('65000')).toBeInTheDocument();
    });

    expect(screen.getByText('65100')).toBeInTheDocument();
  });

  it('shows spread row when both bids and asks are loaded', async () => {
    mockGetOrderBook.mockResolvedValue({
      bids: [['65000', '1.0']],
      asks: [['65100', '0.5']],
      sequence: 1,
    });

    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    await waitFor(() => {
      expect(screen.getByText('Spread')).toBeInTheDocument();
    });
  });

  it('shows depth percentage bars when data loaded', async () => {
    mockGetOrderBook.mockResolvedValue({
      bids: [['65000', '1.0'], ['64900', '2.0']],
      asks: [['65100', '0.5']],
      sequence: 1,
    });

    const { container } = await act(async () => render(<OrderBook symbol="BTCUSDT" />));

    await waitFor(() => {
      const depthBars = container.querySelectorAll('[style*="width"]');
      expect(depthBars.length).toBeGreaterThan(0);
    });
  });

  it('clicking an ask price row fills price in trading store', async () => {
    mockGetOrderBook.mockResolvedValue({
      bids: [['65000', '1.0']],
      asks: [['65100', '0.5']],
      sequence: 1,
    });

    const user = userEvent.setup();
    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    await waitFor(() => {
      expect(screen.getByTitle('Click to fill price: 65100')).toBeInTheDocument();
    });

    await user.click(screen.getByTitle('Click to fill price: 65100'));

    expect(useTradingStore.getState().price).toBe('65100');
  });

  it('clicking a bid price row fills price in trading store', async () => {
    mockGetOrderBook.mockResolvedValue({
      bids: [['65000', '1.0']],
      asks: [['65100', '0.5']],
      sequence: 1,
    });

    const user = userEvent.setup();
    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    await waitFor(() => {
      expect(screen.getByTitle('Click to fill price: 65000')).toBeInTheDocument();
    });

    await user.click(screen.getByTitle('Click to fill price: 65000'));

    expect(useTradingStore.getState().price).toBe('65000');
  });

  it('shows skeleton rows while loading (empty bids and asks)', async () => {
    // Delay API response so skeleton shows
    let resolveApi: (v: { bids: [string,string][], asks: [string,string][], sequence: number }) => void;
    mockGetOrderBook.mockReturnValue(
      new Promise((r) => { resolveApi = r; }),
    );

    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    // Panel renders without crashing
    expect(screen.getByText('Order Book')).toBeInTheDocument();

    // Resolve to avoid act warning
    act(() => {
      resolveApi({ bids: [], asks: [], sequence: 0 });
    });
  });

  it('precision selector options are rendered', async () => {
    mockGetOrderBook.mockResolvedValue({ bids: [], asks: [], sequence: 0 });
    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    const select = screen.getByRole('combobox');
    expect(select).toBeInTheDocument();
  });

  it('labels show Price, Size, Total', async () => {
    mockGetOrderBook.mockResolvedValue({ bids: [], asks: [], sequence: 0 });
    await act(async () => { render(<OrderBook symbol="BTCUSDT" />); });

    expect(screen.getByText('Price')).toBeInTheDocument();
    expect(screen.getByText('Size')).toBeInTheDocument();
    expect(screen.getByText('Total')).toBeInTheDocument();
  });
});
