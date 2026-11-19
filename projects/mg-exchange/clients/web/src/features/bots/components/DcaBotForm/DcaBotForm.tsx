import { useState } from 'react';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { createDcaBot } from '../../api';
import styles from './DcaBotForm.module.css';

const SYMBOL_OPTIONS: SelectOption[] = [
  { value: 'BTCUSDT', label: 'BTC/USDT' },
  { value: 'ETHUSDT', label: 'ETH/USDT' },
  { value: 'SOLUSDT', label: 'SOL/USDT' },
  { value: 'BNBUSDT', label: 'BNB/USDT' },
  { value: 'XRPUSDT', label: 'XRP/USDT' },
];

const INTERVAL_OPTIONS: SelectOption[] = [
  { value: '1', label: 'Every hour' },
  { value: '4', label: 'Every 4 hours' },
  { value: '8', label: 'Every 8 hours' },
  { value: '24', label: 'Every day' },
  { value: '168', label: 'Every week' },
];

interface DcaBotFormProps {
  onCreated: () => void;
}

interface FormErrors {
  amount?: string;
  total_orders?: string;
}

export function DcaBotForm({ onCreated }: DcaBotFormProps) {
  const [symbol, setSymbol] = useState('BTCUSDT');
  const [amount, setAmount] = useState('');
  const [intervalHours, setIntervalHours] = useState('24');
  const [totalOrders, setTotalOrders] = useState('10');
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});

  function validate(): boolean {
    const errs: FormErrors = {};
    const amt = parseFloat(amount);
    const orders = parseInt(totalOrders, 10);
    if (!isFinite(amt) || amt <= 0) errs.amount = 'Enter a valid amount';
    if (!isFinite(orders) || orders < 1 || orders > 1000) errs.total_orders = 'Total orders must be between 1 and 1000';
    setErrors(errs);
    return Object.keys(errs).length === 0;
  }

  async function handleSubmit() {
    if (!validate()) return;
    setLoading(true);
    try {
      await createDcaBot({
        symbol,
        amount_per_order: amount,
        interval_secs: parseInt(intervalHours, 10) * 3600,
        total_orders: parseInt(totalOrders, 10),
      });
      toast.success('DCA bot created');
      onCreated();
      setAmount('');
      setTotalOrders('10');
      setErrors({});
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create bot');
    } finally {
      setLoading(false);
    }
  }

  const totalInvestment = (() => {
    const amt = parseFloat(amount);
    const orders = parseInt(totalOrders, 10);
    if (!isFinite(amt) || !isFinite(orders) || amt <= 0 || orders <= 0) return null;
    return (amt * orders).toFixed(2);
  })();

  return (
    <div className={styles.container}>
      <Select
        label="Symbol"
        options={SYMBOL_OPTIONS}
        value={symbol}
        onChange={(e) => setSymbol(e.target.value)}
      />

      <NumberInput
        label="Amount per order (USDT)"
        min={0}
        step={0.01}
        placeholder="0.00"
        value={amount}
        onChange={(v) => { setAmount(v); setErrors((p) => ({ ...p, amount: undefined })); }}
        error={errors.amount}
        suffix="USDT"
      />

      <Select
        label="Interval"
        options={INTERVAL_OPTIONS}
        value={intervalHours}
        onChange={(e) => setIntervalHours(e.target.value)}
      />

      <NumberInput
        label="Total Orders"
        step={1}
        placeholder="10"
        value={totalOrders}
        onChange={(v) => { setTotalOrders(v); setErrors((p) => ({ ...p, total_orders: undefined })); }}
        error={errors.total_orders}
        showStepper={true}
      />

      {totalInvestment !== null && (
        <div className={styles.summary}>
          <span className={styles.summaryLabel}>Total investment</span>
          <span className={styles.summaryValue}>{totalInvestment} USDT</span>
        </div>
      )}

      <Button
        variant="primary"
        fullWidth
        loading={loading}
        onClick={() => { void handleSubmit(); }}
      >
        Create DCA Bot
      </Button>
    </div>
  );
}
