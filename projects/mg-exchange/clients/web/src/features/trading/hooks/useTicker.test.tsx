import { renderHook, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../api', () => ({
  getTicker: vi.fn(),
}));

vi.mock('@shared/api/websocket', () => ({
  ws: {
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
    onMessage: vi.fn(() => vi.fn()),
  },
}));

vi.mock('../stores/market', () => {
  const updateTicker = vi.fn();
  return {
    useMarketStore: () => ({ updateTicker }),
  };
});

import { useTicker } from './useTicker';
import { getTicker } from '../api';
import { ws } from '@shared/api/websocket';
import { useMarketStore } from '../stores/market';
import type { Ticker } from '../types';

const mockGetTicker = vi.mocked(getTicker);
const mockWs = vi.mocked(ws);

function getStore() {
  return useMarketStore() as ReturnType<typeof useMarketStore>;
}

const MOCK_TICKER: Ticker = {
  symbol: 'BTCUSDT',
  last_price: '65000',
  price_change_pct_24h: '2.5',
  price_change_24h: '1600',
  high_24h: '66000',
  low_24h: '63500',
  volume_24h: '12000',
  quote_volume_24h: '780000000',
  funding_rate: '0.0001',
  next_funding_time: 1700000000,
  mark_price: '65010',
  index_price: '65005',
  open_interest: '50000',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetTicker.mockResolvedValue(MOCK_TICKER);
  mockWs.onMessage.mockReturnValue(vi.fn());
});

describe('useTicker', () => {
  it('fetches initial ticker via REST on mount', async () => {
    renderHook(() => useTicker('BTCUSDT'));

    await waitFor(() => {
      expect(mockGetTicker).toHaveBeenCalledWith('BTCUSDT');
    });
  });

  it('updates market store with fetched ticker data', async () => {
    renderHook(() => useTicker('BTCUSDT'));

    await waitFor(() => {
      const { updateTicker } = getStore();
      expect(updateTicker).toHaveBeenCalledWith(
        expect.objectContaining({
          lastPrice: MOCK_TICKER.last_price,
          priceChangePct24h: MOCK_TICKER.price_change_pct_24h,
          high24h: MOCK_TICKER.high_24h,
          low24h: MOCK_TICKER.low_24h,
          volume24h: MOCK_TICKER.volume_24h,
          fundingRate: MOCK_TICKER.funding_rate,
          markPrice: MOCK_TICKER.mark_price,
          indexPrice: MOCK_TICKER.index_price,
        }),
      );
    });
  });

  it('subscribes to ticker WS topic on mount', () => {
    renderHook(() => useTicker('BTCUSDT'));

    expect(mockWs.subscribe).toHaveBeenCalledWith(['ticker.BTCUSDT']);
  });

  it('registers a WS message handler on mount', () => {
    renderHook(() => useTicker('BTCUSDT'));

    expect(mockWs.onMessage).toHaveBeenCalled();
  });

  it('unsubscribes from WS topic on unmount', () => {
    const { unmount } = renderHook(() => useTicker('BTCUSDT'));

    unmount();

    expect(mockWs.unsubscribe).toHaveBeenCalledWith(['ticker.BTCUSDT']);
  });

  it('removes the WS message handler on unmount', () => {
    const removeFn = vi.fn();
    mockWs.onMessage.mockReturnValue(removeFn);

    const { unmount } = renderHook(() => useTicker('BTCUSDT'));
    unmount();

    expect(removeFn).toHaveBeenCalled();
  });

  it('re-fetches ticker when symbol changes', async () => {
    const { rerender } = renderHook(({ sym }: { sym: string }) => useTicker(sym), {
      initialProps: { sym: 'BTCUSDT' },
    });

    await waitFor(() => {
      expect(mockGetTicker).toHaveBeenCalledWith('BTCUSDT');
    });

    rerender({ sym: 'ETHUSDT' });

    await waitFor(() => {
      expect(mockGetTicker).toHaveBeenCalledWith('ETHUSDT');
    });
  });

  it('re-subscribes to new symbol WS topic when symbol changes', () => {
    const { rerender } = renderHook(({ sym }: { sym: string }) => useTicker(sym), {
      initialProps: { sym: 'BTCUSDT' },
    });

    rerender({ sym: 'ETHUSDT' });

    expect(mockWs.subscribe).toHaveBeenCalledWith(['ticker.ETHUSDT']);
  });

  it('updates market store when a WS ticker message arrives', async () => {
    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    renderHook(() => useTicker('BTCUSDT'));

    await waitFor(() => {
      expect(capturedHandler).not.toBeNull();
    });

    const { updateTicker } = getStore();

    const wsTickerData: Ticker = {
      ...MOCK_TICKER,
      last_price: '66000',
      price_change_pct_24h: '3.1',
    };

    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'ticker.BTCUSDT',
        data: wsTickerData,
      });
    });

    expect(updateTicker).toHaveBeenCalledWith(
      expect.objectContaining({
        lastPrice: '66000',
        priceChangePct24h: '3.1',
      }),
    );
  });

  it('ignores WS messages for other topics', async () => {
    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    renderHook(() => useTicker('BTCUSDT'));

    await waitFor(() => {
      expect(capturedHandler).not.toBeNull();
    });

    // Clear any calls from REST fetch
    const { updateTicker } = getStore();
    vi.clearAllMocks();

    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'ticker.ETHUSDT',
        data: MOCK_TICKER,
      });
    });

    expect(updateTicker).not.toHaveBeenCalled();
  });

  it('handles optional ticker fields with defaults when missing', async () => {
    const minimalTicker: Ticker = {
      symbol: 'BTCUSDT',
      last_price: '65000',
      price_change_pct_24h: '1.0',
      high_24h: '66000',
      low_24h: '64000',
      volume_24h: '5000',
      quote_volume_24h: '325000000',
    };
    mockGetTicker.mockResolvedValue(minimalTicker);

    renderHook(() => useTicker('BTCUSDT'));

    await waitFor(() => {
      const { updateTicker } = getStore();
      expect(updateTicker).toHaveBeenCalledWith(
        expect.objectContaining({
          fundingRate: '',
          markPrice: '',
          indexPrice: '',
          openInterest: '',
          nextFundingTime: 0,
          priceChange24h: '',
        }),
      );
    });
  });
});
