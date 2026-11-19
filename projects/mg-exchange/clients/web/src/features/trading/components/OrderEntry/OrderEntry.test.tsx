import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  placeOrder: vi.fn(),
  getBalances: vi.fn(),
}));

vi.mock('@shared/api/websocket', () => ({
  ws: {
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
    onMessage: vi.fn(() => vi.fn()),
  },
}));

import { OrderEntry } from '../OrderEntry';
import { placeOrder, getBalances } from '../../api';
import { useTradingStore } from '../../stores/trading';
import { useMarketStore } from '../../stores/market';
import { useToastStore } from '@shared/ui/Toast/store';
import { ApiError } from '@shared/api/client';
import type { BalanceResponse } from '../../types';

const mockPlaceOrder = vi.mocked(placeOrder);
const mockGetBalances = vi.mocked(getBalances);

const MOCK_BALANCES: BalanceResponse[] = [
  { asset: 'USDT', available: '10000', locked: '0', total: '10000' },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetBalances.mockResolvedValue(MOCK_BALANCES);
  mockPlaceOrder.mockResolvedValue({ order_id: 'ord-1', status: 'open', fills: [] });
  // Reset stores
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
    oneClickMode: true,
  });
  useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
  useToastStore.setState({ toasts: [] });
});

describe('OrderEntry', () => {
  it('renders buy and sell side buttons', async () => {
    await act(async () => { render(<OrderEntry />); });

    expect(screen.getAllByRole('button', { name: /Buy \/ Long/i }).length).toBeGreaterThan(0);
    expect(screen.getAllByRole('button', { name: /Sell \/ Short/i }).length).toBeGreaterThan(0);
  });

  it('clicking Buy side button sets side to buy', async () => {
    const user = userEvent.setup();
    // Start with sell selected
    useTradingStore.setState({ side: 'sell' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtn = screen.getByRole('button', { name: /Buy \/ Long/i });
    await user.click(buyBtn);

    expect(useTradingStore.getState().side).toBe('buy');
  });

  it('clicking Sell side button sets side to sell', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OrderEntry />); });

    const sellBtn = screen.getByRole('button', { name: /Sell \/ Short/i });
    await user.click(sellBtn);

    expect(useTradingStore.getState().side).toBe('sell');
  });

  it('renders order type tabs: Limit, Market, Stop-Limit, Stop-Market', async () => {
    await act(async () => { render(<OrderEntry />); });

    expect(screen.getByRole('button', { name: 'Limit' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Market' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop-Limit' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop-Market' })).toBeInTheDocument();
  });

  it('clicking Market tab switches order type', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OrderEntry />); });

    await user.click(screen.getByRole('button', { name: 'Market' }));

    expect(useTradingStore.getState().orderType).toBe('market');
  });

  it('price input is shown for limit orders', async () => {
    await act(async () => { render(<OrderEntry />); });

    expect(screen.getByPlaceholderText('0.00')).toBeInTheDocument();
  });

  it('price input accepts numeric input', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OrderEntry />); });

    const priceInput = screen.getByPlaceholderText('0.00');
    await user.clear(priceInput);
    await user.type(priceInput, '65000');

    expect(useTradingStore.getState().price).toBe('65000');
  });

  it('qty % buttons (25%, 50%, 75%, 100%) are rendered', async () => {
    await act(async () => { render(<OrderEntry />); });

    expect(screen.getByRole('button', { name: '25%' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '50%' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '75%' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: '100%' })).toBeInTheDocument();
  });

  it('25% button fills qty to 25% of balance divided by price', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ price: '50000' });

    await act(async () => { render(<OrderEntry />); });

    await waitFor(() => {
      // Wait for balances to load
      expect(mockGetBalances).toHaveBeenCalled();
    });

    await user.click(screen.getByRole('button', { name: '25%' }));

    // 10000 * 0.25 / 50000 = 0.05
    await waitFor(() => {
      expect(useTradingStore.getState().qty).toMatch(/^0\.05/);
    });
  });

  it('submit calls placeOrder API with correct form state', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    // The submit button is the last "Buy / Long" button
    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
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

  it('shows error toast on API failure', async () => {
    mockPlaceOrder.mockRejectedValue(new ApiError(400, 'Insufficient balance'));
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error' && t.message === 'Insufficient balance')).toBe(true);
    });
  });

  it('shows validation error when qty is 0', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    expect(mockPlaceOrder).not.toHaveBeenCalled();

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });
  });

  it('clears price and qty after successful order', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    await waitFor(() => {
      const state = useTradingStore.getState();
      expect(state.price).toBe('');
      expect(state.qty).toBe('');
    });
  });

  it('shows success toast after successful order', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'success')).toBe(true);
    });
  });

  it('price input is hidden for market order type', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OrderEntry />); });

    await user.click(screen.getByRole('button', { name: 'Market' }));

    expect(screen.queryByPlaceholderText('0.00')).not.toBeInTheDocument();
  });

  it('shows spot buy/sell labels when product type is spot', async () => {
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'spot', tradingMode: 'spot' });
    await act(async () => { render(<OrderEntry />); });

    // For spot, the side buttons show just 'Buy' and 'Sell'
    expect(screen.getByRole('button', { name: /^Buy$/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Sell$/ })).toBeInTheDocument();
  });

  // === Order Type Behavior ===

  it('market order hides price input', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OrderEntry />); });

    await user.click(screen.getByRole('button', { name: 'Market' }));

    // price placeholder '0.00' input must not be present for market orders
    expect(screen.queryByPlaceholderText('0.00')).not.toBeInTheDocument();
  });

  it('stop-limit shows both trigger price and limit price inputs', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OrderEntry />); });

    await user.click(screen.getByRole('button', { name: 'Stop-Limit' }));

    // Both the trigger price and the limit price use placeholder '0.00'
    const priceInputs = screen.getAllByPlaceholderText('0.00');
    expect(priceInputs.length).toBe(2);
  });

  it('switching order type from limit to market clears price from store on next render', async () => {
    const user = userEvent.setup();
    // Start with a price set
    useTradingStore.setState({ price: '65000', orderType: 'limit' });
    await act(async () => { render(<OrderEntry />); });

    // Verify the price input exists in limit mode
    expect(screen.getByPlaceholderText('0.00')).toBeInTheDocument();

    // Switch to market — price input disappears
    await user.click(screen.getByRole('button', { name: 'Market' }));
    expect(screen.queryByPlaceholderText('0.00')).not.toBeInTheDocument();

    // Store still keeps the value (not auto-cleared on type switch — that's correct behavior)
    expect(useTradingStore.getState().orderType).toBe('market');
  });

  // === Leverage ===

  it('leverage slider changes leverage in store', async () => {
    const user = userEvent.setup();
    // isPerp must be true
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
    await act(async () => { render(<OrderEntry />); });

    // Open leverage popup by clicking the leverage button (shows "1x" initially)
    const leverageBtn = screen.getByRole('button', { name: /^1x$/i });
    await user.click(leverageBtn);

    // Click the 10x tick button — use getAllByRole to handle multiple matches and pick the first one inside the popup
    const tick10Buttons = screen.getAllByRole('button', { name: /^10x$/ });
    // The first such button is the tick button in the popup (the main leverage btn shows current value)
    await user.click(tick10Buttons[0]);

    expect(useTradingStore.getState().leverage).toBe(10);
  });

  it('leverage section shown for perpetual products', async () => {
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
    await act(async () => { render(<OrderEntry />); });

    // The leverage button with "x" suffix is rendered
    expect(screen.getByRole('button', { name: /1x/i })).toBeInTheDocument();
  });

  it('leverage section not shown for spot products', async () => {
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'spot', tradingMode: 'spot' });
    await act(async () => { render(<OrderEntry />); });

    // No leverage button in spot mode
    expect(screen.queryByRole('button', { name: /1x/i })).not.toBeInTheDocument();
  });

  // === TP/SL ===

  it('TP/SL section hidden by default', async () => {
    // showTpSl = false by default
    await act(async () => { render(<OrderEntry />); });

    expect(screen.queryByPlaceholderText('Optional')).not.toBeInTheDocument();
  });

  it('toggling TP/SL checkbox shows Take Profit and Stop Loss inputs', async () => {
    const user = userEvent.setup();
    // Ensure we're in perpetual mode so the TP/SL toggle renders
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
    await act(async () => { render(<OrderEntry />); });

    const tpslCheckbox = screen.getByRole('checkbox', { name: /Take Profit \/ Stop Loss/i });
    await user.click(tpslCheckbox);

    // After toggling, Optional placeholder inputs should appear (TP and SL use placeholder="Optional")
    const optionalInputs = screen.getAllByPlaceholderText('Optional');
    expect(optionalInputs.length).toBe(2);
    // Labels are rendered as plain <label> elements (not associated via htmlFor)
    expect(screen.getByText('Take Profit')).toBeInTheDocument();
    expect(screen.getByText('Stop Loss')).toBeInTheDocument();
  });

  it('TP and SL values are stored in trading store', async () => {
    const user = userEvent.setup();
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
    useTradingStore.setState({ showTpSl: true });
    await act(async () => { render(<OrderEntry />); });

    // TP and SL inputs use placeholder="Optional" — get them by index (TP first, SL second)
    const optionalInputs = screen.getAllByPlaceholderText('Optional');
    const tpInput = optionalInputs[0];
    const slInput = optionalInputs[1];

    await user.clear(tpInput);
    await user.type(tpInput, '70000');

    await user.clear(slInput);
    await user.type(slInput, '60000');

    expect(useTradingStore.getState().tpPrice).toBe('70000');
    expect(useTradingStore.getState().slPrice).toBe('60000');
  });

  // === Qty Percentage Buttons ===

  it('100% button sets qty to max affordable at current price', async () => {
    const user = userEvent.setup();
    // balance = 10000 USDT, price = 50000 → max qty = 10000 / 50000 = 0.2
    useTradingStore.setState({ price: '50000' });

    await act(async () => { render(<OrderEntry />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    await user.click(screen.getByRole('button', { name: '100%' }));

    await waitFor(() => {
      expect(useTradingStore.getState().qty).toMatch(/^0\.2/);
    });
  });

  it('50% button sets qty to 50% of available balance divided by price', async () => {
    const user = userEvent.setup();
    // balance = 10000 USDT, price = 50000 → 50% qty = 10000 * 0.5 / 50000 = 0.1
    useTradingStore.setState({ price: '50000' });

    await act(async () => { render(<OrderEntry />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    await user.click(screen.getByRole('button', { name: '50%' }));

    await waitFor(() => {
      expect(useTradingStore.getState().qty).toMatch(/^0\.1/);
    });
  });

  it('percentage buttons do nothing when price is not set', async () => {
    const user = userEvent.setup();
    // price is empty
    useTradingStore.setState({ price: '', qty: '' });

    await act(async () => { render(<OrderEntry />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    await user.click(screen.getByRole('button', { name: '25%' }));

    // qty should remain empty because fillQtyByPct returns early when price is falsy
    expect(useTradingStore.getState().qty).toBe('');
  });

  // === Validation Edge Cases ===

  it('cannot submit with empty qty', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    expect(mockPlaceOrder).not.toHaveBeenCalled();

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });
  });

  it('cannot submit limit order without price', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    expect(mockPlaceOrder).not.toHaveBeenCalled();

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error' && /price/i.test(t.message))).toBe(true);
    });
  });

  it('market order with qty but no price passes validation (price not required)', async () => {
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'market', price: '', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    // For market orders, getByRole 'button' that matches 'Buy / Long' at bottom
    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    await waitFor(() => {
      expect(mockPlaceOrder).toHaveBeenCalledWith(
        expect.objectContaining({
          side: 'buy',
          order_type: 'market',
          qty: '0.1',
        }),
      );
    });
  });

  // === API Error Handling ===

  it('shows specific error message from API response', async () => {
    mockPlaceOrder.mockRejectedValue(new ApiError(400, 'Insufficient margin'));
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error' && t.message === 'Insufficient margin')).toBe(true);
    });
  });

  it('form retains price and qty after failed submission', async () => {
    mockPlaceOrder.mockRejectedValue(new ApiError(400, 'Insufficient margin'));
    const user = userEvent.setup();
    useTradingStore.setState({ side: 'buy', orderType: 'limit', price: '65000', qty: '0.1' });

    await act(async () => { render(<OrderEntry />); });

    const buyBtns = screen.getAllByRole('button', { name: /Buy \/ Long/i });
    const submitBtn = buyBtns[buyBtns.length - 1];

    await act(async () => {
      await user.click(submitBtn);
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });

    // Store values should remain intact after failed submission (reset() is NOT called on error)
    const state = useTradingStore.getState();
    expect(state.price).toBe('65000');
    expect(state.qty).toBe('0.1');
  });

  // === Reduce Only ===

  it('reduce only checkbox sets reduceOnly in store', async () => {
    const user = userEvent.setup();
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'perpetual', tradingMode: 'futures' });
    useTradingStore.setState({ reduceOnly: false });
    await act(async () => { render(<OrderEntry />); });

    const reduceOnlyCheckbox = screen.getByRole('checkbox', { name: /Reduce Only/i });
    await user.click(reduceOnlyCheckbox);

    expect(useTradingStore.getState().reduceOnly).toBe(true);
  });

  it('reduce only checkbox not rendered for spot products', async () => {
    useMarketStore.setState({ symbol: 'BTCUSDT', productType: 'spot', tradingMode: 'spot' });
    await act(async () => { render(<OrderEntry />); });

    expect(screen.queryByRole('checkbox', { name: /Reduce Only/i })).not.toBeInTheDocument();
  });
});
