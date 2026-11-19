import { Badge } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { formatPercent } from '@shared/utils/format';
import type { LeveragedToken } from '../../types';

interface TokenListProps {
  tokens: LeveragedToken[];
  loading: boolean;
  onPurchase: (token: LeveragedToken) => void;
  onRedeem: (token: LeveragedToken) => void;
}

export function TokenList({ tokens, loading, onPurchase, onRedeem }: TokenListProps) {
  const columns: ColumnDef<Record<string, unknown>>[] = [
    { key: 'symbol', header: 'Symbol', render: (row) => <strong>{String(row.symbol)}</strong> },
    { key: 'underlying', header: 'Underlying' },
    {
      key: 'leverage',
      header: 'Leverage',
      render: (row) => <Badge variant="info" size="sm">{String(row.leverage)}x</Badge>,
    },
    { key: 'nav', header: 'NAV (USDT)', align: 'right', sortable: true },
    {
      key: 'change_24h',
      header: '24h Change',
      align: 'right',
      sortable: true,
      render: (row) => {
        const pct = parseFloat(String(row.change_24h));
        const color = pct > 0 ? 'var(--green)' : pct < 0 ? 'var(--red)' : 'var(--text-1)';
        return <span style={{ color }}>{formatPercent(pct)}</span>;
      },
    },
    { key: 'management_fee', header: 'Mgmt Fee', align: 'right', render: (row) => `${row.management_fee}%` },
    {
      key: 'actions',
      header: 'Actions',
      render: (row) => {
        const token = row as unknown as LeveragedToken;
        return (
          <div style={{ display: 'flex', gap: '4px' }}>
            <Button size="sm" variant="primary" onClick={() => onPurchase(token)}>Buy</Button>
            <Button size="sm" variant="ghost" onClick={() => onRedeem(token)}>Redeem</Button>
          </div>
        );
      },
    },
  ];

  return (
    <Table
      columns={columns}
      data={tokens as unknown as Record<string, unknown>[]}
      loading={loading}
      emptyText="No leveraged tokens available"
    />
  );
}
