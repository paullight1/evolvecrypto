import { useState } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { toast } from '@shared/ui/Toast';
import { earnRedeem } from '../../api';
import type { EarnAllocation } from '../../types';

interface AllocationRow extends Record<string, unknown> {
  id: string;
  asset: string;
  amount: string;
  earned_interest: string;
  status: string;
  started_at: string;
}

function statusVariant(status: string): BadgeVariant {
  return status === 'active' ? 'success' : 'neutral';
}

interface AllocationTableProps {
  allocations: EarnAllocation[];
  loading?: boolean;
  onRedeemed: () => void;
}

export function AllocationTable({ allocations, loading = false, onRedeemed }: AllocationTableProps) {
  const [redeemTarget, setRedeemTarget] = useState<string | null>(null);
  const [redeeming, setRedeeming] = useState(false);

  async function handleRedeem() {
    if (!redeemTarget) return;
    setRedeeming(true);
    try {
      await earnRedeem({ allocationId: redeemTarget });
      toast.success('Redeemed successfully');
      onRedeemed();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Redemption failed');
    } finally {
      setRedeeming(false);
      setRedeemTarget(null);
    }
  }

  const columns: ColumnDef<AllocationRow>[] = [
    { key: 'asset', header: 'Asset', width: '100px' },
    { key: 'amount', header: 'Amount', align: 'right' },
    { key: 'earned_interest', header: 'Earned', align: 'right' },
    {
      key: 'status',
      header: 'Status',
      render: (row) => (
        <Badge variant={statusVariant(String(row.status))}>
          {String(row.status)}
        </Badge>
      ),
    },
    {
      key: 'started_at',
      header: 'Date',
      render: (row) => new Date(String(row.started_at)).toLocaleDateString(),
    },
    {
      key: '_action',
      header: '',
      align: 'right',
      render: (row) =>
        row.status === 'active' ? (
          <Button
            variant="secondary"
            size="sm"
            onClick={() => setRedeemTarget(String(row.id))}
          >
            Redeem
          </Button>
        ) : null,
    },
  ];

  const rows: AllocationRow[] = allocations.map((a) => ({
    id: a.allocationId,
    asset: a.coin,
    amount: a.amount,
    earned_interest: a.earnedInterest,
    status: a.status,
    started_at: a.startedAt,
  }));

  return (
    <>
      <Table<AllocationRow>
        columns={columns}
        data={rows}
        loading={loading}
        emptyText="No active allocations"
      />
      <ConfirmDialog
        open={redeemTarget !== null && !redeeming}
        onConfirm={() => { void handleRedeem(); }}
        onCancel={() => setRedeemTarget(null)}
        title="Redeem Allocation"
        message="Redeem this allocation and return funds to your wallet?"
        confirmText="Redeem"
        variant="warning"
      />
    </>
  );
}
