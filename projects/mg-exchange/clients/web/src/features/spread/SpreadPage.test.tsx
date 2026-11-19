import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getSpreadInstruments: vi.fn(),
  placeSpreadOrder: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import SpreadPage from './SpreadPage';
import { getSpreadInstruments, placeSpreadOrder } from './api';
import type { SpreadInstrument } from './types';

const mockGetSpreadInstruments = vi.mocked(getSpreadInstruments);
const mockPlaceSpreadOrder = vi.mocked(placeSpreadOrder);

const MOCK_INSTRUMENTS: SpreadInstrument[] = [
  {
    id: 'spread-1',
    name: 'BTC Q1-Q2',
    near_leg: 'BTCUSDT-240329',
    far_leg: 'BTCUSDT-240628',
    spread_value: '250.00',
    bid: '248.50',
    ask: '251.50',
    last: '250.00',
    volume_24h: '1500000',
  },
  {
    id: 'spread-2',
    name: 'ETH Q1-Q2',
    near_leg: 'ETHUSDT-240329',
    far_leg: 'ETHUSDT-240628',
    spread_value: '15.00',
    bid: '14.80',
    ask: '15.20',
    last: '15.00',
    volume_24h: '800000',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetSpreadInstruments.mockResolvedValue(MOCK_INSTRUMENTS);
  mockPlaceSpreadOrder.mockResolvedValue({ order_id: 'ord-1', status: 'open' });
});

describe('SpreadPage', () => {
  it('renders instruments list', async () => {
    render(<SpreadPage />);

    expect(await screen.findByText('BTC Q1-Q2')).toBeInTheDocument();
    expect(screen.getByText('ETH Q1-Q2')).toBeInTheDocument();
  });

  it('selecting instrument updates order form header', async () => {
    render(<SpreadPage />);

    await screen.findByText('BTC Q1-Q2');

    // Click the first row to select the instrument
    await userEvent.click(screen.getByText('BTC Q1-Q2'));

    // Form panel title should update to show the selected instrument name
    await waitFor(() => {
      expect(screen.getByText('Order: BTC Q1-Q2')).toBeInTheDocument();
    });
  });

  it('order form validates quantity > 0', async () => {
    render(<SpreadPage />);

    await screen.findByText('BTC Q1-Q2');
    await userEvent.click(screen.getByText('BTC Q1-Q2'));

    // Quantity input is now shown
    const qtyInput = await screen.findByLabelText('Quantity');
    await userEvent.type(qtyInput, '0');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Place Spread Order' }));
    });

    expect(mockPlaceSpreadOrder).not.toHaveBeenCalled();
  });

  it('submitting a valid order calls placeSpreadOrder', async () => {
    render(<SpreadPage />);

    await screen.findByText('BTC Q1-Q2');
    await userEvent.click(screen.getByText('BTC Q1-Q2'));

    const qtyInput = await screen.findByLabelText('Quantity');
    await userEvent.type(qtyInput, '5');

    // Default order type is limit — fill price
    const priceInput = screen.getByLabelText('Price');
    await userEvent.type(priceInput, '250');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Place Spread Order' }));
    });

    expect(mockPlaceSpreadOrder).toHaveBeenCalledWith({
      instrument_id: 'spread-1',
      side: 'buy',
      qty: '5',
      order_type: 'limit',
      price: '250',
    });
  });

  it('getSpreadInstruments is called on mount', async () => {
    render(<SpreadPage />);
    await screen.findByText('BTC Q1-Q2');
    expect(mockGetSpreadInstruments).toHaveBeenCalledTimes(1);
  });
});
