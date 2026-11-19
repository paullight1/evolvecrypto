import { renderHook, act, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../api', () => ({
  getOrderBook: vi.fn(),
}));

vi.mock('@shared/api/websocket', () => ({
  ws: {
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
    onMessage: vi.fn(() => vi.fn()),
  },
}));

vi.mock('../stores/orderbook', () => {
  const applySnapshot = vi.fn();
  const applyDelta = vi.fn();
  const reset = vi.fn();
  return {
    useOrderBookStore: () => ({ applySnapshot, applyDelta, reset }),
  };
});

import { useOrderBook } from './useOrderBook';
import { getOrderBook } from '../api';
import { ws } from '@shared/api/websocket';
import { useOrderBookStore } from '../stores/orderbook';

const mockGetOrderBook = vi.mocked(getOrderBook);
const mockWs = vi.mocked(ws);

function getStore() {
  return useOrderBookStore() as ReturnType<typeof useOrderBookStore>;
}

const MOCK_SNAPSHOT = {
  bids: [['65000', '1.0']] as [string, string][],
  asks: [['65100', '0.5']] as [string, string][],
  sequence: 5,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetOrderBook.mockResolvedValue(MOCK_SNAPSHOT);
  mockWs.onMessage.mockReturnValue(vi.fn());
});

describe('useOrderBook', () => {
  it('calls getOrderBook API on mount with the given symbol', async () => {
    renderHook(() => useOrderBook('BTCUSDT'));

    await waitFor(() => {
      expect(mockGetOrderBook).toHaveBeenCalledWith('BTCUSDT', 25);
    });
  });

  it('subscribes to the orderbook WS topic on mount', () => {
    renderHook(() => useOrderBook('BTCUSDT'));

    expect(mockWs.subscribe).toHaveBeenCalledWith(['orderbook.BTCUSDT']);
  });

  it('registers a WS message handler on mount', () => {
    renderHook(() => useOrderBook('BTCUSDT'));

    expect(mockWs.onMessage).toHaveBeenCalled();
  });

  it('applies snapshot data to orderbook store after fetch', async () => {
    renderHook(() => useOrderBook('BTCUSDT'));

    await waitFor(() => {
      const { applySnapshot } = getStore();
      expect(applySnapshot).toHaveBeenCalledWith(
        MOCK_SNAPSHOT.bids,
        MOCK_SNAPSHOT.asks,
        MOCK_SNAPSHOT.sequence,
      );
    });
  });

  it('calls reset before fetching snapshot', async () => {
    renderHook(() => useOrderBook('BTCUSDT'));

    await waitFor(() => {
      const { reset } = getStore();
      expect(reset).toHaveBeenCalled();
    });
  });

  it('unsubscribes from WS topic on unmount', () => {
    const { unmount } = renderHook(() => useOrderBook('BTCUSDT'));

    unmount();

    expect(mockWs.unsubscribe).toHaveBeenCalledWith(['orderbook.BTCUSDT']);
  });

  it('removes the WS message handler on unmount', () => {
    const removeFn = vi.fn();
    mockWs.onMessage.mockReturnValue(removeFn);

    const { unmount } = renderHook(() => useOrderBook('BTCUSDT'));
    unmount();

    expect(removeFn).toHaveBeenCalled();
  });

  it('re-fetches and resets when symbol changes', async () => {
    const { rerender } = renderHook(({ sym }: { sym: string }) => useOrderBook(sym), {
      initialProps: { sym: 'BTCUSDT' },
    });

    await waitFor(() => {
      expect(mockGetOrderBook).toHaveBeenCalledWith('BTCUSDT', 25);
    });

    rerender({ sym: 'ETHUSDT' });

    await waitFor(() => {
      expect(mockGetOrderBook).toHaveBeenCalledWith('ETHUSDT', 25);
    });
  });

  it('re-subscribes to the new symbol topic when symbol changes', async () => {
    const { rerender } = renderHook(({ sym }: { sym: string }) => useOrderBook(sym), {
      initialProps: { sym: 'BTCUSDT' },
    });

    rerender({ sym: 'ETHUSDT' });

    await waitFor(() => {
      expect(mockWs.subscribe).toHaveBeenCalledWith(['orderbook.ETHUSDT']);
    });
  });

  it('applies delta updates received over WS', async () => {
    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    renderHook(() => useOrderBook('BTCUSDT'));

    await waitFor(() => {
      expect(capturedHandler).not.toBeNull();
    });

    const { applyDelta } = getStore();

    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'orderbook.BTCUSDT',
        data: {
          symbol: 'BTCUSDT',
          bids: [['65000', '2.0']] as [string, string][],
          asks: [] as [string, string][],
          sequence: 6,
        },
      });
    });

    expect(applyDelta).toHaveBeenCalledWith(
      [['65000', '2.0']],
      [],
      6,
    );
  });

  it('ignores WS messages for other topics', async () => {
    let capturedHandler: ((msg: unknown) => void) | null = null;
    mockWs.onMessage.mockImplementation((fn) => {
      capturedHandler = fn;
      return vi.fn();
    });

    renderHook(() => useOrderBook('BTCUSDT'));

    await waitFor(() => {
      expect(capturedHandler).not.toBeNull();
    });

    const { applyDelta } = getStore();

    act(() => {
      capturedHandler!({
        op: 'data',
        topic: 'orderbook.ETHUSDT',
        data: {
          symbol: 'ETHUSDT',
          bids: [['3000', '1.0']] as [string, string][],
          asks: [] as [string, string][],
          sequence: 1,
        },
      });
    });

    expect(applyDelta).not.toHaveBeenCalled();
  });
});
