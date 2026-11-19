import { Badge } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { toast } from '@shared/ui/Toast';
import { executeRfq } from '../../api';
import type { RfqQuote } from '../../types';

interface QuoteListProps {
  rfqId: string;
  quotes: RfqQuote[];
  loading: boolean;
  onExecuted: () => void;
}

type QuoteRow = RfqQuote & Record<string, unknown>;

function buildColumns(rfqId: string, onExecuted: () => void): ColumnDef<QuoteRow>[] {
  return [
    { key: 'maker', header: 'Maker' },
    { key: 'price', header: 'Price', align: 'right', sortable: true },
    { key: 'qty', header: 'Qty', align: 'right' },
    {
      key: 'notional_usd',
      header: 'Notional',
      align: 'right',
      render: (r) => `$${parseFloat(r.notional_usd as string).toLocaleString()}`,
    },
    {
      key: 'expires_at',
      header: 'Expires',
      render: (r) => new Date(r.expires_at as string).toLocaleTimeString(),
    },
    {
      key: 'status',
      header: 'Status',
      width: '100px',
      render: (r) => {
        const s = r.status as RfqQuote['status'];
        const v = s === 'accepted' ? 'success' : s === 'rejected' || s === 'expired' ? 'danger' : 'warning';
        return <Badge variant={v} size="sm">{s}</Badge>;
      },
    },
    {
      key: 'actions',
      header: '',
      width: '90px',
      render: (r) => {
        const q = r as RfqQuote;
        if (q.status !== 'pending') return null;
        return (
          <Button
            variant="primary"
            size="sm"
            onClick={() => {
              executeRfq(rfqId, q.id)
                .then(() => { toast.success('Trade executed'); onExecuted(); })
                .catch((e: unknown) => toast.error(e instanceof Error ? e.message : 'Execution failed'));
            }}
          >
            Execute
          </Button>
        );
      },
    },
  ];
}

export function QuoteList({ rfqId, quotes, loading, onExecuted }: QuoteListProps) {
  const columns = buildColumns(rfqId, onExecuted);
  return (
    <Table<QuoteRow>
      columns={columns}
      data={quotes as QuoteRow[]}
      loading={loading}
      emptyText="No quotes received yet"
    />
  );
}
