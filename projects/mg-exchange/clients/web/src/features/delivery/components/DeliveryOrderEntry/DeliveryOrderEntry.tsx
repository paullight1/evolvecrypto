import { useState } from 'react';
import { Button } from '@shared/ui/Button';
import { Input } from '@shared/ui/Input';
import { Select } from '@shared/ui/Select';
import { Card } from '@shared/ui/Card';
import { placeDeliveryOrder } from '../../api';
import type { PlaceDeliveryOrderRequest } from '../../types';
import styles from './DeliveryOrderEntry.module.css';

interface DeliveryOrderEntryProps {
  instruments: Array<{ symbol: string; delivery_date: string }>;
}

const ORDER_TYPE_OPTIONS = [
  { value: 'limit', label: 'Limit' },
  { value: 'market', label: 'Market' },
];

const SIDE_OPTIONS = [
  { value: 'buy', label: 'Buy / Long' },
  { value: 'sell', label: 'Sell / Short' },
];

export function DeliveryOrderEntry({ instruments }: DeliveryOrderEntryProps) {
  const [symbol, setSymbol] = useState(instruments[0]?.symbol ?? '');
  const [side, setSide] = useState<'buy' | 'sell'>('buy');
  const [orderType, setOrderType] = useState<'limit' | 'market'>('limit');
  const [qty, setQty] = useState('');
  const [price, setPrice] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const instrumentOptions = instruments.map((i) => ({
    value: i.symbol,
    label: `${i.symbol} (${new Date(i.delivery_date).toLocaleDateString()})`,
  }));

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!symbol) {
      setError('Select an instrument');
      return;
    }
    if (!qty || parseFloat(qty) <= 0) {
      setError('Enter a valid quantity');
      return;
    }
    setError('');
    const req: PlaceDeliveryOrderRequest = {
      symbol,
      side,
      order_type: orderType,
      qty,
      ...(orderType === 'limit' && price ? { price } : {}),
    };
    setLoading(true);
    try {
      await placeDeliveryOrder(req);
      setQty('');
      setPrice('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Order failed');
    } finally {
      setLoading(false);
    }
  }

  return (
    <Card className={styles.card}>
      <h3 className={styles.title}>Place Delivery Order</h3>
      <form onSubmit={handleSubmit} className={styles.form}>
        {instrumentOptions.length > 0 ? (
          <Select
            label="Instrument"
            options={instrumentOptions}
            value={symbol}
            onChange={(e) => setSymbol(e.target.value)}
          />
        ) : (
          <p className={styles.empty}>No instruments available</p>
        )}
        <Select
          label="Side"
          options={SIDE_OPTIONS}
          value={side}
          onChange={(e) => setSide(e.target.value as 'buy' | 'sell')}
        />
        <Select
          label="Order Type"
          options={ORDER_TYPE_OPTIONS}
          value={orderType}
          onChange={(e) => setOrderType(e.target.value as 'limit' | 'market')}
        />
        <Input
          label="Quantity"
          type="number"
          min="0"
          step="0.001"
          placeholder="0.000"
          value={qty}
          onChange={(e) => setQty(e.target.value)}
        />
        {orderType === 'limit' && (
          <Input
            label="Price"
            type="number"
            min="0"
            step="0.01"
            placeholder="0.00"
            value={price}
            onChange={(e) => setPrice(e.target.value)}
          />
        )}
        {error && <p className={styles.error}>{error}</p>}
        <Button
          type="submit"
          variant={side === 'buy' ? 'long' : 'short'}
          fullWidth
          loading={loading}
          disabled={!symbol}
        >
          {side === 'buy' ? 'Buy Long' : 'Sell Short'}
        </Button>
      </form>
    </Card>
  );
}
