import { Table, type ColumnDef } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { approveKyc, rejectKyc } from '../api';
import type { KycEntry } from '../types';

interface KycRow extends Record<string, unknown> {
  user_id: string;
  email: string;
  level: number;
  country: string;
  submitted_at: number;
}

interface Props {
  entries: KycEntry[];
  loading: boolean;
  onRefresh: () => void;
}

export function KycReviewTable({ entries, loading, onRefresh }: Props) {
  async function handleApprove(userId: string) {
    try {
      await approveKyc(userId, 1);
      toast.success('KYC approved');
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to approve');
    }
  }

  async function handleReject(userId: string) {
    try {
      await rejectKyc(userId, 'Does not meet requirements');
      toast.success('KYC rejected');
      onRefresh();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to reject');
    }
  }

  const COLUMNS: ColumnDef<KycRow>[] = [
    { key: 'email', header: 'Email' },
    { key: 'level', header: 'Level', width: '70px' },
    { key: 'country', header: 'Country', width: '100px' },
    { key: 'submitted_at', header: 'Submitted', render: (r) => new Date((r.submitted_at as number) * 1000).toLocaleDateString() },
    {
      key: 'user_id',
      header: 'Actions',
      render: (r) => (
        <div style={{ display: 'flex', gap: '6px' }}>
          <Button variant="primary" size="sm" onClick={() => { void handleApprove(String(r.user_id)); }}>Approve</Button>
          <Button variant="danger" size="sm" onClick={() => { void handleReject(String(r.user_id)); }}>Reject</Button>
        </div>
      ),
    },
  ];

  const rows: KycRow[] = entries.map((e) => ({
    user_id: e.user_id,
    email: e.email,
    level: e.level,
    country: e.country,
    submitted_at: e.submitted_at,
  }));

  return <Table<KycRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No pending KYC" />;
}
