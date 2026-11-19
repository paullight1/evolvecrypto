import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import type { AmlAlert } from '../types';

interface AlertRow extends Record<string, unknown> {
  id: string;
  user_id: string;
  alert_type: string;
  severity: string;
  status: string;
  created_at: number;
}

const COLUMNS: ColumnDef<AlertRow>[] = [
  { key: 'user_id', header: 'User ID', width: '120px' },
  { key: 'alert_type', header: 'Type' },
  {
    key: 'severity',
    header: 'Severity',
    render: (r) => {
      const s = String(r.severity);
      const variant = s === 'high' ? 'danger' : s === 'medium' ? 'warning' : 'neutral';
      return <Badge variant={variant} size="sm">{s}</Badge>;
    },
  },
  { key: 'status', header: 'Status', width: '100px' },
  { key: 'created_at', header: 'Date', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
];

interface Props {
  alerts: AmlAlert[];
  loading: boolean;
}

export function AmlAlertsTable({ alerts, loading }: Props) {
  const rows: AlertRow[] = alerts.map((a) => ({
    id: a.id,
    user_id: a.user_id,
    alert_type: a.alert_type,
    severity: a.severity,
    status: a.status,
    created_at: a.created_at,
  }));

  return <Table<AlertRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No AML alerts" />;
}
