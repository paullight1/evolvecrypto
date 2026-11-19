import { Table, type ColumnDef } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { approveWithdrawal, rejectWithdrawal } from '../api';
import type { PendingWithdrawal } from '../types';

interface WdRow extends Record<string, unknown> {
  id: string;
  user_id: string;
  asset: string;
  amount: string;
  address: string;
  network: string;
  created_at: number;
}

interface Props {
  withdrawals: PendingWithdrawal[];
  loading: boolean;
  onRefresh: () => void;
}

export function WithdrawalsTable({ withdrawals, loading, onRefresh }: Props) {
  async function handleApprove(id: string) {
    try {
      await approveWithdrawal(id);
      toast.success('Withdrawal approved');
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    }
  }

  async function handleReject(id: string) {
    try {
      await rejectWithdrawal(id, 'Rejected by admin');
      toast.success('Withdrawal rejected');
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    }
  }

  const COLUMNS: ColumnDef<WdRow>[] = [
    { key: 'user_id', header: 'User', width: '110px' },
    { key: 'asset', header: 'Asset', width: '80px' },
    { key: 'amount', header: 'Amount', align: 'right' },
    { key: 'network', header: 'Network', width: '90px' },
    { key: 'address', header: 'Address', render: (r) => <span title={String(r.address)}>{String(r.address).slice(0, 16)}…</span> },
    { key: 'created_at', header: 'Date', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
    {
      key: 'id',
      header: 'Actions',
      render: (r) => (
        <div style={{ display: 'flex', gap: '6px' }}>
          <Button variant="primary" size="sm" onClick={() => { void handleApprove(String(r.id)); }}>Approve</Button>
          <Button variant="danger" size="sm" onClick={() => { void handleReject(String(r.id)); }}>Reject</Button>
        </div>
      ),
    },
  ];

  const rows: WdRow[] = withdrawals.map((w) => ({
    id: w.id,
    user_id: w.user_id,
    asset: w.asset,
    amount: w.amount,
    address: w.address,
    network: w.network,
    created_at: w.created_at,
  }));

  return <Table<WdRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No pending withdrawals" />;
}
