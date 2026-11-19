import { Card } from '@shared/ui/Card';
import type { WalletOverview } from '../types';
import styles from '../DashboardPage.module.css';

interface Props {
  overview: WalletOverview | null;
  loading: boolean;
}

export function PortfolioSummary({ overview, loading }: Props) {
  if (loading) return <div className={styles.skeletonCard} />;

  return (
    <Card padding="md" className={styles.summaryCard}>
      <h2 className={styles.cardTitle}>Portfolio</h2>
      <div className={styles.equityRow}>
        <span className={styles.equityLabel}>Total Equity</span>
        <span className={styles.equityValue}>{overview?.total_equity_usd ?? '—'} USDT</span>
      </div>
    </Card>
  );
}
