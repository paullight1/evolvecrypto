import { useState, type FormEvent } from 'react';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Checkbox } from '@shared/ui/Checkbox';
import { toast } from '@shared/ui/Toast';
import { createAd } from '../../api';
import type { P2PPaymentMethod } from '../../types';
import styles from './CreateAdForm.module.css';

const ASSETS = ['USDT', 'BTC', 'ETH', 'USDC'];
const FIATS = ['USD', 'EUR', 'GBP', 'UAH', 'PLN'];
const ALL_METHODS: { value: P2PPaymentMethod; label: string }[] = [
  { value: 'bank_transfer', label: 'Bank Transfer' },
  { value: 'paypal', label: 'PayPal' },
  { value: 'sepa', label: 'SEPA' },
  { value: 'cash_app', label: 'Cash App' },
  { value: 'zelle', label: 'Zelle' },
  { value: 'wise', label: 'Wise' },
];

interface CreateAdFormProps {
  onCreated: () => void;
}

export function CreateAdForm({ onCreated }: CreateAdFormProps) {
  const [side, setSide] = useState<'buy' | 'sell'>('sell');
  const [asset, setAsset] = useState('USDT');
  const [fiat, setFiat] = useState('USD');
  const [price, setPrice] = useState('');
  const [minAmount, setMinAmount] = useState('');
  const [maxAmount, setMaxAmount] = useState('');
  const [totalAmount, setTotalAmount] = useState('');
  const [methods, setMethods] = useState<P2PPaymentMethod[]>(['bank_transfer']);
  const [submitting, setSubmitting] = useState(false);

  function toggleMethod(method: P2PPaymentMethod) {
    setMethods((prev) =>
      prev.includes(method) ? prev.filter((m) => m !== method) : [...prev, method],
    );
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (methods.length === 0) {
      toast.error('Select at least one payment method');
      return;
    }
    setSubmitting(true);
    try {
      await createAd({ side, asset, fiat_currency: fiat, price, min_amount: minAmount, max_amount: maxAmount, payment_methods: methods });
      toast.success('Ad created');
      onCreated();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to create ad');
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form className={styles.form} onSubmit={(e) => { void handleSubmit(e); }}>
      <div className={styles.row}>
        <Select
          label="Side"
          value={side}
          onChange={(e) => setSide(e.target.value as 'buy' | 'sell')}
          options={[{ value: 'sell', label: 'Sell' }, { value: 'buy', label: 'Buy' }]}
        />
        <Select
          label="Asset"
          value={asset}
          onChange={(e) => setAsset(e.target.value)}
          options={ASSETS.map((a) => ({ value: a, label: a }))}
        />
        <Select
          label="Fiat"
          value={fiat}
          onChange={(e) => setFiat(e.target.value)}
          options={FIATS.map((f) => ({ value: f, label: f }))}
        />
      </div>
      <div className={styles.row}>
        <NumberInput label={`Price (${fiat})`} min={0} value={price} onChange={(v) => setPrice(v)} placeholder="0.00" />
        <NumberInput label={`Total Amount (${asset})`} min={0} value={totalAmount} onChange={(v) => setTotalAmount(v)} placeholder="0.00" />
      </div>
      <div className={styles.row}>
        <NumberInput label={`Min Order (${fiat})`} min={0} value={minAmount} onChange={(v) => setMinAmount(v)} placeholder="0.00" />
        <NumberInput label={`Max Order (${fiat})`} min={0} value={maxAmount} onChange={(v) => setMaxAmount(v)} placeholder="0.00" />
      </div>

      <div>
        <p className={styles.methodsLabel}>Payment Methods</p>
        <div className={styles.methods}>
          {ALL_METHODS.map((m) => (
            <Checkbox
              key={m.value}
              checked={methods.includes(m.value)}
              onChange={() => toggleMethod(m.value)}
              label={m.label}
            />
          ))}
        </div>
      </div>

      <Button type="submit" variant="primary" fullWidth loading={submitting}>
        Post Ad
      </Button>
    </form>
  );
}
