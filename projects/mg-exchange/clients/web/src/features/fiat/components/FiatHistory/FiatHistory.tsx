import { Badge } from '@shared/ui/Badge';
import { Table, type ColumnDef } from '@shared/ui/Table';
import type { FiatOrder } from '../../types';

interface FiatHistoryProps {
  orders: FiatOrder[];
  loading: boolean;
}

type FiatOrderRow = FiatOrder & Record<string, unknown>;

const COLUMNS: ColumnDef<FiatOrderRow>[] = [
  { key: 'type', header: 'Type', width: '70px', render: (r) => (r.type as string).toUpperCase() },
  { key: 'fiat_currency', header: 'Fiat', width: '70px' },
  { key: 'crypto_asset', header: 'Crypto', width: '80px' },
  { key: 'fiat_amount', header: 'Fiat Amount', align: 'right' },
  { key: 'crypto_amount', header: 'Crypto Amount', align: 'right' },
  { key: 'payment_method', header: 'Method' },
  {
    key: 'status',
    header: 'Status',
    width: '110px',
    render: (r) => {
      const s = r.status as FiatOrder['status'];
      const v = s === 'completed' ? 'success' : s === 'failed' ? 'danger' : 'warning';
      return <Badge variant={v} size="sm">{s}</Badge>;
    },
  },
  {
    key: 'created_at',
    header: 'Date',
    render: (r) => new Date(r.created_at as string).toLocaleString(),
  },
];

export function FiatHistory({ orders, loading }: FiatHistoryProps) {
  return (
    <Table<FiatOrderRow>
      columns={COLUMNS}
      data={orders as FiatOrderRow[]}
      loading={loading}
      emptyText="No fiat orders"
    />
  );
}
