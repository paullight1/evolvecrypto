import { useState, useEffect } from 'react';
import { Badge } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Select } from '@shared/ui/Select';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Modal } from '@shared/ui/Modal';
import { NumberInput } from '@shared/ui/NumberInput';
import { toast } from '@shared/ui/Toast';
import { getP2PAds, createP2POrder } from '../../api';
import type { P2PAd, P2PPaymentMethod } from '../../types';
import styles from './AdList.module.css';

const ASSETS = ['USDT', 'BTC', 'ETH', 'USDC'];
const FIATS = ['USD', 'EUR', 'GBP', 'UAH', 'PLN'];
const PAYMENT_LABELS: Record<P2PPaymentMethod, string> = {
  bank_transfer: 'Bank Transfer',
  paypal: 'PayPal',
  sepa: 'SEPA',
  cash_app: 'Cash App',
  zelle: 'Zelle',
  wise: 'Wise',
};

interface AdListProps {
  side: 'buy' | 'sell';
}

export function AdList({ side }: AdListProps) {
  const [ads, setAds] = useState<P2PAd[]>([]);
  const [loading, setLoading] = useState(false);
  const [asset, setAsset] = useState('USDT');
  const [fiat, setFiat] = useState('USD');
  const [selected, setSelected] = useState<P2PAd | null>(null);
  const [amount, setAmount] = useState('');
  const [placing, setPlacing] = useState(false);

  function load() {
    setLoading(true);
    getP2PAds(side, asset, fiat)
      .then(setAds)
      .catch(() => setAds([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { load(); }, [side, asset, fiat]); // eslint-disable-line react-hooks/exhaustive-deps

  async function handlePlaceOrder() {
    if (!selected) return;
    setPlacing(true);
    try {
      await createP2POrder({
        ad_id: selected.id,
        amount,
      });
      toast.success('Order created successfully');
      setSelected(null);
      setAmount('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create order');
    } finally {
      setPlacing(false);
    }
  }

  const columns: ColumnDef<Record<string, unknown>>[] = [
    {
      key: 'user_id',
      header: 'Trader',
      render: (row) => {
        const ad = row as unknown as P2PAd;
        return (
          <div className={styles.trader}>
            <span className={styles.avatar}>{ad.user_id.slice(0, 2).toUpperCase()}</span>
            <div>
              <div className={styles.traderName}>{ad.user_id}</div>
            </div>
          </div>
        );
      },
    },
    { key: 'price', header: 'Price', align: 'right', sortable: true },
    {
      key: 'min_amount',
      header: 'Limit',
      align: 'right',
      render: (row) => {
        const ad = row as unknown as P2PAd;
        return `${ad.min_amount} – ${ad.max_amount} ${ad.fiat_currency}`;
      },
    },
    {
      key: 'payment_methods',
      header: 'Payment',
      render: (row) => {
        const ad = row as unknown as P2PAd;
        return (
          <div className={styles.methods}>
            {ad.payment_methods.map((m) => (
              <Badge key={m} variant="info" size="sm">{PAYMENT_LABELS[m]}</Badge>
            ))}
          </div>
        );
      },
    },
    {
      key: 'id',
      header: 'Action',
      render: (row) => {
        const ad = row as unknown as P2PAd;
        return (
          <Button variant="primary" size="sm" onClick={() => setSelected(ad)}>
            {side === 'buy' ? 'Buy' : 'Sell'}
          </Button>
        );
      },
    },
  ];

  return (
    <div className={styles.wrap}>
      <div className={styles.filters}>
        <Select
          label="Asset"
          value={asset}
          onChange={(e) => setAsset(e.target.value)}
          options={ASSETS.map((a) => ({ value: a, label: a }))}
          size="sm"
        />
        <Select
          label="Fiat"
          value={fiat}
          onChange={(e) => setFiat(e.target.value)}
          options={FIATS.map((f) => ({ value: f, label: f }))}
          size="sm"
        />
      </div>

      <Table
        columns={columns}
        data={ads as unknown as Record<string, unknown>[]}
        loading={loading}
        emptyText="No ads available"
      />

      <Modal open={selected !== null} onClose={() => setSelected(null)} title="Place Order">
        {selected && (
          <div className={styles.orderForm}>
            <p className={styles.orderInfo}>
              Price: <strong>{selected.price} {selected.fiat_currency}</strong> per {selected.asset}
            </p>
            <p className={styles.orderInfo}>
              Limit: {selected.min_amount} – {selected.max_amount} {selected.fiat_currency}
            </p>
            <NumberInput
              label={`Amount (${selected.asset})`}
              min={0}
              value={amount}
              onChange={(v) => setAmount(v)}
              placeholder="0.00"
            />
            <Button
              variant="primary"
              fullWidth
              loading={placing}
              onClick={() => { void handlePlaceOrder(); }}
            >
              Confirm {side === 'buy' ? 'Buy' : 'Sell'}
            </Button>
          </div>
        )}
      </Modal>
    </div>
  );
}
