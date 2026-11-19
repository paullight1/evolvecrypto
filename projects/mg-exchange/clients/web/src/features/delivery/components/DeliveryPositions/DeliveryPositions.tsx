import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import type { DeliveryPosition } from '../../types';

interface PosRow extends Record<string, unknown> {
  symbol: string;
  side: string;
  size: string;
  entry_price: string;
  mark_price: string;
  unrealized_pnl: string;
  margin: string;
  delivery_date: string;
}

interface DeliveryPositionsProps {
  positions: DeliveryPosition[];
  loading: boolean;
}

const COLUMNS: ColumnDef<PosRow>[] = [
  { key: 'symbol', header: 'Symbol', sortable: true },
  {
    key: 'side',
    header: 'Side',
    render: (row) => (
      <Badge variant={String(row.side).toLowerCase() === 'buy' ? 'long' : 'short'}>
        {String(row.side)}
      </Badge>
    ),
  },
  { key: 'size', header: 'Size', align: 'right' },
  { key: 'entry_price', header: 'Entry Price', align: 'right' },
  { key: 'mark_price', header: 'Mark Price', align: 'right' },
  {
    key: 'unrealized_pnl',
    header: 'Unr. PnL',
    align: 'right',
    render: (row) => {
      const val = parseFloat(String(row.unrealized_pnl));
      const color = val >= 0 ? 'var(--buy)' : 'var(--sell)';
      return <span style={{ color }}>{String(row.unrealized_pnl)}</span>;
    },
  },
  { key: 'margin', header: 'Margin', align: 'right' },
  {
    key: 'delivery_date',
    header: 'Delivery Date',
    render: (row) => new Date(String(row.delivery_date)).toLocaleDateString(),
  },
];

export function DeliveryPositions({ positions, loading }: DeliveryPositionsProps) {
  const rows: PosRow[] = positions.map((p) => ({ ...p }));

  return (
    <Table<PosRow>
      columns={COLUMNS}
      data={rows}
      loading={loading}
      emptyText="No open delivery positions"
    />
  );
}
