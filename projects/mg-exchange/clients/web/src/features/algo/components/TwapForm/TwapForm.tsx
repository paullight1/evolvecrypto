import { useState } from 'react';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { createTwap } from '../../api';
import styles from './TwapForm.module.css';

const SYMBOL_OPTIONS: SelectOption[] = [
  { value: 'BTCUSDT', label: 'BTC/USDT' },
  { value: 'ETHUSDT', label: 'ETH/USDT' },
  { value: 'SOLUSDT', label: 'SOL/USDT' },
];

const SIDE_OPTIONS: SelectOption[] = [
  { value: 'buy', label: 'Buy' },
  { value: 'sell', label: 'Sell' },
];

interface TwapFormProps {
  onCreated: () => void;
}

export function TwapForm({ onCreated }: TwapFormProps) {
  const [symbol, setSymbol] = useState('BTCUSDT');
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [qty, setQty] = useState('');
  const [durationMinutes, setDurationMinutes] = useState('60');
  const [slices, setSlices] = useState('10');
  const [loading, setLoading] = useState(false);

  async function handleSubmit() {
    const qtyVal = parseFloat(qty);
    const dur = parseInt(durationMinutes, 10);
    const sliceCount = parseInt(slices, 10);
    if (!isFinite(qtyVal) || qtyVal <= 0) { toast.error('Enter a valid quantity'); return; }
    if (!isFinite(dur) || dur <= 0) { toast.error('Enter a valid duration'); return; }
    if (!isFinite(sliceCount) || sliceCount < 2) { toast.error('Slices must be at least 2'); return; }

    setLoading(true);
    try {
      await createTwap({ symbol, side, qty, duration_seconds: dur * 60, slices: sliceCount });
      toast.success('TWAP order created');
      onCreated();
      setQty('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create TWAP');
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
      <NumberInput label="Slices" step={1} placeholder="10" value={slices} onChange={(v) => setSlices(v)} />
      <Button variant="primary" fullWidth loading={loading} onClick={() => { void handleSubmit(); }}>
        Create TWAP Order
      </Button>
    </div>
  );
}
