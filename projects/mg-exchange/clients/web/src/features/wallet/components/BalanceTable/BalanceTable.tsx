import { Table, type ColumnDef } from '@shared/ui/Table';
import type { Balance } from '../../types';

interface BalanceRow extends Record<string, unknown> {
  asset: string;
  available: string;
  locked: string;
  total: string;
}

const COLUMNS: ColumnDef<BalanceRow>[] = [
  { key: 'asset', header: 'Asset', sortable: true, width: '120px' },
  { key: 'total', header: 'Total', sortable: true, align: 'right' },
  { key: 'available', header: 'Available', sortable: true, align: 'right' },
  { key: 'locked', header: 'Locked', sortable: true, align: 'right' },
];

export interface BalanceTableProps {
  balances: Balance[];
  loading?: boolean;
}

export function BalanceTable({ balances, loading = false }: BalanceTableProps) {
  const rows: BalanceRow[] = balances.map((b) => ({
    asset: b.asset,
    available: b.available,
    locked: b.locked,
    total: b.total,
  }));

  return (
    <Table<BalanceRow>
      columns={COLUMNS}
      data={rows}
      loading={loading}
      emptyText="No balances found"
    />
  );
}
