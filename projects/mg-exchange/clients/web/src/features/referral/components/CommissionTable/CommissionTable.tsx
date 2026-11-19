import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Table, type ColumnDef } from '@shared/ui/Table';
import type { ReferralCommission } from '../../types';

interface CommissionTableProps {
  commissions: ReferralCommission[];
  loading: boolean;
}

function sourceLabel(source: ReferralCommission['source']): string {
  const labels: Record<ReferralCommission['source'], string> = {
    spot: 'Spot', futures: 'Futures', earn: 'Earn',
  };
  return labels[source];
}

function statusVariant(status: ReferralCommission['status']): BadgeVariant {
  return status === 'paid' ? 'success' : 'warning';
}

export function CommissionTable({ commissions, loading }: CommissionTableProps) {
  const columns: ColumnDef<Record<string, unknown>>[] = [
    { key: 'referral_nickname', header: 'Referral' },
    { key: 'amount_usdt', header: 'Commission (USDT)', align: 'right' },
    {
      key: 'source',
      header: 'Source',
      render: (row) => {
        const c = row as unknown as ReferralCommission;
        return <Badge variant="info" size="sm">{sourceLabel(c.source)}</Badge>;
      },
    },
    {
      key: 'status',
      header: 'Status',
      render: (row) => {
        const c = row as unknown as ReferralCommission;
        return <Badge variant={statusVariant(c.status)} size="sm">{c.status}</Badge>;
      },
    },
    {
      key: 'created_at',
      header: 'Date',
      render: (row) => new Date((row.created_at as number) * 1000).toLocaleDateString(),
    },
  ];

  return (
    <Table
      columns={columns}
      data={commissions as unknown as Record<string, unknown>[]}
      loading={loading}
      emptyText="No commissions yet"
    />
  );
}
