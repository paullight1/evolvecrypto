import { useEffect } from 'react';
import { ws } from '@shared/api/websocket';
import type { WsIncoming, WsOrderBookUpdate } from '@shared/api/types';
import { useOrderBookStore } from '../stores/orderbook';
import { getOrderBook } from '../api';

export function useOrderBook(symbol: string): void {
  const { applySnapshot, applyDelta, reset } = useOrderBookStore();

  // Fetch snapshot and reset on symbol change
  useEffect(() => {
    reset();
    let cancelled = false;

    getOrderBook(symbol, 25)
      .then((snap) => {
        if (cancelled) return;
        applySnapshot(snap.bids, snap.asks, snap.sequence);
      })
      .catch(() => {
        // Leave as loading state — WS will provide data when connected
      });

    return () => {
      cancelled = true;
    };
  }, [symbol, applySnapshot, reset]);

  // Subscribe to WS delta updates
  useEffect(() => {
    const topic = `orderbook.${symbol}`;
    ws.subscribe([topic]);

    const removeHandler = ws.onMessage((msg: WsIncoming) => {
      if (msg.op !== 'data' || msg.topic !== topic) return;
      const u = msg.data as WsOrderBookUpdate;
      applyDelta(u.bids, u.asks, u.sequence);
    });

    return () => {
      ws.unsubscribe([topic]);
      removeHandler();
    };
  }, [symbol, applyDelta]);
}
