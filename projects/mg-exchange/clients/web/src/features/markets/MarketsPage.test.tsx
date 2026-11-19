import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getAllMarkets: vi.fn(),
}));

import MarketsPage from './MarketsPage';
import { getAllMarkets } from './api';
import type { Market } from './types';

const mockGetAllMarkets = vi.mocked(getAllMarkets);

const MOCK_MARKETS: Market[] = [
  {
    symbol: 'BTCUSDT',
    base_asset: 'BTC',
    quote_asset: 'USDT',
    market_type: 'spot',
    last_price: '65000.00',
    price_change_pct_24h: '2.5',
    volume_24h: '1500000000',
    high_24h: '66000.00',
    low_24h: '64000.00',
  },
  {
    symbol: 'ETHUSDT',
    base_asset: 'ETH',
    quote_asset: 'USDT',
    market_type: 'perpetual',
    last_price: '3500.00',
    price_change_pct_24h: '-1.2',
    volume_24h: '800000000',
    high_24h: '3600.00',
    low_24h: '3450.00',
  },
  {
    symbol: 'SOLUSDT',
    base_asset: 'SOL',
    quote_asset: 'USDT',
    market_type: 'spot',
    last_price: '150.00',
    price_change_pct_24h: '3.8',
    volume_24h: '300000000',
    high_24h: '155.00',
    low_24h: '148.00',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetAllMarkets.mockResolvedValue(MOCK_MARKETS);
});

describe('MarketsPage', () => {
  it('renders markets table', async () => {
    render(<MarketsPage />);

    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
    expect(screen.getByText('SOLUSDT')).toBeInTheDocument();
  });

  it('search filters markets by symbol', async () => {
    render(<MarketsPage />);

    // Wait for data, then search
    await screen.findByText('BTCUSDT');
    await userEvent.type(screen.getByPlaceholderText('Search symbol...'), 'ETH');

    await waitFor(() => {
      expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
      expect(screen.queryByText('BTCUSDT')).not.toBeInTheDocument();
      expect(screen.queryByText('SOLUSDT')).not.toBeInTheDocument();
    });
  });

  it('sort by price works', async () => {
    render(<MarketsPage />);
    await screen.findByText('BTCUSDT');

    // Default sort is desc by volume_24h. Click "last price" once → desc by price (BTC first).
    // Click again → asc by price (SOL first, then ETH, then BTC).
    const priceBtn = screen.getByRole('button', { name: /last price/i });
    await userEvent.click(priceBtn);  // now desc by price
    await userEvent.click(priceBtn);  // now asc by price

    // After ascending sort, SOL (lowest price: 150) should appear before BTC (highest: 65000)
    const rows = screen.getAllByRole('row');
    expect(rows.length).toBeGreaterThan(1);
    const rowTexts = rows.map((r) => r.textContent ?? '');
    const solIdx = rowTexts.findIndex((t) => t.includes('SOLUSDT'));
    const btcIdx = rowTexts.findIndex((t) => t.includes('BTCUSDT'));
    expect(solIdx).toBeLessThan(btcIdx);
  });

  it('tab switching All/Spot/Perpetual filters', async () => {
    render(<MarketsPage />);
    await screen.findByText('BTCUSDT');

    // Switch to Spot tab — should show only spot markets
    await userEvent.click(screen.getByRole('tab', { name: 'Spot' }));
    await waitFor(() => {
      expect(screen.getByText('BTCUSDT')).toBeInTheDocument();
      expect(screen.getByText('SOLUSDT')).toBeInTheDocument();
      expect(screen.queryByText('ETHUSDT')).not.toBeInTheDocument();
    });

    // Switch to Perpetual — should show only perpetual markets
    await userEvent.click(screen.getByRole('tab', { name: 'Perpetual' }));
    await waitFor(() => {
      expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
      expect(screen.queryByText('BTCUSDT')).not.toBeInTheDocument();
    });
  });
});
