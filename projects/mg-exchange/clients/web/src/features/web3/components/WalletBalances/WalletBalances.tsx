import { Table, type ColumnDef } from '@shared/ui/Table';
import type { TokenBalance } from '../../types';

interface WalletBalancesProps {
  balances: TokenBalance[];
  loading: boolean;
}

type BalanceRow = TokenBalance & Record<string, unknown>;

const COLUMNS: ColumnDef<BalanceRow>[] = [
  { key: 'chain', header: 'Chain', width: '120px' },
  { key: 'symbol', header: 'Token', width: '80px' },
  { key: 'balance', header: 'Balance', align: 'right', sortable: true },
  { key: 'usd_value', header: 'USD Value', align: 'right', sortable: true, render: (row) => `$${row.usd_value}` },
];

export function WalletBalances({ balances, loading }: WalletBalancesProps) {
  return (
    <Table<BalanceRow>
      columns={COLUMNS}
      data={balances as BalanceRow[]}
      loading={loading}
      emptyText="No balances found. Connect a Web3 wallet to view balances."
    />
  );
}
