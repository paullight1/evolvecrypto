import { useState, useEffect } from 'react';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { toast } from '@shared/ui/Toast';
import { getMyOrders, markPaid, releaseOrder, disputeOrder, cancelOrder } from '../../api';
import type { P2POrder, P2POrderStatus } from '../../types';
import styles from './OrderFlow.module.css';

function statusVariant(status: P2POrderStatus): BadgeVariant {
  if (status === 'Released' || status === 'ReleasedToBuyer') return 'success';
  if (status === 'Disputed') return 'danger';
  if (status === 'Cancelled' || status === 'RefundedToSeller') return 'neutral';
  if (status === 'PaymentSent') return 'warning';
  return 'info';
}

function statusLabel(status: P2POrderStatus): string {
  const labels: Record<P2POrderStatus, string> = {
    Created: 'Created',
    PaymentSent: 'Payment Sent',
    Released: 'Released',
    Disputed: 'Disputed',
    Cancelled: 'Cancelled',
    RefundedToSeller: 'Refunded',
    ReleasedToBuyer: 'Released to Buyer',
  };
  return labels[status];
}

export function OrderFlow() {
  const [orders, setOrders] = useState<P2POrder[]>([]);
  const [loading, setLoading] = useState(false);
  const [acting, setActing] = useState<string | null>(null);

  function load() {
    setLoading(true);
    getMyOrders()
      .then(setOrders)
      .catch(() => setOrders([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { load(); }, []);

  async function handleAction(id: string, action: 'paid' | 'release' | 'dispute' | 'cancel') {
    setActing(id);
    try {
      if (action === 'paid') await markPaid(id);
      else if (action === 'release') await releaseOrder(id);
      else if (action === 'dispute') await disputeOrder(id);
      else await cancelOrder(id);
      toast.success('Action completed');
      load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Action failed');
    } finally {
      setActing(null);
    }
  }

  const columns: ColumnDef<Record<string, unknown>>[] = [
    {
      key: 'id',
      header: 'Order ID',
      render: (row) => <span className={styles.orderId}>{String(row.id).slice(0, 8)}…</span>,
    },
    { key: 'asset', header: 'Asset' },
    { key: 'cryptoAmount', header: 'Amount', align: 'right' },
    {
      key: 'fiatAmount',
      header: 'Fiat Amount',
      align: 'right',
      render: (row) => `${String(row.fiatAmount)} ${String(row.fiatCurrency)}`,
    },
    {
      key: 'status',
      header: 'Status',
      render: (row) => {
        const status = row.status as P2POrderStatus;
        return <Badge variant={statusVariant(status)} size="sm">{statusLabel(status)}</Badge>;
      },
    },
    {
      key: 'actions',
      header: 'Actions',
      render: (row) => {
        const order = row as unknown as P2POrder;
        const busy = acting === order.id;
        return (
          <div className={styles.actions}>
            {order.status === 'Created' && order.buyerId === order.id && (
              <Button size="sm" variant="primary" loading={busy} onClick={() => { void handleAction(order.id, 'paid'); }}>
                Mark Paid
              </Button>
            )}
            {order.status === 'PaymentSent' && order.sellerId === order.id && (
              <Button size="sm" variant="primary" loading={busy} onClick={() => { void handleAction(order.id, 'release'); }}>
                Release
              </Button>
            )}
            {(order.status === 'Created' || order.status === 'PaymentSent') && (
              <Button size="sm" variant="ghost" loading={busy} onClick={() => { void handleAction(order.id, 'dispute'); }}>
                Dispute
              </Button>
            )}
            {order.status === 'Created' && (
              <Button size="sm" variant="danger" loading={busy} onClick={() => { void handleAction(order.id, 'cancel'); }}>
                Cancel
              </Button>
            )}
          </div>
        );
      },
    },
  ];

  return (
    <Table
      columns={columns}
      data={orders as unknown as Record<string, unknown>[]}
      loading={loading}
      emptyText="No orders yet"
    />
  );
}
