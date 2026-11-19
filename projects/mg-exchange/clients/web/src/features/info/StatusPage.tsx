import { useState, useEffect } from 'react';
import { Badge } from '@shared/ui/Badge';
import { get } from '@shared/api/client';
import styles from './InfoPage.module.css';

interface HealthStatus {
  api: string;
  websocket: string;
  database: string;
  matching_engine: string;
  market_data: string;
  settlement: string;
}

type BadgeVariant = 'success' | 'warning' | 'danger' | 'neutral';

function variant(status: string): BadgeVariant {
  if (status === 'ok' || status === 'operational') return 'success';
  if (status === 'degraded') return 'warning';
  return 'danger';
}

const SERVICE_LABELS: [keyof HealthStatus, string][] = [
  ['api', 'REST API'],
  ['websocket', 'WebSocket'],
  ['database', 'Database'],
  ['matching_engine', 'Matching Engine'],
  ['market_data', 'Market Data'],
  ['settlement', 'Settlement'],
];

export default function StatusPage() {
  const [health, setHealth] = useState<HealthStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastCheck, setLastCheck] = useState<Date | null>(null);

  function loadHealth() {
    setLoading(true);
    get<HealthStatus>('/health')
      .then((h) => { setHealth(h); setLastCheck(new Date()); })
      .catch(() => setHealth(null))
      .finally(() => setLoading(false));
  }

  useEffect(() => { loadHealth(); }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>System Status</h1>
      {lastCheck && (
        <p className={styles.subtitle}>Last updated: {lastCheck.toLocaleTimeString()}</p>
      )}

      {loading ? (
        <p style={{ color: 'var(--text-2)' }}>Checking status...</p>
      ) : (
        <div className={styles.statusGrid}>
          {SERVICE_LABELS.map(([key, label]) => (
            <div key={key} className={styles.statusItem}>
              <span className={styles.statusName}>{label}</span>
              <Badge variant={variant(health?.[key] ?? 'unknown')} size="sm">
                {health?.[key] ?? 'unknown'}
              </Badge>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
