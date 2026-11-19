import { Table, type ColumnDef } from '@shared/ui/Table';
import type { MarginBorrow } from '../../types';

interface BorrowRow extends Record<string, unknown> {
  asset: string;
  amount: string;
  interest_rate: string;
  interest_accrued: string;
  created_at: number;
}

interface BorrowTableProps {
  borrows: MarginBorrow[];
  loading: boolean;
}

const COLUMNS: ColumnDef<BorrowRow>[] = [
  { key: 'asset', header: 'Asset', width: '80px' },
  { key: 'amount', header: 'Borrowed', align: 'right', sortable: true },
  { key: 'interest_rate', header: 'Rate (hr)', align: 'right' },
  { key: 'interest_accrued', header: 'Interest', align: 'right' },
  {
    key: 'created_at',
    header: 'Since',
    render: (row) => new Date((row.created_at as number) * 1000).toLocaleString(),
  },
];

export function BorrowTable({ borrows, loading }: BorrowTableProps) {
  const rows: BorrowRow[] = borrows.map((b) => ({ ...b }));

  return (
    <Table<BorrowRow>
      columns={COLUMNS}
      data={rows}
      loading={loading}
      emptyText="No active borrows"
    />
  );
}
