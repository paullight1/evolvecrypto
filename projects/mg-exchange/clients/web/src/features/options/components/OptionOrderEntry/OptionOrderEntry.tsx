import { useState } from 'react';
import { Button } from '@shared/ui/Button';
import { Input } from '@shared/ui/Input';
import { Select } from '@shared/ui/Select';
import { Card } from '@shared/ui/Card';
import { placeOptionOrder } from '../../api';
import type { PlaceOptionOrderRequest } from '../../types';
import styles from './OptionOrderEntry.module.css';

interface OptionOrderEntryProps {
  symbol: string;
  initialSide?: 'buy' | 'sell';
}

const ORDER_TYPE_OPTIONS = [
  { value: 'limit', label: 'Limit' },
  { value: 'market', label: 'Market' },
];

const SIDE_OPTIONS = [
  { value: 'buy', label: 'Buy' },
  { value: 'sell', label: 'Sell' },
];

export function OptionOrderEntry({ symbol, initialSide = 'buy' }: OptionOrderEntryProps) {
  const [side, setSide] = useState<'buy' | 'sell'>(initialSide);
  const [orderType, setOrderType] = useState<'limit' | 'market'>('limit');
  const [qty, setQty] = useState('');
  const [price, setPrice] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!symbol) {
      setError('Select a symbol from the chain');
      return;
    }
    if (!qty || parseFloat(qty) <= 0) {
      setError('Enter a valid quantity');
      return;
    }
    setError('');
    const req: PlaceOptionOrderRequest = {
      symbol,
      side,
      qty,
      premium: orderType === 'limit' && price ? price : '0',
    };
    setLoading(true);
    try {
      await placeOptionOrder(req);
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
      <h3 className={styles.title}>Place Option Order</h3>
      {symbol && <p className={styles.symbol}>{symbol}</p>}
      <form onSubmit={handleSubmit} className={styles.form}>
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
          step="0.01"
          placeholder="0.00"
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
          {side === 'buy' ? 'Buy' : 'Sell'} Option
        </Button>
      </form>
    </Card>
  );
}
