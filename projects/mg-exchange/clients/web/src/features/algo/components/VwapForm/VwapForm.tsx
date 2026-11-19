import { useState } from 'react';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { createVwap } from '../../api';
import styles from './VwapForm.module.css';

const SYMBOL_OPTIONS: SelectOption[] = [
  { value: 'BTCUSDT', label: 'BTC/USDT' },
  { value: 'ETHUSDT', label: 'ETH/USDT' },
  { value: 'SOLUSDT', label: 'SOL/USDT' },
];

const SIDE_OPTIONS: SelectOption[] = [
  { value: 'buy', label: 'Buy' },
  { value: 'sell', label: 'Sell' },
];

interface VwapFormProps {
  onCreated: () => void;
}

export function VwapForm({ onCreated }: VwapFormProps) {
  const [symbol, setSymbol] = useState('BTCUSDT');
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [qty, setQty] = useState('');
  const [durationMinutes, setDurationMinutes] = useState('60');
  const [loading, setLoading] = useState(false);

  async function handleSubmit() {
    const qtyVal = parseFloat(qty);
    const dur = parseInt(durationMinutes, 10);
    if (!isFinite(qtyVal) || qtyVal <= 0) { toast.error('Enter a valid quantity'); return; }
    if (!isFinite(dur) || dur <= 0) { toast.error('Enter a valid duration'); return; }

    setLoading(true);
    try {
      await createVwap({ symbol, side, qty, duration_seconds: dur * 60 });
      toast.success('VWAP order created');
      onCreated();
      setQty('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create VWAP');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.container}>
      <Select label="Symbol" options={SYMBOL_OPTIONS} value={symbol} onChange={(e) => setSymbol(e.target.value)} />
      <Select label="Side" options={SIDE_OPTIONS} value={side} onChange={(e) => setSide(e.target.value as 'buy' | 'sell')} />
      <NumberInput label="Quantity" min={0} placeholder="0.00" value={qty} onChange={(v) => setQty(v)} />
      <NumberInput label="Duration (minutes)" step={1} placeholder="60" value={durationMinutes} onChange={(v) => setDurationMinutes(v)} />
      <Button variant="primary" fullWidth loading={loading} onClick={() => { void handleSubmit(); }}>
        Create VWAP Order
      </Button>
    </div>
  );
}
