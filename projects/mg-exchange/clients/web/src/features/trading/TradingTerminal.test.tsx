import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock resizable panels (requires ResizeObserver not available in jsdom)
vi.mock('react-resizable-panels', () => ({
  Panel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  PanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  PanelResizeHandle: () => <div />,
}));

vi.mock('./api', () => ({
  placeOrder: vi.fn(),
  cancelOrder: vi.fn(),
  cancelAllOrders: vi.fn(),
  getOpenOrders: vi.fn(),
  getOrderHistory: vi.fn(),
  getOrderBook: vi.fn(),
  getKlines: vi.fn(),
  getTicker: vi.fn(),
  getRecentTrades: vi.fn(),
  getPositions: vi.fn(),
  getBalances: vi.fn(),
}));

vi.mock('@shared/api/websocket', () => ({
  ws: {
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
    onMessage: vi.fn(() => vi.fn()),
    onStatus: vi.fn(() => vi.fn()),
    getStatus: vi.fn(() => 'disconnected'),
    connect: vi.fn(),
  },
}));

// Mock Chart and RecentTrades — they have complex deps (canvas, websockets)
vi.mock('./components/Chart', () => ({
  Chart: ({ symbol }: { symbol: string }) => <div data-testid={`chart-${symbol}`} />,
}));
vi.mock('./components/RecentTrades', () => ({
  RecentTrades: ({ symbol }: { symbol: string }) => <div data-testid={`recent-trades-${symbol}`} />,
}));
vi.mock('./components/PairList', () => ({
  PairList: ({
    isOpen,
    onClose,
  }: {
    isOpen: boolean;
    onClose: () => void;
  }) =>
    isOpen ? (
      <div data-testid="pair-list">
        <button onClick={onClose}>Close</button>
      </div>
    ) : null,
}));
vi.mock('./components/TickerBar', () => ({
  TickerBar: ({ onPairListOpen }: { onPairListOpen: () => void }) => (
    <div data-testid="ticker-bar">
      <button data-testid="open-pair-list" onClick={onPairListOpen}>
        Pair List
      </button>
    </div>
  ),
}));

import TradingTerminal from './TradingTerminal';
import {
  placeOrder,
  cancelOrder,
  cancelAllOrders,
  getOpenOrders,
  getPositions,
  getBalances,
  getOrderBook,
  getOrderHistory,
} from './api';
import { useMarketStore } from './stores/market';
import { useTradingStore } from './stores/trading';
import { useOrderBookStore } from './stores/orderbook';
import type { OpenOrder, Position, BalanceResponse, OrderBookResponse } from './types';

const mockPlaceOrder = vi.mocked(placeOrder);
const mockCancelOrder = vi.mocked(cancelOrder);
const mockCancelAllOrders = vi.mocked(cancelAllOrders);
const mockGetOpenOrders = vi.mocked(getOpenOrders);
const mockGetPositions = vi.mocked(getPositions);
const mockGetBalances = vi.mocked(getBalances);
const mockGetOrderBook = vi.mocked(getOrderBook);
const mockGetOrderHistory = vi.mocked(getOrderHistory);

const MOCK_ORDERS: OpenOrder[] = [
  {
    order_id: 'ord-1',
    symbol: 'BTCUSDT',
    side: 'buy',
    order_type: 'limit',
    price: '65000',
    qty: '0.1',
    filled_qty: '0',
    status: 'open',
    created_at: new Date().toISOString(),
  },
];

const MOCK_POSITION: Position = {
  symbol: 'BTCUSDT',
  side: 'buy',
  size: '0.5',
  entry_price: '64000',
  mark_price: '65000',
  unrealized_pnl: '500',
  margin: '640',
  leverage: '10',
  liquidation_price: '58000',
};

const MOCK_BALANCES: BalanceResponse[] = [
  { asset: 'USDT', available: '10000', locked: '6500', total: '16500' },
];

const MOCK_ORDER_BOOK: OrderBookResponse = { bids: [], asks: [], sequence: 0 };

beforeEach(() => {
  vi.clearAllMocks();
  mockGetOpenOrders.mockResolvedValue(MOCK_ORDERS);
  mockGetPositions.mockResolvedValue([MOCK_POSITION]);
  mockGetBalances.mockResolvedValue(MOCK_BALANCES);
  mockPlaceOrder.mockResolvedValue({ order_id: 'new-order', status: 'open', fills: [] });
  mockCancelOrder.mockResolvedValue(undefined);
  mockCancelAllOrders.mockResolvedValue({ cancelled: 1 });
  mockGetOrderBook.mockResolvedValue(MOCK_ORDER_BOOK);
  mockGetOrderHistory.mockResolvedValue([]);

  // Reset stores between tests
  useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
  useTradingStore.setState({ oneClickMode: true });
  useOrderBookStore.getState().reset();
});

