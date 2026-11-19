import { useState, useEffect } from 'react';
import { Input } from '@shared/ui/Input';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { toast } from '@shared/ui/Toast';
import { getFiatMethods, getFiatRate, placeFiatOrder } from '../../api';
import type { FiatMethod, FiatRate } from '../../types';
import styles from './BuyCryptoForm.module.css';

const FIAT_OPTIONS: SelectOption[] = [
  { value: 'USD', label: 'USD' },
  { value: 'EUR', label: 'EUR' },
  { value: 'GBP', label: 'GBP' },
];

const CRYPTO_OPTIONS: SelectOption[] = [
  { value: 'BTC', label: 'BTC' },
  { value: 'ETH', label: 'ETH' },
  { value: 'USDT', label: 'USDT' },
  { value: 'SOL', label: 'SOL' },
];

interface BuyCryptoFormProps {
  mode: 'buy' | 'sell';
  onSuccess: () => void;
}

export function BuyCryptoForm({ mode, onSuccess }: BuyCryptoFormProps) {
  const [fiatCurrency, setFiatCurrency] = useState('USD');
  const [cryptoAsset, setCryptoAsset] = useState('BTC');
  const [fiatAmount, setFiatAmount] = useState('');
  const [methods, setMethods] = useState<FiatMethod[]>([]);
  const [selectedMethod, setSelectedMethod] = useState('');
  const [rate, setRate] = useState<FiatRate | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    getFiatMethods()
      .then((m) => {
        setMethods(m);
        setSelectedMethod(m[0]?.id ?? '');
      })
      .catch(() => setMethods([]));
  }, []);

  useEffect(() => {
    if (!fiatCurrency || !cryptoAsset) return;
    getFiatRate(fiatCurrency, cryptoAsset)
      .then(setRate)
      .catch(() => setRate(null));
  }, [fiatCurrency, cryptoAsset]);

  const methodOptions: SelectOption[] = methods.map((m) => ({
    value: m.id,
    label: `${m.name} (fee: ${m.fee_pct}%)`,
  }));

  const estimatedCrypto = rate && fiatAmount
    ? (parseFloat(fiatAmount) / parseFloat(rate.rate)).toFixed(8)
    : null;

  async function handleSubmit() {
    if (!fiatAmount || !selectedMethod) { toast.error('Fill in all fields'); return; }
    const amt = parseFloat(fiatAmount);
    if (!isFinite(amt) || amt <= 0) { toast.error('Enter a valid amount'); return; }
    setLoading(true);
    try {
      await placeFiatOrder({
        type: mode,
        fiat_currency: fiatCurrency,
        crypto_asset: cryptoAsset,
        fiat_amount: fiatAmount,
        payment_method_id: selectedMethod,
      });
      toast.success(`${mode === 'buy' ? 'Buy' : 'Sell'} order placed`);
      setFiatAmount('');
      onSuccess();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Order failed');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className={styles.container}>
      <div className={styles.row}>
        <Select label="Fiat Currency" options={FIAT_OPTIONS} value={fiatCurrency} onChange={(e) => setFiatCurrency(e.target.value)} />
        <Select label="Crypto Asset" options={CRYPTO_OPTIONS} value={cryptoAsset} onChange={(e) => setCryptoAsset(e.target.value)} />
      </div>

      <Input
        label={`Amount (${fiatCurrency})`}
        type="number"
        min="0"
        step="any"
        placeholder="100.00"
        value={fiatAmount}
        onChange={(e) => setFiatAmount(e.target.value)}
      />

      {methodOptions.length > 0 && (
        <Select
          label="Payment Method"
          options={methodOptions}
          value={selectedMethod}
          onChange={(e) => setSelectedMethod(e.target.value)}
        />
      )}

      {rate && (
        <Card padding="sm" className={styles.rateCard}>
          <div className={styles.rateRow}>
            <span className={styles.rateLabel}>Rate</span>
            <span className={styles.rateValue}>1 {cryptoAsset} = {parseFloat(rate.rate).toLocaleString()} {fiatCurrency}</span>
          </div>
          {estimatedCrypto && (
            <div className={styles.rateRow}>
              <span className={styles.rateLabel}>You {mode === 'buy' ? 'receive' : 'sell'}</span>
              <span className={styles.rateValue}>{estimatedCrypto} {cryptoAsset}</span>
            </div>
          )}
        </Card>
      )}

      <Button variant="primary" fullWidth loading={loading} onClick={() => { void handleSubmit(); }}>
        {mode === 'buy' ? `Buy ${cryptoAsset}` : `Sell ${cryptoAsset}`}
      </Button>
    </div>
  );
}
