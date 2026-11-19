import { useState } from 'react';
import { Input } from '@shared/ui/Input';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { toast } from '@shared/ui/Toast';
import { placeSpreadOrder } from '../../api';
import type { SpreadInstrument } from '../../types';
import styles from './SpreadOrderForm.module.css';

const SIDE_OPTIONS: SelectOption[] = [
  { value: 'buy', label: 'Buy Spread' },
  { value: 'sell', label: 'Sell Spread' },
];

const ORDER_TYPE_OPTIONS: SelectOption[] = [
  { value: 'limit', label: 'Limit' },
  { value: 'market', label: 'Market' },
];

interface SpreadOrderFormProps {
  instrument: SpreadInstrument | null;
}

export function SpreadOrderForm({ instrument }: SpreadOrderFormProps) {
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState<'limit' | 'market'>('limit');
  const [qty, setQty] = useState('');
  const [price, setPrice] = useState('');
  const [loading, setLoading] = useState(false);

  if (!instrument) {
    return (
      <div className={styles.placeholder}>
        Select a spread instrument from the table to place an order
      </div>
    );
  }

  async function handleSubmit() {
    if (!instrument) return;
    const qtyVal = parseFloat(qty);
    if (!isFinite(qtyVal) || qtyVal <= 0) { toast.error('Enter a valid quantity'); return; }
    if (orderType === 'limit') {
      const priceVal = parseFloat(price);
      if (!isFinite(priceVal)) { toast.error('Enter a valid price for limit order'); return; }
    }
    setLoading(true);
    try {
      await placeSpreadOrder({
        instrument_id: instrument.id,
        side,
        qty,
        order_type: orderType,
        price: orderType === 'limit' ? price : undefined,
      });
      toast.success('Spread order placed');
      setQty('');
      setPrice('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Order failed');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.container}>
      <Card padding="sm" className={styles.instrumentInfo}>
        <div className={styles.infoRow}>
          <span className={styles.infoLabel}>Spread</span>
          <span className={styles.infoValue}>{instrument.name}</span>
        </div>
        <div className={styles.infoRow}>
          <span className={styles.infoLabel}>Bid / Ask</span>
          <span className={styles.infoValue}>{instrument.bid} / {instrument.ask}</span>
        </div>
      </Card>

      <Select label="Side" options={SIDE_OPTIONS} value={side} onChange={(e) => setSide(e.target.value as 'buy' | 'sell')} />
      <Select label="Order Type" options={ORDER_TYPE_OPTIONS} value={orderType} onChange={(e) => setOrderType(e.target.value as 'limit' | 'market')} />
      <Input label="Quantity" type="number" min="0" step="any" placeholder="0.00" value={qty} onChange={(e) => setQty(e.target.value)} />
      {orderType === 'limit' && (
        <Input label="Price" type="number" step="any" placeholder="0.00" value={price} onChange={(e) => setPrice(e.target.value)} />
      )}
      <Button variant="primary" fullWidth loading={loading} onClick={() => { void handleSubmit(); }}>
        Place Spread Order
      </Button>
    </div>
  );
}
