import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import type { DeliveryInstrument } from '../../types';

interface InstrumentRow extends Record<string, unknown> {
  symbol: string;
  underlying: string;
  delivery_date: string;
  days_remaining: number;
  last_price: string;
  mark_price: string;
  basis: string;
  oi: string;
}

interface InstrumentListProps {
  instruments: DeliveryInstrument[];
  loading: boolean;
  onSelect: (symbol: string) => void;
}

const COLUMNS: ColumnDef<InstrumentRow>[] = [
  { key: 'symbol', header: 'Symbol', sortable: true },
  { key: 'underlying', header: 'Underlying', width: '100px' },
  {
    key: 'delivery_date',
    header: 'Delivery Date',
    render: (row) => new Date(String(row.delivery_date)).toLocaleDateString(),
  },
  {
    key: 'days_remaining',
    header: 'Days Left',
    align: 'right',
    sortable: true,
    render: (row) => {
      const days = Number(row.days_remaining);
      const variant = days <= 7 ? 'warning' : days <= 30 ? 'info' : 'neutral';
      return <Badge variant={variant}>{days}d</Badge>;
    },
  },
  { key: 'last_price', header: 'Last Price', align: 'right' },
  { key: 'mark_price', header: 'Mark Price', align: 'right' },
  {
    key: 'basis',
    header: 'Basis %',
    align: 'right',
    render: (row) => {
      const val = parseFloat(String(row.basis));
      const color = val >= 0 ? 'var(--buy)' : 'var(--sell)';
      return <span style={{ color }}>{String(row.basis)}%</span>;
    },
  },
  { key: 'oi', header: 'Open Interest', align: 'right', sortable: true },
];

export function InstrumentList({ instruments, loading, onSelect }: InstrumentListProps) {
  const rows: InstrumentRow[] = instruments.map((i) => ({ ...i }));

  return (
    <Table<InstrumentRow>
      columns={COLUMNS}
      data={rows}
      loading={loading}
      emptyText="No delivery instruments available"
      onRowClick={(row) => onSelect(String(row.symbol))}
    />
  );
}
