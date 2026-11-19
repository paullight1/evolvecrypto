import { Table, type ColumnDef } from '@shared/ui/Table';
import type { BlockTrade } from '../../types';

interface BlockTradeHistoryProps {
  trades: BlockTrade[];
  loading: boolean;
}

type TradeRow = BlockTrade & Record<string, unknown>;

const COLUMNS: ColumnDef<TradeRow>[] = [
  { key: 'symbol', header: 'Symbol', width: '110px' },
  { key: 'side', header: 'Side', width: '70px' },
  { key: 'qty', header: 'Qty', align: 'right' },
  { key: 'price', header: 'Price', align: 'right', sortable: true },
  {
    key: 'notional_usd',
    header: 'Notional',
    align: 'right',
    render: (r) => `$${parseFloat(r.notional_usd as string).toLocaleString()}`,
  },
  { key: 'counterparty', header: 'Counterparty' },
  {
    key: 'executed_at',
    header: 'Date',
    render: (r) => new Date(r.executed_at as string).toLocaleString(),
  },
];

export function BlockTradeHistory({ trades, loading }: BlockTradeHistoryProps) {
  return (
    <Table<TradeRow>
      columns={COLUMNS}
      data={trades as TradeRow[]}
      loading={loading}
      emptyText="No block trades"
    />
  );
}
