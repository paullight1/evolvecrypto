import { useState } from 'react';
import { Modal } from '@shared/ui/Modal';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { earnSubscribe } from '../../api';
import type { EarnProduct } from '../../types';
import styles from './SubscribeModal.module.css';

interface SubscribeModalProps {
  product: EarnProduct | null;
  onClose: () => void;
  onSuccess: () => void;
}

function estimateEarnings(amount: string, apy: string, days = 30): string {
  const amountNum = parseFloat(amount);
  const apyNum = parseFloat(apy.replace('%', ''));
  if (!isFinite(amountNum) || !isFinite(apyNum) || amountNum <= 0) return '0.00';
  return ((amountNum * apyNum) / 100 / 365 * days).toFixed(6);
}

export function SubscribeModal({ product, onClose, onSuccess }: SubscribeModalProps) {
  const [amount, setAmount] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  function handleClose() {
    setAmount('');
    setError('');
    onClose();
  }

  async function handleSubmit() {
    if (!product) return;

    const numAmount = parseFloat(amount);
    const minAmount = parseFloat(product.minAmount);
    if (!amount.trim() || !isFinite(numAmount) || numAmount <= 0) {
      setError('Enter a valid amount');
      return;
    }
    if (numAmount < minAmount) {
      setError(`Minimum amount is ${product.minAmount} ${product.coin}`);
      return;
    }

    setError('');
    setLoading(true);
    try {
      await earnSubscribe({ productId: product.id, amount });
      toast.success(`Subscribed ${amount} ${product.coin}`);
      onSuccess();
      handleClose();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Subscription failed');
    } finally {
      setLoading(false);
    }
  }

  const estimated = product ? estimateEarnings(amount, product.apy) : '0.00';

  return (
    <Modal open={product !== null} onClose={handleClose} title="Subscribe to Earn">
      {product && (
        <div className={styles.content}>
          <div className={styles.productInfo}>
            <div className={styles.infoRow}>
              <span className={styles.label}>Product</span>
              <span className={styles.value}>{product.coin} — {product.productType}</span>
            </div>
            <div className={styles.infoRow}>
              <span className={styles.label}>APY</span>
              <span className={styles.apyValue}>{product.apy}</span>
            </div>
            {product.lockDays > 0 && (
              <div className={styles.infoRow}>
                <span className={styles.label}>Lock Period</span>
                <span className={styles.value}>{product.lockDays} days</span>
              </div>
            )}
          </div>

          <NumberInput
            label={`Amount (${product.coin})`}
            min={0}
            placeholder={`Min. ${product.minAmount}`}
            value={amount}
            onChange={(v) => { setAmount(v); setError(''); }}
            error={error}
          />

          <div className={styles.estimate}>
            <span className={styles.estimateLabel}>Est. earnings (30 days)</span>
            <span className={styles.estimateValue}>
              {estimated} {product.coin}
            </span>
          </div>

          <Button
            variant="primary"
            fullWidth
            loading={loading}
            onClick={() => { void handleSubmit(); }}
          >
            Confirm Subscribe
          </Button>
        </div>
      )}
    </Modal>
  );
}
