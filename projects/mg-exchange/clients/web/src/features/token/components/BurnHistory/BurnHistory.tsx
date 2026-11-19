import { Badge } from '@shared/ui/Badge';
import { Table, type ColumnDef } from '@shared/ui/Table';
import type { BurnRecord } from '../../types';

interface BurnHistoryProps {
  records: BurnRecord[];
  loading: boolean;
}

function sourceLabel(source: BurnRecord['source']): string {
  const labels: Record<BurnRecord['source'], string> = {
    buyback: 'Buyback', fees: 'Fee Burn', manual: 'Manual',
  };
  return labels[source];
}

export function BurnHistory({ records, loading }: BurnHistoryProps) {
  const columns: ColumnDef<Record<string, unknown>>[] = [
    { key: 'amount', header: 'Amount (EXC)', align: 'right', sortable: true },
    {
      key: 'source',
      header: 'Source',
      render: (row) => {
        const r = row as unknown as BurnRecord;
        return <Badge variant="warning" size="sm">{sourceLabel(r.source)}</Badge>;
      },
    },
    {
      key: 'tx_hash',
      header: 'Tx Hash',
      render: (row) => <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--text-xs)' }}>{String(row.tx_hash).slice(0, 12)}…</span>,
    },
    {
      key: 'burned_at',
      header: 'Date',
      render: (row) => new Date((row.burned_at as number) * 1000).toLocaleDateString(),
    },
  ];

  return (
    <Table
      columns={columns}
      data={records as unknown as Record<string, unknown>[]}
      loading={loading}
      emptyText="No burn records"
    />
  );
}
