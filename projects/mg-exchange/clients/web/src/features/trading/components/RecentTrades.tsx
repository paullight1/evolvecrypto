import { useRecentTrades } from '../hooks/useRecentTrades';
import styles from './RecentTrades.module.css';
import { cn } from '@shared/utils/format';

interface RecentTradesProps {
  symbol: string;
}

function formatTime(ts: string): string {
  const d = new Date(ts);
  return d.toLocaleTimeString('en-US', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function formatQty(qty: string): string {
  const n = parseFloat(qty);
  if (!isFinite(n)) return qty;
  if (n >= 1000) return n.toFixed(2);
  if (n >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

export function RecentTrades({ symbol }: RecentTradesProps) {
  const trades = useRecentTrades(symbol);

  return (
    <div className={styles.panel}>
      <div className={styles.header}>Recent Trades</div>
      <div className={styles.labels}>
        <span>Price</span>
        <span>Size</span>
        <span>Time</span>
      </div>
      <div className={styles.list}>
        {trades.length === 0 ? (
          <div className={styles.empty}>Waiting for trades...</div>
        ) : (
          trades.map((t) => {
            const isBuy = t.side === 'Buy';
            return (
              <div key={t.trade_id} className={cn(styles.row, !isBuy && styles.rowSell)}>
                <span className={cn(styles.price, isBuy ? styles.priceBuy : styles.priceSell)}>
                  {t.price}
                </span>
                <span className={styles.qty}>{formatQty(t.qty)}</span>
                <span className={styles.time}>{formatTime(t.timestamp)}</span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
