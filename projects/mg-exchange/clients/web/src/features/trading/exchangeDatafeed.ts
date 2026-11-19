/**
 * Datafeed adapter for @mg-exchange/charts.
 * Maps our REST + WS APIs to the chart library's Datafeed interface.
 */
import type { Datafeed } from '@mg-exchange/charts';
import { getKlines, getMarkets } from './api';
import { ws } from '@shared/api/websocket';
import type { WsIncoming, WsKline } from '@shared/api/types';

function parseNum(s: string): number {
  return parseFloat(s);
}

export const exchangeDatafeed: Datafeed = {
  async getBars({ symbol, timeframe, limit }) {
    const klines = await getKlines(symbol, timeframe, limit);
    return klines.map((k) => ({
      time: k.open_time,
      open: parseNum(k.open),
      high: parseNum(k.high),
      low: parseNum(k.low),
      close: parseNum(k.close),
      volume: parseNum(k.volume),
    }));
  },

  subscribe({ symbol, timeframe, onBar, onTick }) {
    const topic = `kline.${timeframe}.${symbol}`;
    ws.subscribe([topic]);

    let lastBarTime = 0;

    const removeHandler = ws.onMessage((msg: WsIncoming) => {
      if (msg.op !== 'data' || msg.topic !== topic) return;
      const kline = msg.data as WsKline;

      if (kline.confirmed && kline.open_time > lastBarTime) {
        lastBarTime = kline.open_time;
        onBar({
          time: kline.open_time,
          open: parseNum(kline.open),
          high: parseNum(kline.high),
          low: parseNum(kline.low),
          close: parseNum(kline.close),
          volume: parseNum(kline.volume),
        });
      } else {
        onTick({
          time: kline.open_time,
          price: parseNum(kline.close),
          volume: parseNum(kline.volume),
        });
      }
    });

    return () => {
      ws.unsubscribe([topic]);
      removeHandler();
    };
  },

  async searchSymbols(query) {
    const markets = await getMarkets();
    const q = query.toLowerCase();
    return markets
      .filter((m) => m.symbol.toLowerCase().includes(q))
      .map((m) => ({
        symbol: m.symbol,
        name: `${m.base_asset}/${m.quote_asset}`,
        exchange: 'MG Exchange',
        type: m.market_type === 'coin_m' ? 'perpetual' as const : m.market_type as 'spot' | 'perpetual',
        pricePrecision: m.symbol.includes('BTC') ? 1 : 2,
        qtyPrecision: m.symbol.includes('BTC') ? 5 : 3,
        minMove: m.symbol.includes('BTC') ? 0.1 : 0.01,
      }));
  },
};
