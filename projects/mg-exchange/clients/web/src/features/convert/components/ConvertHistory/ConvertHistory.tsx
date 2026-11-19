import { Badge } from '@shared/ui/Badge';
import { Table, type ColumnDef } from '@shared/ui/Table';
import type { ConvertHistoryItem } from '../../types';

interface ConvertHistoryProps {
  items: ConvertHistoryItem[];
  loading: boolean;
}

type ConvertRow = ConvertHistoryItem & Record<string, unknown>;

const COLUMNS: ColumnDef<ConvertRow>[] = [
  { key: 'from_asset', header: 'From', width: '80px' },
  { key: 'to_asset', header: 'To', width: '80px' },
  { key: 'from_amount', header: 'Sold', align: 'right' },
  { key: 'to_amount', header: 'Received', align: 'right' },
  { key: 'rate', header: 'Rate', align: 'right' },
  {
    key: 'status',
    header: 'Status',
    width: '110px',
    render: (r) => {
      const s = r.status as ConvertHistoryItem['status'];
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

export function ConvertHistory({ items, loading }: ConvertHistoryProps) {
  return (
    <Table<ConvertRow>
      columns={COLUMNS}
      data={items as ConvertRow[]}
      loading={loading}
      emptyText="No conversion history"
    />
  );
}
