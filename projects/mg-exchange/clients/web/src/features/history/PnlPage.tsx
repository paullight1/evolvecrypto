import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { get } from '@shared/api/client';
import styles from './HistoryPage.module.css';

interface PnlStats {
  total_pnl: string;
  realized_pnl: string;
  unrealized_pnl: string;
  win_rate: string;
  best_trade: string;
  worst_trade: string;
}

export default function PnlPage() {
  const [stats, setStats] = useState<PnlStats | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    get<PnlStats>('/account/pnl')
      .then(setStats)
      .catch(() => setStats(null))
      .finally(() => setLoading(false));
  }, []);

  const items = stats ? [
    { label: 'Total PnL', value: stats.total_pnl },
    { label: 'Realized PnL', value: stats.realized_pnl },
    { label: 'Unrealized PnL', value: stats.unrealized_pnl },
    { label: 'Win Rate', value: `${stats.win_rate}%` },
    { label: 'Best Trade', value: stats.best_trade },
    { label: 'Worst Trade', value: stats.worst_trade },
  ] : [];

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>PnL Performance</h1>
      {loading ? (
        <p style={{ color: 'var(--text-2)' }}>Loading...</p>
      ) : (
        <div className={styles.taxStats} style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
          {items.map((item) => (
            <Card key={item.label} padding="sm">
              <div className={styles.taxStat}>
                <span className={styles.taxStatLabel}>{item.label}</span>
                <span className={styles.taxStatValue}>{item.value}</span>
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
