import { useState, type FormEvent } from 'react';
import { Modal } from '@shared/ui/Modal';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { commitToProject } from '../../api';
import type { LaunchpadProject } from '../../types';
import styles from './CommitForm.module.css';

interface CommitFormProps {
  project: LaunchpadProject | null;
  onClose: () => void;
  onSuccess: () => void;
}

export function CommitForm({ project, onClose, onSuccess }: CommitFormProps) {
  const [amount, setAmount] = useState('');
  const [submitting, setSubmitting] = useState(false);

  const estimatedTokens = (() => {
    if (!project) return null;
    const n = parseFloat(amount);
    const price = parseFloat(project.token_price_usdt);
    if (!isFinite(n) || !isFinite(price) || price === 0 || n <= 0) return null;
    return (n / price).toFixed(4);
  })();

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!project) return;
    const n = parseFloat(amount);
    if (!isFinite(n) || n <= 0) { toast.error('Enter a valid amount'); return; }
    setSubmitting(true);
    try {
      await commitToProject({ project_id: project.id, amount_usdt: amount, currency: 'USDT' });
      toast.success('Commitment submitted');
      setAmount('');
      onSuccess();
      onClose();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Commitment failed');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={project !== null} onClose={onClose} title={project ? `Commit to ${project.token_name}` : 'Commit'}>
      {project && (
        <form className={styles.form} onSubmit={(e) => { void handleSubmit(e); }}>
          <div className={styles.meta}>
            <div className={styles.metaRow}>
              <span>Token Price</span>
              <strong>${project.token_price_usdt}</strong>
            </div>
            <div className={styles.metaRow}>
              <span>Min / Max</span>
              <strong>${project.min_commitment_usdt} – ${project.max_commitment_usdt}</strong>
            </div>
          </div>

          <NumberInput
            label="Amount (USDT)"
            min={0}
            placeholder="0.00"
            value={amount}
            onChange={(v) => setAmount(v)}
          />

          {estimatedTokens && (
            <p className={styles.estimate}>
              Estimated: <strong>{estimatedTokens} {project.token_symbol}</strong>
            </p>
          )}

          <Button type="submit" variant="primary" fullWidth loading={submitting}>
            Commit
          </Button>
        </form>
      )}
    </Modal>
  );
}
