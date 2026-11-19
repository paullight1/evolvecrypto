import { useEffect, useState } from 'react';
import { ws } from '@shared/api/websocket';
import type { WsIncoming, WsTrade } from '@shared/api/types';
import { getRecentTrades } from '../api';
import type { RecentTrade } from '../types';

const MAX_TRADES = 50;

export function useRecentTrades(symbol: string): RecentTrade[] {
  const [trades, setTrades] = useState<RecentTrade[]>([]);

  // Fetch initial trades via REST
  useEffect(() => {
    setTrades([]);
    let cancelled = false;

    getRecentTrades(symbol, MAX_TRADES)
      .then((data) => {
        if (cancelled) return;
        setTrades(data);
      })
      .catch(() => {
        // Leave empty — WS will populate
      });

    return () => {
      cancelled = true;
    };
  }, [symbol]);

  // Subscribe to live WS trade updates
  useEffect(() => {
    const topic = `trade.${symbol}`;
    ws.subscribe([topic]);

    const removeHandler = ws.onMessage((msg: WsIncoming) => {
      if (msg.op !== 'data' || msg.topic !== topic) return;
      // The WS MarketEvent::Trade has `taker_side` (String) and `timestamp` (i64 unix ms)
      const t = msg.data as WsTrade & { taker_side?: string };
      const side = (t.taker_side ?? t.side ?? 'Buy') as 'Buy' | 'Sell';
      const ts = typeof t.timestamp === 'number'
        ? new Date(t.timestamp).toISOString()
        : (t.timestamp as string);
      const entry: RecentTrade = {
        trade_id: String(t.trade_id),
        price: t.price,
        qty: t.qty,
        side,
        timestamp: ts,
      };
      setTrades((prev) => {
        const next = [entry, ...prev];
        return next.length > MAX_TRADES ? next.slice(0, MAX_TRADES) : next;
      });
    });

    return () => {
      ws.unsubscribe([topic]);
      removeHandler();
    };
  }, [symbol]);

  return trades;
}
