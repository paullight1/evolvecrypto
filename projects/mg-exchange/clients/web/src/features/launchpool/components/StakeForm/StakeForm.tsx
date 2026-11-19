import { useState, type FormEvent } from 'react';
import { Modal } from '@shared/ui/Modal';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { stakeInPool } from '../../api';
import type { LaunchpoolPool } from '../../types';
import styles from './StakeForm.module.css';

interface StakeFormProps {
  pool: LaunchpoolPool | null;
  onClose: () => void;
  onSuccess: () => void;
}

export function StakeForm({ pool, onClose, onSuccess }: StakeFormProps) {
  const [amount, setAmount] = useState('');
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!pool) return;
    const n = parseFloat(amount);
    if (!isFinite(n) || n <= 0) { toast.error('Enter a valid amount'); return; }
    setSubmitting(true);
    try {
      await stakeInPool({ pool_id: pool.id, amount });
      toast.success(`Staked ${amount} ${pool.stake_token}`);
      setAmount('');
      onSuccess();
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Stake failed');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={pool !== null} onClose={onClose} title={pool ? `Stake ${pool.stake_token}` : 'Stake'}>
      {pool && (
        <form className={styles.form} onSubmit={(e) => { void handleSubmit(e); }}>
          <div className={styles.info}>
            <div className={styles.infoRow}>
              <span>Reward Token</span>
              <strong>{pool.reward_token}</strong>
            </div>
            <div className={styles.infoRow}>
              <span>APY</span>
              <strong>{pool.apy}</strong>
            </div>
            <div className={styles.infoRow}>
              <span>Max Stake</span>
              <strong>{parseFloat(pool.max_stake).toLocaleString()} {pool.stake_token}</strong>
            </div>
          </div>
          <NumberInput
            label={`Amount (${pool.stake_token})`}
            min={0}
            placeholder="0.00"
            value={amount}
            onChange={(v) => setAmount(v)}
          />
          <Button type="submit" variant="primary" fullWidth loading={submitting}>
            Stake
          </Button>
        </form>
      )}
    </Modal>
  );
}
