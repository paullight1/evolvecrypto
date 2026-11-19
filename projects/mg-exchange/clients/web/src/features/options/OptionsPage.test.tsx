import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getOptionInstruments: vi.fn(),
  getOptionTickers: vi.fn(),
  getOptionPositions: vi.fn(),
  getOptionGreeks: vi.fn(),
  placeOptionOrder: vi.fn(),
}));

import OptionsPage from './OptionsPage';
import {
  getOptionInstruments,
  getOptionTickers,
  getOptionPositions,
  getOptionGreeks,
  placeOptionOrder,
} from './api';
import type { OptionInstrument, OptionTicker, OptionPosition, OptionGreeks } from './types';

const mockGetOptionInstruments = vi.mocked(getOptionInstruments);
const mockGetOptionTickers = vi.mocked(getOptionTickers);
const mockGetOptionPositions = vi.mocked(getOptionPositions);
const mockGetOptionGreeks = vi.mocked(getOptionGreeks);
const mockPlaceOptionOrder = vi.mocked(placeOptionOrder);

const MOCK_INSTRUMENTS: OptionInstrument[] = [
  { symbol: 'BTC-20240329-60000-C', underlying: 'BTC', strike: '60000', expiry: '2024-03-29', optionType: 'Call', contractSize: '1', settleCoin: 'USDT', isActive: true },
  { symbol: 'BTC-20240329-60000-P', underlying: 'BTC', strike: '60000', expiry: '2024-03-29', optionType: 'Put', contractSize: '1', settleCoin: 'USDT', isActive: true },
  { symbol: 'BTC-20240329-70000-C', underlying: 'BTC', strike: '70000', expiry: '2024-03-29', optionType: 'Call', contractSize: '1', settleCoin: 'USDT', isActive: true },
  { symbol: 'BTC-20240329-70000-P', underlying: 'BTC', strike: '70000', expiry: '2024-03-29', optionType: 'Put', contractSize: '1', settleCoin: 'USDT', isActive: true },
];

const MOCK_TICKERS: OptionTicker[] = [
  { symbol: 'BTC-20240329-60000-C', markPrice: '1225', iv: '0.72', openInterest: '50', volume24h: '10', turnover24h: '5000', expiryTimestamp: 1711670400 },
  { symbol: 'BTC-20240329-60000-P', markPrice: '825', iv: '0.68', openInterest: '40', volume24h: '8', turnover24h: '4000', expiryTimestamp: 1711670400 },
];

const MOCK_POSITIONS: OptionPosition[] = [
  {
    symbol: 'BTC-20240329-60000-C',
    underlying: 'BTC',
    qty: '1',
    avgEntryPremium: '1200',
    markPrice: '1250',
    unrealisedPnl: '50',
    updatedAt: '2024-03-29T10:00:00.000Z',
  },
];

const MOCK_GREEKS: OptionGreeks = {
  totalDelta: '0.65',
  totalGamma: '0.001',
  totalTheta: '-15',
  totalVega: '200',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetOptionInstruments.mockResolvedValue(MOCK_INSTRUMENTS);
  mockGetOptionTickers.mockResolvedValue(MOCK_TICKERS);
  mockGetOptionPositions.mockResolvedValue(MOCK_POSITIONS);
  mockGetOptionGreeks.mockResolvedValue(MOCK_GREEKS);
  mockPlaceOptionOrder.mockResolvedValue({ orderId: 'opt-order-1', symbol: 'BTC-20240329-60000-C', side: 'buy', qty: '1', premium: '1225', markPrice: '1225', status: 'open' });
});

describe('OptionsPage', () => {
  it('renders title, tabs, and underlying selector', async () => {
    await act(async () => { render(<OptionsPage />); });
    expect(screen.getByText('Options')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Chain' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Positions' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Greeks' })).toBeInTheDocument();
  });

  it('loads instruments and renders strike prices in chain', async () => {
    await act(async () => { render(<OptionsPage />); });
    expect(await screen.findByText('60000')).toBeInTheDocument();
    expect(screen.getByText('70000')).toBeInTheDocument();
    expect(mockGetOptionInstruments).toHaveBeenCalledWith('BTC');
    expect(mockGetOptionTickers).toHaveBeenCalledWith('BTC');
  });

  it('clicking a strike row populates the order entry symbol', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionsPage />); });

    // Strike row for 60000 call should be clickable
    const strikeRow = await screen.findByText('60000');
    await user.click(strikeRow.closest('tr') ?? strikeRow);

    // The order entry should show the selected symbol
    expect(screen.getByText('BTC-20240329-60000-C')).toBeInTheDocument();
  });

  it('placing an option order calls placeOptionOrder API', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionsPage />); });

    // Click strike to populate symbol in order entry
    const strikeRow = await screen.findByText('60000');
    await user.click(strikeRow.closest('tr') ?? strikeRow);

    await screen.findByText('BTC-20240329-60000-C');

    const qtyInput = screen.getByLabelText(/Quantity/i);
    await user.clear(qtyInput);
    await user.type(qtyInput, '1');

    const priceInput = screen.getByLabelText(/Price/i);
    await user.clear(priceInput);
    await user.type(priceInput, '1250');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: /Buy Option/i }));
    });

    expect(mockPlaceOptionOrder).toHaveBeenCalledWith(
      expect.objectContaining({
        symbol: 'BTC-20240329-60000-C',
        side: 'buy',
        qty: '1',
        premium: '1250',
      }),
    );
  });

  it('positions tab shows positions', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionsPage />); });

    await user.click(screen.getByRole('tab', { name: 'Positions' }));

    expect(await screen.findByText('BTC-20240329-60000-C')).toBeInTheDocument();
    expect(screen.getByText('1250')).toBeInTheDocument(); // mark price
  });

  it('switching underlying to ETH re-fetches instruments', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionsPage />); });

    await screen.findByText('60000'); // instruments loaded for BTC

    const underlyingSelect = screen.getByLabelText('Underlying');
    await act(async () => {
      await user.selectOptions(underlyingSelect, 'ETH');
    });

    expect(mockGetOptionInstruments).toHaveBeenCalledWith('ETH');
  });

  it('greeks tab shows portfolio greek totals', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<OptionsPage />); });

    await user.click(screen.getByRole('tab', { name: 'Greeks' }));

    expect(await screen.findByText('0.65')).toBeInTheDocument(); // totalDelta
    expect(mockGetOptionGreeks).toHaveBeenCalled();
  });
});
