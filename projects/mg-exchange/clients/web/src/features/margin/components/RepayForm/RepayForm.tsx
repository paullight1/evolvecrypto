import { useState } from 'react';
import { Select } from '@shared/ui/Select';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { repayMargin } from '../../api';
import type { MarginBorrow } from '../../types';
import styles from './RepayForm.module.css';

interface RepayFormProps {
  borrows: MarginBorrow[];
}

export function RepayForm({ borrows }: RepayFormProps) {
  const uniqueAssets = Array.from(new Set(borrows.map((b) => b.asset)));
  const assetOptions = uniqueAssets.map((a) => ({ value: a, label: a }));

  const [asset, setAsset] = useState(uniqueAssets[0] ?? '');
  const [amount, setAmount] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const activeBorrow = borrows.find((b) => b.asset === asset);

  function handleMax() {
    if (activeBorrow) setAmount(activeBorrow.amount);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!asset || !amount || parseFloat(amount) <= 0) {
      setError('Enter a valid amount');
      return;
    }
    setError('');
    setLoading(true);
    try {
      await repayMargin({ asset, amount });
      setAmount('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Repay failed');
    } finally {
      setLoading(false);
    }
  }

  if (borrows.length === 0) {
    return (
      <Card className={styles.card}>
        <p className={styles.empty}>No active borrows to repay.</p>
      </Card>
    );
  }

  return (
    <Card className={styles.card}>
      <h3 className={styles.title}>Repay</h3>
      <form onSubmit={handleSubmit} className={styles.form}>
        <Select
          label="Asset"
          options={assetOptions}
          value={asset}
          onChange={(e) => setAsset(e.target.value)}
        />
        {activeBorrow && (
          <p className={styles.borrowInfo}>
            Outstanding: {activeBorrow.amount} {asset}
            {' '}(+{activeBorrow.interest_accrued} interest)
          </p>
        )}
        <NumberInput
          label="Amount"
          min={0}
          step={0.00000001}
          precision={8}
          placeholder="0.00"
          value={amount}
          onChange={(v) => setAmount(v)}
          suffix={
            <button type="button" className={styles.maxBtn} onClick={handleMax}>
              Max
            </button>
          }
        />
        {error && <p className={styles.error}>{error}</p>}
        <Button type="submit" variant="primary" fullWidth loading={loading}>
          Repay
        </Button>
      </form>
    </Card>
  );
}
