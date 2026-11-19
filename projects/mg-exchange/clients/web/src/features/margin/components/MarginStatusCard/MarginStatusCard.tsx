import { Card } from '@shared/ui/Card';
import { Badge } from '@shared/ui/Badge';
import type { MarginStatus } from '../../types';
import styles from './MarginStatusCard.module.css';

interface MarginStatusCardProps {
  status: MarginStatus | null;
  loading: boolean;
}

interface StatRowProps {
  label: string;
  value: string;
}

function StatRow({ label, value }: StatRowProps) {
  return (
    <div className={styles.row}>
      <span className={styles.label}>{label}</span>
      <span className={styles.value}>{value}</span>
    </div>
  );
}

function marginLevelVariant(level: string): 'success' | 'warning' | 'danger' {
  const v = parseFloat(level);
  if (v >= 2) return 'success';
  if (v >= 1.3) return 'warning';
  return 'danger';
}

export function MarginStatusCard({ status, loading }: MarginStatusCardProps) {
  if (loading) {
    return (
      <Card className={styles.card}>
        <p className={styles.loading}>Loading margin status...</p>
      </Card>
    );
  }

  if (!status) {
    return (
      <Card className={styles.card}>
        <p className={styles.empty}>Margin status unavailable.</p>
      </Card>
    );
  }

  return (
    <Card className={styles.card}>
      <div className={styles.header}>
        <h3 className={styles.title}>Margin Account</h3>
        <Badge variant={status.enabled ? 'success' : 'neutral'}>
          {status.enabled ? 'Active' : 'Disabled'}
        </Badge>
      </div>
      <div className={styles.stats}>
        <div className={styles.levelRow}>
          <span className={styles.label}>Margin Level</span>
          <Badge variant={marginLevelVariant(status.margin_level)}>
            {status.margin_level}x
          </Badge>
        </div>
        <StatRow label="Available Margin" value={status.available_margin} />
        <StatRow label="Used Margin" value={status.used_margin} />
        <StatRow label="Total Assets" value={status.total_assets} />
        <StatRow label="Total Liabilities" value={status.total_liabilities} />
      </div>
    </Card>
  );
}
