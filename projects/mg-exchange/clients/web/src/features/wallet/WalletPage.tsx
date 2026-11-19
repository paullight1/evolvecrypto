import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import { EmptyState } from '@shared/ui/EmptyState';
import { Button } from '@shared/ui/Button';
import { navigate } from '@app/Router';
import { BalanceTable } from './components/BalanceTable';
import { DepositForm } from './components/DepositForm';
import { WithdrawForm } from './components/WithdrawForm';
import { getBalances, getDeposits, getWithdrawals } from './api';
import type { Balance, DepositRecord, WithdrawalRecord } from './types';
import styles from './WalletPage.module.css';

const TABS: TabDef[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'deposit', label: 'Deposit' },
  { key: 'withdraw', label: 'Withdraw' },
  { key: 'history', label: 'History' },
];

type StatusVariant = 'success' | 'warning' | 'danger' | 'neutral';

function txStatusVariant(status: string): StatusVariant {
  if (status === 'completed') return 'success';
  if (status === 'pending') return 'warning';
  if (status === 'failed') return 'danger';
  return 'neutral';
}

interface TxRow extends Record<string, unknown> {
  id: string;
  type: string;
  asset: string;
  amount: string;
  status: string;
  created_at: number;
  tx_hash: string;
}

const HISTORY_COLUMNS: ColumnDef<TxRow>[] = [
  { key: 'type', header: 'Type', width: '100px' },
  { key: 'asset', header: 'Asset', width: '80px' },
  { key: 'amount', header: 'Amount', align: 'right' },
  {
    key: 'status',
    header: 'Status',
    render: (row) => (
      <Badge variant={txStatusVariant(String(row.status))}>
        {String(row.status)}
      </Badge>
    ),
  },
  {
    key: 'created_at',
    header: 'Date',
    render: (row) => new Date((row.created_at as number) * 1000).toLocaleDateString(),
  },
];

export default function WalletPage() {
  const [tab, setTab] = useState('overview');
  const [balances, setBalances] = useState<Balance[]>([]);
  const [balancesLoading, setBalancesLoading] = useState(false);
  const [history, setHistory] = useState<TxRow[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  useEffect(() => {
    setBalancesLoading(true);
    getBalances()
      .then(setBalances)
      .catch(() => setBalances([]))
      .finally(() => setBalancesLoading(false));
  }, []);

  useEffect(() => {
    if (tab !== 'history') return;
    setHistoryLoading(true);
    Promise.all([getDeposits(50), getWithdrawals(50)])
      .then(([deps, wds]) => {
        const depRows: TxRow[] = deps.map((d: DepositRecord) => ({
          id: d.id, type: 'Deposit', asset: d.asset, amount: d.amount,
          status: d.status, created_at: d.created_at, tx_hash: d.tx_hash ?? '',
        }));
        const wdRows: TxRow[] = wds.map((w: WithdrawalRecord) => ({
          id: w.id, type: 'Withdrawal', asset: w.asset, amount: w.amount,
          status: w.status, created_at: w.created_at, tx_hash: '',
        }));
        setHistory([...depRows, ...wdRows].sort((a, b) => b.created_at - a.created_at));
      })
      .catch(() => setHistory([]))
      .finally(() => setHistoryLoading(false));
  }, [tab]);

  const txRows: TxRow[] = history;

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Wallet</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'overview' && (
          <BalanceTable balances={balances} loading={balancesLoading} />
        )}
        {tab === 'deposit' && <DepositForm />}
        {tab === 'withdraw' && <WithdrawForm />}
        {tab === 'history' && (
          <Table<TxRow>
            columns={HISTORY_COLUMNS}
            data={txRows}
            loading={historyLoading}
            emptyNode={
              <EmptyState
                title="No transaction history"
                description="Your deposits and withdrawals will appear here"
                action={<Button size="sm" onClick={() => navigate('trade')}>Start Trading</Button>}
              />
            }
          />
        )}
      </div>
    </div>
  );
}
