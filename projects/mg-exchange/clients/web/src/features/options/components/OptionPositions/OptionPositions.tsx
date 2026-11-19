import { Table, type ColumnDef } from '@shared/ui/Table';
import { EmptyState } from '@shared/ui/EmptyState';
import type { OptionPosition } from '../../types';

interface PosRow extends Record<string, unknown> {
  symbol: string;
  underlying: string;
  qty: string;
  avg_entry_premium: string;
  mark_price: string;
  unrealized_pnl: string;
  updated_at: string;
}

interface OptionPositionsProps {
  positions: OptionPosition[];
  loading: boolean;
}

const COLUMNS: ColumnDef<PosRow>[] = [
  { key: 'symbol', header: 'Symbol', sortable: true },
  { key: 'underlying', header: 'Underlying' },
  { key: 'qty', header: 'Qty', align: 'right' },
  { key: 'avg_entry_premium', header: 'Entry Premium', align: 'right' },
  { key: 'mark_price', header: 'Mark', align: 'right' },
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
  { key: 'updated_at', header: 'Updated', render: (row) => new Date(String(row.updated_at)).toLocaleDateString() },
];

export function OptionPositions({ positions, loading }: OptionPositionsProps) {
  const rows: PosRow[] = positions.map((p) => ({
    symbol: p.symbol,
    underlying: p.underlying,
    qty: p.qty,
    avg_entry_premium: p.avgEntryPremium,
    mark_price: p.markPrice,
    unrealized_pnl: p.unrealisedPnl,
    updated_at: p.updatedAt,
  }));

  return (
    <Table<PosRow>
      columns={COLUMNS}
      data={rows}
      loading={loading}
      emptyNode={
        <EmptyState
          title="No open option positions"
          description="Buy calls or puts from the Options Chain tab"
        />
      }
    />
  );
}