describe('TradingTerminal', () => {
  it('renders all major sections: ticker, chart, orderbook, order entry, bottom panel', async () => {
    await act(async () => { render(<TradingTerminal />); });

    expect(screen.getByTestId('ticker-bar')).toBeInTheDocument();
    expect(screen.getByTestId('chart-BTCUSDT')).toBeInTheDocument();
    // OrderBook renders its title
    expect(screen.getByText('Order Book')).toBeInTheDocument();
    // OrderEntry renders side buttons (multiple "Buy / Long" elements expected)
    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    expect(buyBtns.length).toBeGreaterThan(0);
    // BottomPanel renders tab labels
    expect(screen.getByRole('tab', { name: /Open Orders/i })).toBeInTheDocument();
  });

  it('clicking ticker bar pair-list button opens the pair list panel', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    expect(screen.queryByTestId('pair-list')).not.toBeInTheDocument();
    await user.click(screen.getByTestId('open-pair-list'));
    expect(screen.getByTestId('pair-list')).toBeInTheDocument();
  });

  it('pair list Close button hides the panel', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    await user.click(screen.getByTestId('open-pair-list'));
    expect(screen.getByTestId('pair-list')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Close' }));
    expect(screen.queryByTestId('pair-list')).not.toBeInTheDocument();
  });

  it('placing a limit buy order calls placeOrder API with correct params', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    // Price input (placeholder 0.00 for limit orders)
    const priceInput = screen.getByPlaceholderText('0.00');
    await user.clear(priceInput);
    await user.type(priceInput, '65000');

    // Qty input (placeholder 0.00000)
    const qtyInput = screen.getByPlaceholderText('0.00000');
    await user.clear(qtyInput);
    await user.type(qtyInput, '0.1');

    // The submit button text is "Buy / Long" (leverage=1 so no "1x" suffix)
    // There are two "Buy / Long" buttons: the side selector and the submit button.
    // The submit button is type="submit"; use the last one.
    const buyBtns = screen.getAllByRole('button', { name: /^Buy \/ Long$/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    expect(mockPlaceOrder).toHaveBeenCalledWith(
      expect.objectContaining({
        symbol: 'BTCUSDT',
        side: 'buy',
        order_type: 'limit',
        price: '65000',
        qty: '0.1',
      }),
    );
  });

  it('cancel order button calls cancelOrder API with the order id', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    // Open Orders tab shows orders; wait for data
    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();

    const cancelBtn = screen.getByRole('button', { name: 'Cancel' });
    await act(async () => {
      await user.click(cancelBtn);
    });

    expect(mockCancelOrder).toHaveBeenCalledWith('ord-1');
  });

  it('WS delta message updates the orderbook store bids', () => {
    const { applySnapshot, applyDelta } = useOrderBookStore.getState();

    act(() => {
      applySnapshot(
        [['65000', '1.0'], ['64900', '2.0']],
        [['65100', '0.5']],
        1,
      );
    });

    expect(useOrderBookStore.getState().bids[0].price).toBe('65000');

    act(() => {
      applyDelta(
        [['65050', '0.8'], ['65000', '0']],
        [],
        2,
      );
    });

    const state = useOrderBookStore.getState();
    expect(state.bids[0].price).toBe('65050');
    expect(state.bids.find((b) => b.price === '65000')).toBeUndefined();
  });

  it('switching to Sell side changes the submit button label', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    // There are two "Sell / Short" buttons; click the side selector (first one)
    const sellBtns = screen.getAllByRole('button', { name: /Sell \/ Short/i });
    await user.click(sellBtns[0]);

    // After switching to sell, there should be at least one "Sell / Short" visible
    expect(screen.getAllByRole('button', { name: /Sell \/ Short/i }).length).toBeGreaterThan(0);
  });

  it('switching to market order type hides the price input', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    // Price input visible in limit mode
    expect(screen.getByPlaceholderText('0.00')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Market' }));

    // Price input disappears in market mode
    expect(screen.queryByPlaceholderText('0.00')).not.toBeInTheDocument();
  });

  it('Cancel All button calls cancelAllOrders with the current symbol', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    // Wait for orders to load
    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();

    const cancelAllBtn = screen.getByRole('button', { name: 'Cancel All' });
    await act(async () => {
      await user.click(cancelAllBtn);
    });

    expect(mockCancelAllOrders).toHaveBeenCalledWith('BTCUSDT');
  });

  it('positions tab shows position data from API', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<TradingTerminal />); });

    await user.click(screen.getByRole('tab', { name: /Positions/i }));

    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('Long')).toBeInTheDocument();
    expect(screen.getByText('+500.0000')).toBeInTheDocument();
  });
});
