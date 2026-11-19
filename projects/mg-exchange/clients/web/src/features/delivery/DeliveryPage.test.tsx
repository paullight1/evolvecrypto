import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getDeliveryInstruments: vi.fn(),
  placeDeliveryOrder: vi.fn(),
  getDeliveryPositions: vi.fn(),
  getDeliveryPrice: vi.fn(),
  getSettlementPrices: vi.fn(),
}));

import DeliveryPage from './DeliveryPage';
import {
  getDeliveryInstruments,
  placeDeliveryOrder,
  getDeliveryPositions,
  getSettlementPrices,
} from './api';
import type { DeliveryInstrument, DeliveryPosition, SettlementRecord } from './types';

const mockGetDeliveryInstruments = vi.mocked(getDeliveryInstruments);
const mockPlaceDeliveryOrder = vi.mocked(placeDeliveryOrder);
const mockGetDeliveryPositions = vi.mocked(getDeliveryPositions);
const mockGetSettlementPrices = vi.mocked(getSettlementPrices);

const MOCK_INSTRUMENTS: DeliveryInstrument[] = [
  {
    symbol: 'BTCUSD-240329',
    underlying: 'BTC',
    delivery_date: '2024-03-29T00:00:00Z',
    days_remaining: 15,
    last_price: '65000',
    mark_price: '65100',
    basis: '0.15',
    oi: '1250',
  },
  {
    symbol: 'ETHUSD-240329',
    underlying: 'ETH',
    delivery_date: '2024-03-29T00:00:00Z',
    days_remaining: 15,
    last_price: '3200',
    mark_price: '3210',
    basis: '0.31',
    oi: '5000',
  },
];

const MOCK_POSITIONS: DeliveryPosition[] = [
  {
    symbol: 'BTCUSD-240329',
    side: 'buy',
    size: '1',
    entry_price: '64000',
    mark_price: '65000',
    unrealized_pnl: '1000',
    margin: '6400',
    delivery_date: '2024-03-29T00:00:00Z',
  },
];

const MOCK_SETTLEMENTS: SettlementRecord[] = [
  { symbol: 'BTCUSD-231229', settlement_price: '42000', settled_at: 1703808000, pnl: '500' },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetDeliveryInstruments.mockResolvedValue(MOCK_INSTRUMENTS);
  mockGetDeliveryPositions.mockResolvedValue(MOCK_POSITIONS);
  mockGetSettlementPrices.mockResolvedValue(MOCK_SETTLEMENTS);
  mockPlaceDeliveryOrder.mockResolvedValue({ order_id: 'del-ord-1', status: 'open' });
});

describe('DeliveryPage', () => {
  it('renders title and tabs', async () => {
    await act(async () => { render(<DeliveryPage />); });
    expect(screen.getByText('Delivery Futures')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Instruments' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Positions' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Settlement History' })).toBeInTheDocument();
  });

  it('renders instrument list with symbols', async () => {
    await act(async () => { render(<DeliveryPage />); });
    expect(await screen.findByText('BTCUSD-240329')).toBeInTheDocument();
    expect(screen.getByText('ETHUSD-240329')).toBeInTheDocument();
    expect(mockGetDeliveryInstruments).toHaveBeenCalledTimes(1);
  });

  it('instruments are available in the order entry selector after loading', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryPage />); });

    // Wait for instruments to load in the instrument list table
    await screen.findByText('BTCUSD-240329');

    // The order entry select should have the instrument as an option
    const instrSelect = await screen.findByLabelText('Instrument') as HTMLSelectElement;
    expect(instrSelect).toBeInTheDocument();
    // Select the instrument
    await user.selectOptions(instrSelect, 'BTCUSD-240329');
    expect(instrSelect.value).toBe('BTCUSD-240329');
  });

  it('placing a delivery order calls placeDeliveryOrder API with correct params', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryPage />); });

    await screen.findByText('BTCUSD-240329');

    // Select instrument in order entry
    const instrSelect = await screen.findByLabelText('Instrument') as HTMLSelectElement;
    await user.selectOptions(instrSelect, 'BTCUSD-240329');

    const qtyInput = screen.getByLabelText('Quantity');
    await user.clear(qtyInput);
    await user.type(qtyInput, '2');

    const priceInput = screen.getByLabelText('Price');
    await user.clear(priceInput);
    await user.type(priceInput, '65000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Buy Long' }));
    });

    expect(mockPlaceDeliveryOrder).toHaveBeenCalledWith(
      expect.objectContaining({
        symbol: 'BTCUSD-240329',
        side: 'buy',
        order_type: 'limit',
        qty: '2',
        price: '65000',
      }),
    );
  });

  it('positions tab shows delivery positions', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryPage />); });

    await user.click(screen.getByRole('tab', { name: 'Positions' }));

    expect(await screen.findByText('BTCUSD-240329')).toBeInTheDocument();
    expect(mockGetDeliveryPositions).toHaveBeenCalledTimes(1);
  });

  it('settlement history tab loads and shows settlement records', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DeliveryPage />); });

    await user.click(screen.getByRole('tab', { name: 'Settlement History' }));

    expect(await screen.findByText('BTCUSD-231229')).toBeInTheDocument();
    expect(screen.getByText('42000')).toBeInTheDocument();
    expect(mockGetSettlementPrices).toHaveBeenCalledWith(50);
  });
});
