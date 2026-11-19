import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import { Card } from '@shared/ui/Card';
import type { OpenOrder } from '@features/trading/types';
import styles from '../DashboardPage.module.css';

interface OrderRow extends Record<string, unknown> {
  order_id: string;
  symbol: string;
  side: string;
  qty: string;
  status: string;
  created_at: number;
}

const COLUMNS: ColumnDef<OrderRow>[] = [
  { key: 'symbol', header: 'Symbol', width: '120px' },
  { key: 'side', header: 'Side', width: '70px',
    render: (r) => <Badge variant={r.side === 'buy' ? 'success' : 'danger'} size="sm">{String(r.side)}</Badge> },
  { key: 'qty', header: 'Qty', align: 'right' },
  { key: 'status', header: 'Status', width: '100px' },
  { key: 'created_at', header: 'Date', render: (r) => new Date(r.created_at as number).toLocaleDateString() },
];

interface Props {
  orders: OpenOrder[];
  loading: boolean;
}

export function RecentOrdersList({ orders, loading }: Props) {
  const rows: OrderRow[] = orders.map((o) => ({
    order_id: o.order_id,
    symbol: o.symbol,
    side: o.side,
    qty: o.qty,
    status: o.status,
    created_at: o.created_at,
  }));

  return (
    <Card padding="md" className={styles.tableCard}>
      <h2 className={styles.cardTitle}>Recent Orders</h2>
      <Table<OrderRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No recent orders" />
    </Card>
  );
}
