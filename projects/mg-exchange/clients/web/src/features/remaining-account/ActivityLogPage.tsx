import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { get } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface ActivityEntry {
  id: string;
  action: string;
  ip: string;
  device: string;
  created_at: number;
}

interface ActivityRow extends Record<string, unknown> {
  id: string;
  action: string;
  ip: string;
  device: string;
  created_at: number;
}

const COLUMNS: ColumnDef<ActivityRow>[] = [
  { key: 'action', header: 'Action' },
  { key: 'ip', header: 'IP Address', width: '140px' },
  { key: 'device', header: 'Device' },
  { key: 'created_at', header: 'Time', render: (r) => new Date((r.created_at as number) * 1000).toLocaleString() },
];

export default function ActivityLogPage() {
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    get<ActivityEntry[]>('/account/activity')
      .then(setEntries)
      .catch(() => setEntries([]))
      .finally(() => setLoading(false));
  }, []);

  const rows: ActivityRow[] = entries.map((e) => ({
    id: e.id, action: e.action, ip: e.ip, device: e.device, created_at: e.created_at,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Activity Log</h1>
      <Table<ActivityRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No activity recorded" />
    </div>
  );
}
