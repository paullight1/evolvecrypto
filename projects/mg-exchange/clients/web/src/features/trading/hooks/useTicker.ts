import { useEffect } from 'react';
import { ws } from '@shared/api/websocket';
import type { WsIncoming } from '@shared/api/types';
import { useMarketStore } from '../stores/market';
import { getTicker } from '../api';
import type { Ticker } from '../types';

export function useTicker(symbol: string): void {
  const { updateTicker } = useMarketStore();

  // Fetch initial ticker via REST
  useEffect(() => {
    let cancelled = false;

    getTicker(symbol)
      .then((t: Ticker) => {
        if (cancelled) return;
        updateTicker({
          lastPrice: t.last_price,
          priceChangePct24h: t.price_change_pct_24h,
          priceChange24h: t.price_change_24h ?? '',
          high24h: t.high_24h,
          low24h: t.low_24h,
          volume24h: t.volume_24h,
          fundingRate: t.funding_rate ?? '',
          nextFundingTime: t.next_funding_time ?? 0,
          markPrice: t.mark_price ?? '',
          indexPrice: t.index_price ?? '',
          openInterest: t.open_interest ?? '',
        });
      })
      .catch(() => {
        // Non-fatal — WS will provide data
      });

    return () => {
      cancelled = true;
    };
  }, [symbol, updateTicker]);

  // Subscribe to WS ticker updates
  useEffect(() => {
    const topic = `ticker.${symbol}`;
    ws.subscribe([topic]);

    const removeHandler = ws.onMessage((msg: WsIncoming) => {
      if (msg.op !== 'data' || msg.topic !== topic) return;
      const t = msg.data as unknown as Ticker;
      updateTicker({
        lastPrice: t.last_price,
        priceChangePct24h: t.price_change_pct_24h,
        priceChange24h: t.price_change_24h ?? '',
        high24h: t.high_24h,
        low24h: t.low_24h,
        volume24h: t.volume_24h,
        fundingRate: t.funding_rate ?? '',
        nextFundingTime: t.next_funding_time ?? 0,
        markPrice: t.mark_price ?? '',
        indexPrice: t.index_price ?? '',
        openInterest: t.open_interest ?? '',
      });
    });

    return () => {
      ws.unsubscribe([topic]);
      removeHandler();
    };
  }, [symbol, updateTicker]);
}
