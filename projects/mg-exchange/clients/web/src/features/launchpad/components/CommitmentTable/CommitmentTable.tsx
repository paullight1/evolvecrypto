import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Table, type ColumnDef } from '@shared/ui/Table';
import type { LaunchpadCommitment } from '../../types';

function commitStatusVariant(status: LaunchpadCommitment['status']): BadgeVariant {
  if (status === 'distributed') return 'success';
  if (status === 'refunded') return 'neutral';
  if (status === 'confirmed') return 'info';
  return 'warning';
}

function commitStatusLabel(status: LaunchpadCommitment['status']): string {
  const labels: Record<LaunchpadCommitment['status'], string> = {
    pending: 'Pending',
    confirmed: 'Confirmed',
    distributed: 'Distributed',
    refunded: 'Refunded',
  };
  return labels[status];
}

interface CommitmentTableProps {
  commitments: LaunchpadCommitment[];
  loading: boolean;
}

export function CommitmentTable({ commitments, loading }: CommitmentTableProps) {
  const columns: ColumnDef<Record<string, unknown>>[] = [
    {
      key: 'project_name',
      header: 'Project',
      render: (row) => {
        const c = row as unknown as LaunchpadCommitment;
        return `${c.project_name} (${c.token_symbol})`;
      },
    },
    { key: 'committed_usdt', header: 'Committed (USDT)', align: 'right' },
    {
      key: 'estimated_tokens',
      header: 'Est. Tokens',
      align: 'right',
      render: (row) => {
        const c = row as unknown as LaunchpadCommitment;
        return `${c.estimated_tokens} ${c.token_symbol}`;
      },
    },
    {
      key: 'allocation_received',
      header: 'Received',
      align: 'right',
      render: (row) => {
        const c = row as unknown as LaunchpadCommitment;
        return c.allocation_received ? `${c.allocation_received} ${c.token_symbol}` : '—';
      },
    },
    {
      key: 'claim_date',
      header: 'Claim Date',
      render: (row) => {
        const c = row as unknown as LaunchpadCommitment;
        return c.claim_date ? new Date(c.claim_date).toLocaleDateString() : 'TBA';
      },
    },
    {
      key: 'status',
      header: 'Status',
      render: (row) => {
        const c = row as unknown as LaunchpadCommitment;
        return <Badge variant={commitStatusVariant(c.status)} size="sm">{commitStatusLabel(c.status)}</Badge>;
      },
    },
  ];

  return (
    <Table
      columns={columns}
      data={commitments as unknown as Record<string, unknown>[]}
      loading={loading}
      emptyText="No commitments yet"
    />
  );
}
