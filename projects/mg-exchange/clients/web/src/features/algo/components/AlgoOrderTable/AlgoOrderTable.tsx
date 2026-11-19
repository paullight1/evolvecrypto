import { Badge } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { toast } from '@shared/ui/Toast';
import { cancelAlgo } from '../../api';
import type { AlgoOrder, AlgoStatus } from '../../types';

interface AlgoOrderTableProps {
  orders: AlgoOrder[];
  loading: boolean;
  onCancelled: () => void;
  showActions?: boolean;
}

type AlgoOrderRow = AlgoOrder & Record<string, unknown>;

function statusVariant(s: AlgoStatus): 'success' | 'warning' | 'danger' | 'info' | 'neutral' {
  if (s === 'completed') return 'success';
  if (s === 'running') return 'info';
  if (s === 'paused') return 'warning';
  if (s === 'failed') return 'danger';
  return 'neutral';
}

function progress(order: AlgoOrder): string {
  const total = parseFloat(order.total_qty);
  const exec = parseFloat(order.executed_qty);
  if (!isFinite(total) || total === 0) return '0%';
  return `${((exec / total) * 100).toFixed(1)}%`;
}

function buildColumns(showActions: boolean, onCancelled: () => void): ColumnDef<AlgoOrderRow>[] {
  const cols: ColumnDef<AlgoOrderRow>[] = [
    { key: 'type', header: 'Type', width: '80px', render: (r) => (r.type as string).toUpperCase() },
    { key: 'symbol', header: 'Symbol', width: '110px' },
    { key: 'side', header: 'Side', width: '70px' },
    { key: 'total_qty', header: 'Qty', align: 'right' },
    {
      key: 'progress',
      header: 'Progress',
      align: 'right',
      render: (r) => progress(r as AlgoOrder),
    },
    {
      key: 'status',
      header: 'Status',
      width: '110px',
      render: (r) => {
        const v = statusVariant(r.status as AlgoStatus);
        return <Badge variant={v} size="sm">{r.status as string}</Badge>;
      },
    },
    { key: 'created_at', header: 'Created', render: (r) => new Date(r.created_at as string).toLocaleString() },
  ];

  if (showActions) {
    cols.push({
      key: 'actions',
      header: '',
      width: '80px',
      render: (r) => {
        const order = r as AlgoOrder;
        if (order.status !== 'running' && order.status !== 'paused') return null;
        return (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              cancelAlgo(order.id)
                .then(() => { toast.success('Algo cancelled'); onCancelled(); })
                .catch((e: unknown) => toast.error(e instanceof Error ? e.message : 'Cancel failed'));
            }}
          >
            Cancel
          </Button>
        );
      },
    });
  }

  return cols;
}

export function AlgoOrderTable({ orders, loading, onCancelled, showActions = false }: AlgoOrderTableProps) {
  const columns = buildColumns(showActions, onCancelled);
  return (
    <Table<AlgoOrderRow>
      columns={columns}
      data={orders as AlgoOrderRow[]}
      loading={loading}
      emptyText="No algo orders"
    />
  );
}
