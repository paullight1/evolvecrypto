import { Table, type ColumnDef } from '@shared/ui/Table';
import { Card } from '@shared/ui/Card';
import type { Position } from '@features/trading/types';
import styles from '../DashboardPage.module.css';

interface PosRow extends Record<string, unknown> {
  symbol: string;
  side: string;
  size: string;
  entry_price: string;
  unrealized_pnl: string;
}

const COLUMNS: ColumnDef<PosRow>[] = [
  { key: 'symbol', header: 'Symbol', width: '120px' },
  { key: 'side', header: 'Side', width: '70px' },
  { key: 'size', header: 'Size', align: 'right' },
  { key: 'entry_price', header: 'Entry', align: 'right' },
  { key: 'unrealized_pnl', header: 'PnL', align: 'right',
    render: (r) => {
      const pnl = parseFloat(String(r.unrealized_pnl));
      return <span style={{ color: pnl >= 0 ? 'var(--color-buy)' : 'var(--color-sell)' }}>{String(r.unrealized_pnl)}</span>;
    } },
];

interface Props {
  positions: Position[];
  loading: boolean;
}

export function OpenPositionsSummary({ positions, loading }: Props) {
  const rows: PosRow[] = positions.map((p) => ({
    symbol: p.symbol,
    side: p.side,
    size: p.size,
    entry_price: p.entry_price,
    unrealized_pnl: p.unrealized_pnl,
  }));

  return (
    <Card padding="md" className={styles.tableCard}>
      <h2 className={styles.cardTitle}>Open Positions</h2>
      <Table<PosRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No open positions" />
    </Card>
  );
}
