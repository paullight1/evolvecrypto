import { renderHook, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../api', () => ({
  getRecentTrades: vi.fn(),
}));

vi.mock('@shared/api/websocket', () => ({
  ws: {
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
    onMessage: vi.fn(() => vi.fn()),
  },
}));

import { useRecentTrades } from './useRecentTrades';
import { getRecentTrades } from '../api';
import { ws } from '@shared/api/websocket';
import type { RecentTrade } from '../types';

const mockGetRecentTrades = vi.mocked(getRecentTrades);
const mockWs = vi.mocked(ws);

function makeTrade(id: number): RecentTrade {
  return {
    trade_id: String(id),
    price: `${65000 + id}`,
    qty: '0.1',
    side: 'Buy',
    timestamp: new Date().toISOString(),
  };
}

const INITIAL_TRADES: RecentTrade[] = [makeTrade(1), makeTrade(2), makeTrade(3)];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetRecentTrades.mockResolvedValue(INITIAL_TRADES);
  mockWs.onMessage.mockReturnValue(vi.fn());
});

describe('useRecentTrades', () => {
  it('fetches initial trades via REST on mount', async () => {
    renderHook(() => useRecentTrades('BTCUSDT'));

    await waitFor(() => {
      expect(mockGetRecentTrades).toHaveBeenCalledWith('BTCUSDT', 50);
    });
  });

  it('returns initial trades from REST response', async () => {
    const { result } = renderHook(() => useRecentTrades('BTCUSDT'));

    await waitFor(() => {
      expect(result.current).toHaveLength(3);
    });

    expect(result.current[0].trade_id).toBe('1');
  });

  it('subscribes to trade WS topic on mount', () => {
    renderHook(() => useRecentTrades('BTCUSDT'));

    expect(mockWs.subscribe).toHaveBeenCalledWith(['trade.BTCUSDT']);
  });

  it('registers a WS message handler on mount', () => {
    renderHook(() => useRecentTrades('BTCUSDT'));

    expect(mockWs.onMessage).toHaveBeenCalled();
  });

  it('unsubscribes from WS topic on unmount', () => {
    const { unmount } = renderHook(() => useRecentTrades('BTCUSDT'));

    unmount();

    expect(mockWs.unsubscribe).toHaveBeenCalledWith(['trade.BTCUSDT']);
  });

  it('removes the WS message handler on unmount', () => {
    const removeFn = vi.fn();
    mockWs.onMessage.mockReturnValue(removeFn);

    const { unmount } = renderHook(() => useRecentTrades('BTCUSDT'));
    unmount();

    expect(removeFn).toHaveBeenCalled();
  });

  it('prepends new trades received over WS', async () => {
    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    const { result } = renderHook(() => useRecentTrades('BTCUSDT'));

    await waitFor(() => {
      expect(result.current).toHaveLength(3);
    });

    const newTrade = {
      symbol: 'BTCUSDT',
      trade_id: 999,
      price: '66000',
      qty: '0.5',
      side: 'Buy' as const,
      timestamp: new Date().toISOString(),
    };

    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'trade.BTCUSDT',
        data: newTrade,
      });
    });

    expect(result.current[0].trade_id).toBe('999');
    expect(result.current).toHaveLength(4);
  });

  it('caps trades list at 50 entries', async () => {
    // Start with 50 trades
    const fiftyTrades = Array.from({ length: 50 }, (_, i) => makeTrade(i + 1));
    mockGetRecentTrades.mockResolvedValue(fiftyTrades);

    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    const { result } = renderHook(() => useRecentTrades('BTCUSDT'));

    await waitFor(() => {
      expect(result.current).toHaveLength(50);
    });

    // Add one more — should stay at 50
    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'trade.BTCUSDT',
        data: {
          symbol: 'BTCUSDT',
          trade_id: 9999,
          price: '67000',
          qty: '1.0',
          side: 'Sell' as const,
          timestamp: new Date().toISOString(),
        },
      });
    });

    expect(result.current).toHaveLength(50);
    // Newest trade is first
    expect(result.current[0].trade_id).toBe('9999');
  });

  it('re-fetches and clears list when symbol changes', async () => {
    const { result, rerender } = renderHook(({ sym }: { sym: string }) => useRecentTrades(sym), {
      initialProps: { sym: 'BTCUSDT' },
    });

    await waitFor(() => {
      expect(result.current).toHaveLength(3);
    });

    const ethTrades = [makeTrade(100), makeTrade(101)];
    mockGetRecentTrades.mockResolvedValue(ethTrades);

    rerender({ sym: 'ETHUSDT' });

    await waitFor(() => {
      expect(mockGetRecentTrades).toHaveBeenCalledWith('ETHUSDT', 50);
    });

    await waitFor(() => {
      expect(result.current[0].trade_id).toBe('100');
    });
  });

  it('re-subscribes to new symbol WS topic when symbol changes', () => {
    const { rerender } = renderHook(({ sym }: { sym: string }) => useRecentTrades(sym), {
      initialProps: { sym: 'BTCUSDT' },
    });

    rerender({ sym: 'ETHUSDT' });

    expect(mockWs.subscribe).toHaveBeenCalledWith(['trade.ETHUSDT']);
    expect(mockWs.unsubscribe).toHaveBeenCalledWith(['trade.BTCUSDT']);
  });

  it('ignores WS messages for other topics', async () => {
    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    const { result } = renderHook(() => useRecentTrades('BTCUSDT'));

    await waitFor(() => {
      expect(result.current).toHaveLength(3);
    });

    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'trade.ETHUSDT',
        data: {
          symbol: 'ETHUSDT',
          trade_id: 500,
          price: '3000',
          qty: '1.0',
          side: 'Buy' as const,
          timestamp: new Date().toISOString(),
        },
      });
    });

    // No change — still 3 initial trades
    expect(result.current).toHaveLength(3);
    expect(result.current[0].trade_id).toBe('1');
  });
});
