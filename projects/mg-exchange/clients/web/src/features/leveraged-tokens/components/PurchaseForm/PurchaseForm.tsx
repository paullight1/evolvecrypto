import { useState, type FormEvent } from 'react';
import { Modal } from '@shared/ui/Modal';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { purchaseLt } from '../../api';
import type { LeveragedToken } from '../../types';
import styles from './PurchaseForm.module.css';

interface PurchaseFormProps {
  token: LeveragedToken | null;
  onClose: () => void;
  onSuccess: () => void;
}

export function PurchaseForm({ token, onClose, onSuccess }: PurchaseFormProps) {
  const [amount, setAmount] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const estimated = (() => {
    if (!token) return null;
    const n = parseFloat(amount);
    const nav = parseFloat(token.nav);
    if (!isFinite(n) || !isFinite(nav) || nav === 0 || n <= 0) return null;
    return (n / nav).toFixed(6);
  })();

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!token) return;
    setSubmitting(true);
    try {
      const res = await purchaseLt({ token_id: token.id, usdt_amount: amount });
      toast.success(`Purchased ${res.tokens_received} ${token.symbol}`);
      setAmount('');
      onSuccess();
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Purchase failed');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={token !== null} onClose={onClose} title={token ? `Buy ${token.symbol}` : 'Purchase'}>
      {token && (
        <form className={styles.form} onSubmit={(e) => { void handleSubmit(e); }}>
          <div className={styles.info}>
            <div className={styles.row}><span>NAV</span><strong>${token.nav}</strong></div>
            <div className={styles.row}><span>Leverage</span><strong>{token.leverage}x</strong></div>
            <div className={styles.row}><span>Management Fee</span><strong>{token.management_fee}% daily</strong></div>
          </div>
          <Input
            label="Amount (USDT)"
            type="number"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            placeholder="0.00"
            required
          />
          {estimated && (
            <p className={styles.estimate}>You will receive approximately <strong>{estimated} {token.symbol}</strong></p>
          )}
          <Button type="submit" variant="primary" fullWidth loading={submitting}>Purchase</Button>
        </form>
      )}
    </Modal>
  );
}
