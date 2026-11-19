import { useState } from 'react';
import { Modal, Button, Checkbox } from '@shared/ui';
import type { OrderType } from '@shared/api/types';
import styles from './OrderConfirmModal.module.css';

export interface OrderConfirmModalProps {
  symbol: string;
  side: 'buy' | 'sell';
  orderType: OrderType;
  price: string;
  qty: string;
  leverage: number;
  isPerp: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  onEnableOneClick: () => void;
}

function formatNotional(price: string, qty: string): string {
  const n = parseFloat(price) * parseFloat(qty);
  if (!isFinite(n)) return '?';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toFixed(2);
}

const ORDER_TYPE_LABELS: Record<OrderType, string> = {
  limit:         'Limit',
  market:        'Market',
  stop_limit:    'Stop-Limit',
  stop_market:   'Stop-Market',
  trailing_stop: 'Trailing Stop',
};

export function OrderConfirmModal({
  symbol,
  side,
  orderType,
  price,
  qty,
  leverage,
  isPerp,
  onConfirm,
  onCancel,
  onEnableOneClick,
}: OrderConfirmModalProps) {
  const [skipConfirm, setSkipConfirm] = useState(false);
  const isBuy = side === 'buy';
  const estCost = orderType !== 'market' && price && qty
    ? formatNotional(price, qty)
    : '—';
  const displayPrice = orderType === 'market' ? 'Market' : (price || '—');

  return (
    <Modal open={true} onClose={onCancel} title="Confirm Order">
      <div className={styles.body}>
        <div className={styles.summary}>
          <div className={styles.row}>
            <span className={styles.rowLabel}>Side</span>
            <span className={isBuy ? styles.buyText : styles.sellText}>
              {isBuy ? (isPerp ? 'Buy / Long' : 'Buy') : (isPerp ? 'Sell / Short' : 'Sell')}
            </span>
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>Symbol</span>
            <span>{symbol}</span>
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>Type</span>
            <span>{ORDER_TYPE_LABELS[orderType]}</span>
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>Price</span>
            <span className={styles.mono}>{displayPrice}</span>
          </div>
          <div className={styles.row}>
            <span className={styles.rowLabel}>Qty</span>
            <span className={styles.mono}>{qty || '—'}</span>
          </div>
          {isPerp && leverage > 1 && (
            <div className={styles.row}>
              <span className={styles.rowLabel}>Leverage</span>
              <span>{leverage}x</span>
            </div>
          )}
          <div className={styles.rowTotal}>
            <span className={styles.rowLabel}>Est. Cost</span>
            <span className={styles.mono}>{estCost} USDT</span>
          </div>
        </div>

        <Checkbox
          checked={skipConfirm}
          onChange={(v) => {
            setSkipConfirm(v);
            if (v) onEnableOneClick();
          }}
          label="Don't show again (enable one-click mode)"
        />

        <div className={styles.footer}>
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant={isBuy ? 'long' : 'short'} onClick={onConfirm}>
            Confirm {isBuy ? 'Buy' : 'Sell'}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
