import { useEffect, useCallback } from 'react';
import { ChartContainer, useChart } from '@mg-exchange/charts/react';
import type { ChartEventMap } from '@mg-exchange/charts';
import { exchangeDatafeed } from '../exchangeDatafeed';
import { useThemeStore } from '@shared/stores/theme';
import { amendOrder, getOpenOrders, getPositions } from '../api';
import styles from './Chart.module.css';

interface ChartProps {
  symbol: string;
}

export function Chart({ symbol }: ChartProps) {
  const { ref: chartContainerRef, getChart } = useChart();
  const theme = useThemeStore((s) => s.theme);

  // Sync order lines from open orders
  const syncOrderLines = useCallback(async () => {
    const chart = getChart();
    if (!chart) return;
    try {
      const orders = await getOpenOrders(symbol);

      // Clear existing, re-add
      for (const line of chart.getOrderLines()) {
        chart.removeOrderLine(line.id);
      }

      for (const order of orders) {
        if (!order.price) continue;
        chart.addOrderLine({
          id: order.order_id,
          price: parseFloat(order.price),
          type: order.order_type === 'limit' ? 'limit' : 'stop',
          side: order.side.toLowerCase() as 'buy' | 'sell',
          label: `${order.side} ${order.order_type} ${order.qty}`,
          quantity: parseFloat(order.qty),
        });
      }
    } catch {
      // Orders may not be available (not logged in, etc.)
    }
  }, [symbol, getChart]);

  // Sync position overlays
  const syncPositions = useCallback(async () => {
    const chart = getChart();
    if (!chart) return;
    try {
      const positions = await getPositions();
      const overlays = positions
        .filter((p) => p.symbol === symbol && parseFloat(p.size) !== 0)
        .map((p) => ({
          side: p.side.toLowerCase() as 'long' | 'short',
          entryPrice: parseFloat(p.entry_price),
          quantity: parseFloat(p.size),
          currentPrice: parseFloat(p.mark_price),
          pnl: parseFloat(p.unrealized_pnl),
          tpLevels: p.tp_price ? [{ price: parseFloat(p.tp_price), quantity: parseFloat(p.size) }] : [],
          slLevels: p.sl_price ? [{ price: parseFloat(p.sl_price), quantity: parseFloat(p.size) }] : [],
        }));
      chart.setPositionOverlays(overlays);
    } catch {
      // Positions may not be available
    }
  }, [symbol, getChart]);

  // Refresh orders + positions periodically
  useEffect(() => {
    void syncOrderLines();
    void syncPositions();
    const interval = setInterval(() => {
      void syncOrderLines();
      void syncPositions();
    }, 5000);
    return () => clearInterval(interval);
  }, [syncOrderLines, syncPositions]);

  // Wire up trading events
  useEffect(() => {
    const chart = getChart();
    if (!chart) return;

    const handleOrderLineMoved = ({ id, price }: ChartEventMap['orderLineMoved']) => {
      void amendOrder(id, { price: String(price) }).then(() => syncOrderLines());
    };

    chart.on('orderLineMoved', handleOrderLineMoved);

    chart.setTradeMode(true, {
      showPlusButton: true,
      showContextMenu: true,
      draggableOrderLines: true,
    });

    return () => {
      chart.off('orderLineMoved', handleOrderLineMoved);
    };
  }, [getChart, syncOrderLines]);

  return (
    <div className={styles.panel}>
      <ChartContainer
        ref={chartContainerRef}
        options={{
          symbol,
          timeframe: '1m',
          datafeed: exchangeDatafeed,
          theme,
          renderer: 'webgl',
        }}
        symbol={symbol}
        theme={theme}
        className={styles.canvas}
      />
    </div>
  );
}
