import { StatCard } from '@shared/ui/StatCard';
import type { AdminStats } from '../types';
import styles from '../AdminPage.module.css';

interface Props {
  stats: AdminStats | null;
  loading: boolean;
}

export function StatsGrid({ stats, loading }: Props) {
  const items = [
    { label: 'Total Users', value: stats?.total_users.toLocaleString() ?? '—' },
    { label: 'Active 24h', value: stats?.active_users_24h.toLocaleString() ?? '—' },
    { label: '24h Volume', value: stats?.total_volume_24h ?? '—' },
    { label: 'Open Orders', value: stats?.open_orders.toLocaleString() ?? '—' },
    { label: 'Open Positions', value: stats?.open_positions.toLocaleString() ?? '—' },
    { label: 'Insurance Fund', value: stats?.insurance_fund ?? '—' },
  ];

  return (
    <div className={styles.statsGrid}>
      {items.map((item) => (
        <StatCard key={item.label} label={item.label} value={item.value} loading={loading} />
      ))}
    </div>
  );
}
