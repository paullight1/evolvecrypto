import { useState } from 'react';
import { Modal } from '@shared/ui/Modal';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { followTrader } from '../../api';
import type { MasterTrader } from '../../types';
import styles from './FollowModal.module.css';

interface FollowModalProps {
  trader: MasterTrader | null;
  onClose: () => void;
  onSuccess: () => void;
}

interface FormErrors {
  allocation?: string;
  stop_loss_pct?: string;
}

export function FollowModal({ trader, onClose, onSuccess }: FollowModalProps) {
  const [allocation, setAllocation] = useState('');
  const [stopLossPct, setStopLossPct] = useState('');
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});

  function handleClose() {
    setAllocation('');
    setStopLossPct('');
    setErrors({});
    onClose();
  }

  function validate(): boolean {
    const errs: FormErrors = {};
    const alloc = parseFloat(allocation);
    if (!isFinite(alloc) || alloc <= 0) errs.allocation = 'Enter a valid allocation amount';

    if (stopLossPct.trim() !== '') {
      const sl = parseFloat(stopLossPct);
      if (!isFinite(sl) || sl <= 0 || sl > 100) errs.stop_loss_pct = 'Stop loss must be between 0 and 100';
    }

    setErrors(errs);
    return Object.keys(errs).length === 0;
  }

  async function handleSubmit() {
    if (!trader || !validate()) return;
    setLoading(true);
    try {
      await followTrader({
        master_id: trader.user_id,
        allocation,
        stop_loss_pct: stopLossPct.trim() !== '' ? stopLossPct : undefined,
      });
      toast.success(`Now following ${trader.display_name}`);
      onSuccess();
      handleClose();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to follow trader');
    } finally {
      setLoading(false);
    }
  }

  return (
    <Modal open={trader !== null} onClose={handleClose} title="Follow Trader">
      {trader && (
        <div className={styles.content}>
          <div className={styles.traderInfo}>
            <div className={styles.infoRow}>
              <span className={styles.label}>Trader</span>
              <span className={styles.value}>{trader.display_name}</span>
            </div>
            <div className={styles.infoRow}>
              <span className={styles.label}>ROI 30d</span>
              <span className={styles.value}>N/A</span>
            </div>
            <div className={styles.infoRow}>
              <span className={styles.label}>Win Rate</span>
              <span className={styles.value}>{trader.win_rate}%</span>
            </div>
            <div className={styles.infoRow}>
              <span className={styles.label}>Max Drawdown</span>
              <span className={styles.value}>N/A</span>
            </div>
          </div>

          <NumberInput
            label="Allocation (USDT)"
            min={0}
            placeholder="e.g. 1000"
            value={allocation}
            onChange={(v) => { setAllocation(v); setErrors((p) => ({ ...p, allocation: undefined })); }}
            error={errors.allocation}
          />

          <NumberInput
            label="Stop Loss % (optional)"
            placeholder="e.g. 20"
            value={stopLossPct}
            onChange={(v) => { setStopLossPct(v); setErrors((p) => ({ ...p, stop_loss_pct: undefined })); }}
            error={errors.stop_loss_pct}
          />

          <Button
            variant="primary"
            fullWidth
            loading={loading}
            onClick={() => { void handleSubmit(); }}
          >
            Confirm Follow
          </Button>
        </div>
      )}
    </Modal>
  );
}
