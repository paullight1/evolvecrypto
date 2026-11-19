import { Badge } from '@shared/ui/Badge';
import { Card } from '@shared/ui/Card';
import type { SystemHealth } from '../types';
import styles from '../AdminPage.module.css';

interface Props {
  health: SystemHealth | null;
  loading: boolean;
}

type BadgeVariant = 'success' | 'danger' | 'warning' | 'neutral';

function statusVariant(status: string): BadgeVariant {
  if (status === 'ok' || status === 'healthy') return 'success';
  if (status === 'degraded') return 'warning';
  return 'danger';
}

export function SystemHealthPanel({ health, loading }: Props) {
  if (loading) return <div className={styles.skeletonStat} style={{ height: '120px' }} />;

  const services = [
    { name: 'API', status: health?.api ?? 'unknown' },
    { name: 'WebSocket', status: health?.websocket ?? 'unknown' },
    { name: 'Database', status: health?.database ?? 'unknown' },
    { name: 'Matching Engine', status: health?.matching_engine ?? 'unknown' },
  ];

  return (
    <Card padding="md">
      <h3 className={styles.sectionTitle}>System Health</h3>
      <div className={styles.healthGrid}>
        {services.map((s) => (
          <div key={s.name} className={styles.healthItem}>
            <span className={styles.healthName}>{s.name}</span>
            <Badge variant={statusVariant(s.status)} size="sm">{s.status}</Badge>
          </div>
        ))}
      </div>
      {health && (
        <div className={styles.healthLatency}>Latency: {health.latency_ms}ms</div>
      )}
    </Card>
  );
}
