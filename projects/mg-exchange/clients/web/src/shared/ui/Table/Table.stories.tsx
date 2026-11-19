import type { Meta, StoryObj } from '@storybook/react-vite';
import { Table, type ColumnDef } from './Table';
import { Badge } from '../Badge/Badge';

interface TradeRow extends Record<string, unknown> {
  symbol: string;
  side: string;
  price: string;
  qty: string;
  total: string;
  time: string;
}

const TRADE_COLUMNS: ColumnDef<TradeRow>[] = [
  { key: 'symbol', header: 'Symbol', sortable: true, width: '120px' },
  {
    key: 'side',
    header: 'Side',
    width: '80px',
    render: (row) => (
      <Badge variant={row.side === 'Buy' ? 'long' : 'short'} size="sm">
        {row.side}
      </Badge>
    ),
  },
  { key: 'price', header: 'Price',  sortable: true, align: 'right' },
  { key: 'qty',   header: 'Qty',    sortable: true, align: 'right' },
  { key: 'total', header: 'Total',  sortable: true, align: 'right' },
  { key: 'time',  header: 'Time',                   align: 'right' },
];

const TRADE_DATA: TradeRow[] = [
  { symbol: 'BTCUSDT', side: 'Buy',  price: '67,450.00', qty: '0.250',  total: '16,862.50', time: '14:32:01' },
  { symbol: 'ETHUSDT', side: 'Sell', price: '3,520.00',  qty: '1.500',  total: '5,280.00',  time: '14:31:55' },
  { symbol: 'BTCUSDT', side: 'Sell', price: '67,380.00', qty: '0.100',  total: '6,738.00',  time: '14:31:48' },
  { symbol: 'SOLUSDT', side: 'Buy',  price: '185.40',    qty: '10.000', total: '1,854.00',  time: '14:31:40' },
  { symbol: 'BTCUSDT', side: 'Buy',  price: '67,200.00', qty: '0.500',  total: '33,600.00', time: '14:30:12' },
];

// Upcast to the base Record type so story args are compatible with Table's default generic
const columns = TRADE_COLUMNS as ColumnDef<Record<string, unknown>>[];
const data = TRADE_DATA as Record<string, unknown>[];

const meta = {
  title: 'Shared/UI/Table',
  component: Table,
  parameters: {
    layout: 'padded',
  },
} satisfies Meta<typeof Table>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: { columns, data },
};

export const SortableColumns: Story = {
  name: 'Sortable columns',
  args: { columns, data },
  parameters: {
    docs: {
      description: {
        story: 'Click Price, Qty, or Total headers to toggle ascending/descending sort.',
      },
    },
  },
};

export const WithRowClick: Story = {
  name: 'Clickable rows',
  args: {
    columns,
    data,
    onRowClick: (row) => {
      // eslint-disable-next-line no-alert
      alert(`Clicked: ${String(row['symbol'])} ${String(row['side'])} @ ${String(row['price'])}`);
    },
  },
};

export const EmptyState: Story = {
  name: 'Empty state',
  args: { columns, data: [], emptyText: 'No trades found' },
};

export const LoadingState: Story = {
  name: 'Loading state',
  args: { columns, data: [], loading: true },
};
