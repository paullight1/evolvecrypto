import { render, screen, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { Ticker } from '../../types';

// Mock the trading API (used by useTicker hook inside TickerBar)
vi.mock('../../api', () => ({
  getTicker: vi.fn(),
  getOrderBook: vi.fn(),
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
    getStatus: vi.fn(() => 'disconnected'),
    connect: vi.fn(),
  },
}));

import { TickerBar } from '../TickerBar';
import { useMarketStore } from '../../stores/market';
import { getTicker } from '../../api';

const mockGetTicker = vi.mocked(getTicker);

function makeTicker(overrides: Partial<Ticker> = {}): Ticker {
  return {
    symbol: 'BTCUSDT',
    last_price: '',
    price_change_pct_24h: '',
    high_24h: '',
    low_24h: '',
    volume_24h: '',
    quote_volume_24h: '',
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  // Default mock: return empty ticker so it doesn't interfere
  mockGetTicker.mockResolvedValue(makeTicker());

  useMarketStore.setState({
    symbol: 'BTCUSDT',
    productType: 'perpetual',
    tradingMode: 'futures',
    lastPrice: '',
    priceDirection: 'flat',
    priceChangePct24h: '',
    priceChange24h: '',
    high24h: '',
    low24h: '',
    volume24h: '',
    fundingRate: '',
    nextFundingTime: 0,
    markPrice: '',
    indexPrice: '',
    openInterest: '',
    lastTickerUpdateTime: 0,
    spreadPct: '',
  });
});

describe('TickerBar', () => {
  it('shows the symbol name', async () => {
    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    expect(screen.getByText('BTCUSDT')).toBeInTheDocument();
  });

  it('shows last price when available via API', async () => {
    mockGetTicker.mockResolvedValue(makeTicker({
      last_price: '65000',
      price_change_pct_24h: '1.0',
      high_24h: '66000',
      low_24h: '64000',
      volume_24h: '5000',
    }));

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    // formatPrice(65000) -> '65,000.0'
    await waitFor(() => {
      expect(screen.getByText(/65,000/)).toBeInTheDocument();
    });
  });

  it('shows dash for price when no last price', async () => {
    mockGetTicker.mockResolvedValue(makeTicker({ last_price: '' }));

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    await waitFor(() => {
      expect(screen.getAllByText('—').length).toBeGreaterThan(0);
    });
  });

  it('shows positive 24h change percentage with + prefix', async () => {
    mockGetTicker.mockResolvedValue(makeTicker({ price_change_pct_24h: '2.5' }));

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    await waitFor(() => {
      expect(screen.getByText('+2.50%')).toBeInTheDocument();
    });
  });

  it('shows negative 24h change percentage without + prefix', async () => {
    mockGetTicker.mockResolvedValue(makeTicker({ price_change_pct_24h: '-1.3' }));

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    await waitFor(() => {
      expect(screen.getByText('-1.30%')).toBeInTheDocument();
    });
  });

  it('shows 24h volume', async () => {
    mockGetTicker.mockResolvedValue(makeTicker({ volume_24h: '12000000' }));

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    // formatVolume(12000000) -> '12.00M'
    await waitFor(() => {
      expect(screen.getByText('12.00M')).toBeInTheDocument();
    });
  });

  it('shows funding rate for perpetuals', async () => {
    mockGetTicker.mockResolvedValue(makeTicker({
      funding_rate: '0.0001',
      next_funding_time: 0,
    }));

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    // fundingRate 0.0001 * 100 = 0.0100%
    await waitFor(() => {
      expect(screen.getByText(/0\.0100%/)).toBeInTheDocument();
    });
  });

  it('does not show funding rate for spot', async () => {
    useMarketStore.setState({
      symbol: 'BTCUSDT',
      productType: 'spot',
      tradingMode: 'spot',
    });

    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    expect(screen.queryByText('Funding / Next')).not.toBeInTheDocument();
  });

  it('calls onPairListOpen when symbol button is clicked', async () => {
    const onOpen = vi.fn();
    await act(async () => {
      render(<TickerBar onPairListOpen={onOpen} />);
    });

    // The symbol button contains the symbol name text
    const symbolBtn = screen.getByRole('button', { name: /BTCUSDT/i });
    symbolBtn.click();

    expect(onOpen).toHaveBeenCalled();
  });

  it('shows Spot/USDT-M/COIN-M mode tabs', async () => {
    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    expect(screen.getByRole('button', { name: 'Spot' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'USDT-M' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'COIN-M' })).toBeInTheDocument();
  });

  it('shows 24h high and low labels', async () => {
    await act(async () => {
      render(<TickerBar onPairListOpen={() => {}} />);
    });

    expect(screen.getByText('24h High')).toBeInTheDocument();
    expect(screen.getByText('24h Low')).toBeInTheDocument();
  });
});
