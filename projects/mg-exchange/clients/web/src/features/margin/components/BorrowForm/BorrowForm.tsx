import { useState, useEffect } from 'react';
import { Select } from '@shared/ui/Select';
import { NumberInput } from '@shared/ui/NumberInput';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { getBorrowableAssets, borrowMargin } from '../../api';
import type { BorrowableAsset } from '../../types';
import styles from './BorrowForm.module.css';

export function BorrowForm() {
  const [assets, setAssets] = useState<BorrowableAsset[]>([]);
  const [asset, setAsset] = useState('');
  const [amount, setAmount] = useState('');
  const [loading, setLoading] = useState(false);
  const [fetchingAssets, setFetchingAssets] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    setFetchingAssets(true);
    getBorrowableAssets()
      .then((data) => {
        setAssets(data);
        if (data.length > 0) setAsset(data[0].asset);
      })
      .catch(() => setAssets([]))
      .finally(() => setFetchingAssets(false));
  }, []);

  const selected = assets.find((a) => a.asset === asset);
  const assetOptions = assets.map((a) => ({ value: a.asset, label: a.asset }));

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!asset || !amount || parseFloat(amount) <= 0) {
      setError('Enter a valid amount');
      return;
    }
    setError('');
    setLoading(true);
    try {
      await borrowMargin({ asset, amount });
      setAmount('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Borrow failed');
    } finally {
      setLoading(false);
    }
  }

  return (
    <Card className={styles.card}>
      <h3 className={styles.title}>Borrow</h3>
      <form onSubmit={handleSubmit} className={styles.form}>
        <Select
          label="Asset"
          options={assetOptions}
          value={asset}
          onChange={(e) => setAsset(e.target.value)}
          disabled={fetchingAssets}
        />
        {selected && (
          <div className={styles.rateInfo}>
            <span className={styles.rateLabel}>Hourly rate:</span>
            <span className={styles.rateValue}>{selected.hourly_rate}%</span>
            <span className={styles.rateLabel}>Max borrow:</span>
            <span className={styles.rateValue}>{selected.max_borrow} {selected.asset}</span>
          </div>
        )}
        <NumberInput
          label="Amount"
          min={0}
          step={0.00000001}
          precision={8}
          placeholder="0.00"
          value={amount}
          onChange={(v) => setAmount(v)}
          suffix={<span className={styles.assetSuffix}>{asset}</span>}
        />
        {error && <p className={styles.error}>{error}</p>}
        <Button type="submit" variant="primary" fullWidth loading={loading} disabled={fetchingAssets}>
          Borrow
        </Button>
      </form>
    </Card>
  );
}
