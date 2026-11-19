import { useState } from 'react';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { toast } from '@shared/ui/Toast';
import { createRfq } from '../../api';
import styles from './RfqForm.module.css';

const SYMBOL_OPTIONS: SelectOption[] = [
  { value: 'BTCUSDT', label: 'BTC/USDT' },
  { value: 'ETHUSDT', label: 'ETH/USDT' },
  { value: 'SOLUSDT', label: 'SOL/USDT' },
];

const SIDE_OPTIONS: SelectOption[] = [
  { value: 'buy', label: 'Buy' },
  { value: 'sell', label: 'Sell' },
];

const MIN_NOTIONAL = 100_000;

interface RfqFormProps {
  onCreated: () => void;
}

export function RfqForm({ onCreated }: RfqFormProps) {
  const [symbol, setSymbol] = useState('BTCUSDT');
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [qty, setQty] = useState('');
  const [notional, setNotional] = useState('');
  const [loading, setLoading] = useState(false);

  async function handleSubmit() {
    const qtyVal = parseFloat(qty);
    const notVal = parseFloat(notional);
    if (!isFinite(qtyVal) || qtyVal <= 0) { toast.error('Enter a valid quantity'); return; }
    if (!isFinite(notVal) || notVal < MIN_NOTIONAL) {
      toast.error(`Minimum notional is $${MIN_NOTIONAL.toLocaleString()}`);
      return;
    }
    setLoading(true);
    try {
      await createRfq({ symbol, side, qty, notional_usd: notional });
      toast.success('RFQ submitted — awaiting quotes');
      onCreated();
      setQty('');
      setNotional('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create RFQ');
    } finally {
      setLoading(false);
    }
  }

  const notVal = parseFloat(notional);
  const belowMin = isFinite(notVal) && notVal < MIN_NOTIONAL;

  return (
    <div className={styles.container}>
      <Card padding="sm" className={styles.notice}>
        <span className={styles.noticeText}>Block trading minimum: $100,000 notional value</span>
      </Card>

      <Select label="Symbol" options={SYMBOL_OPTIONS} value={symbol} onChange={(e) => setSymbol(e.target.value)} />
      <Select label="Side" options={SIDE_OPTIONS} value={side} onChange={(e) => setSide(e.target.value as 'buy' | 'sell')} />

      <NumberInput
        label="Quantity"
        min={0}
        placeholder="0.00"
        value={qty}
        onChange={(v) => setQty(v)}
      />

      <NumberInput
        label="Notional (USD)"
        min={100000}
        placeholder="100,000"
        value={notional}
        onChange={(v) => setNotional(v)}
        error={belowMin ? `Minimum $${MIN_NOTIONAL.toLocaleString()}` : undefined}
      />

      <Button variant="primary" fullWidth loading={loading} onClick={() => { void handleSubmit(); }}>
        Submit RFQ
      </Button>
    </div>
  );
}
