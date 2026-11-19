import { useState, type FormEvent } from 'react';
import { Modal } from '@shared/ui/Modal';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { redeemLt } from '../../api';
import type { LeveragedToken } from '../../types';
import styles from './RedeemForm.module.css';

interface RedeemFormProps {
  token: LeveragedToken | null;
  onClose: () => void;
  onSuccess: () => void;
}

export function RedeemForm({ token, onClose, onSuccess }: RedeemFormProps) {
  const [amount, setAmount] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const estimatedUsdt = (() => {
    if (!token) return null;
    const n = parseFloat(amount);
    const nav = parseFloat(token.nav);
    if (!isFinite(n) || !isFinite(nav) || n <= 0) return null;
    return (n * nav).toFixed(2);
  })();

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!token) return;
    setSubmitting(true);
    try {
      const res = await redeemLt({ token_id: token.id, token_amount: amount });
      toast.success(`Redeemed — received ${res.usdt_received} USDT`);
      setAmount('');
      onSuccess();
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Redeem failed');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={token !== null} onClose={onClose} title={token ? `Redeem ${token.symbol}` : 'Redeem'}>
      {token && (
        <form className={styles.form} onSubmit={(e) => { void handleSubmit(e); }}>
          <div className={styles.info}>
            <div className={styles.row}><span>NAV</span><strong>${token.nav}</strong></div>
          </div>
          <NumberInput
            label={`Amount (${token.symbol})`}
            min={0}
            placeholder="0.00"
            value={amount}
            onChange={(v) => setAmount(v)}
          />
          {estimatedUsdt && (
            <p className={styles.estimate}>You will receive approximately <strong>{estimatedUsdt} USDT</strong></p>
          )}
          <Button type="submit" variant="danger" fullWidth loading={submitting}>Redeem</Button>
        </form>
      )}
    </Modal>
  );
}
