import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import type { MasterTrader } from '../../types';

interface LeaderboardRow extends Record<string, unknown> {
  user_id: string;
  display_name: string;
  total_pnl: string;
  win_rate: string;
  followers_count: number;
}

function pnlVariant(value: string): BadgeVariant {
  const num = parseFloat(value);
  if (num > 0) return 'success';
  if (num < 0) return 'danger';
  return 'neutral';
}

interface LeaderboardTableProps {
  traders: MasterTrader[];
  loading?: boolean;
  onFollow: (trader: MasterTrader) => void;
}

export function LeaderboardTable({ traders, loading = false, onFollow }: LeaderboardTableProps) {
  const columns: ColumnDef<LeaderboardRow>[] = [
    { key: 'display_name', header: 'Trader', sortable: true },
    {
      key: 'total_pnl',
      header: 'Total PnL',
      sortable: true,
      align: 'right',
      render: (row) => (
        <Badge variant={pnlVariant(String(row.total_pnl))}>
          {parseFloat(String(row.total_pnl)) >= 0 ? '+' : ''}{String(row.total_pnl)} USDT
        </Badge>
      ),
    },
    {
      key: 'win_rate',
      header: 'Win Rate',
      sortable: true,
      align: 'right',
      render: (row) => (
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--text-sm)' }}>
          {String(row.win_rate)}%
        </span>
      ),
    },
    { key: 'followers_count', header: 'Followers', sortable: true, align: 'right' },
    {
      key: '_action',
      header: '',
      align: 'right',
      render: (row) => (
        <Button
          variant="primary"
          size="sm"
          onClick={() => {
            const trader: MasterTrader = {
              user_id: String(row.user_id),
              display_name: String(row.display_name),
              total_pnl: String(row.total_pnl),
              win_rate: String(row.win_rate),
              followers_count: row.followers_count as number,
              profit_share_rate: '0',
              is_active: true,
            };
            onFollow(trader);
          }}
        >
          Follow
        </Button>
      ),
    },
  ];

  const rows: LeaderboardRow[] = traders.map((t) => ({
    user_id: t.user_id,
    display_name: t.display_name,
    total_pnl: t.total_pnl,
    win_rate: t.win_rate,
    followers_count: t.followers_count,
  }));

  return (
    <Table<LeaderboardRow>
      columns={columns}
      data={rows}
      loading={loading}
      emptyText="No traders available"
    />
  );
}
