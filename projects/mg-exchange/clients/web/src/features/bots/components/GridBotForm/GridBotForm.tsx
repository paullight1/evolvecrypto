import { useState } from 'react';
import { Card } from '@shared/ui/Card';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { createGridBot } from '../../api';
import styles from './GridBotForm.module.css';

const SYMBOL_OPTIONS: SelectOption[] = [
  { value: 'BTCUSDT', label: 'BTC/USDT' },
  { value: 'ETHUSDT', label: 'ETH/USDT' },
  { value: 'SOLUSDT', label: 'SOL/USDT' },
  { value: 'BNBUSDT', label: 'BNB/USDT' },
  { value: 'XRPUSDT', label: 'XRP/USDT' },
];

interface GridBotFormProps {
  onCreated: () => void;
}

interface FormErrors {
  symbol?: string;
  upper_price?: string;
  lower_price?: string;
  grid_count?: string;
  investment?: string;
}

function calcGridSpacing(upper: string, lower: string, count: number): string {
  const u = parseFloat(upper);
  const l = parseFloat(lower);
  if (!isFinite(u) || !isFinite(l) || u <= l || count <= 1) return '-';
  return ((u - l) / (count - 1)).toFixed(4);
}

function calcPerGridAmount(investment: string, count: number): string {
  const inv = parseFloat(investment);
  if (!isFinite(inv) || inv <= 0 || count <= 0) return '-';
  return (inv / count).toFixed(2);
}

export function GridBotForm({ onCreated }: GridBotFormProps) {
  const [symbol, setSymbol] = useState('BTCUSDT');
  const [upperPrice, setUpperPrice] = useState('');
  const [lowerPrice, setLowerPrice] = useState('');
  const [gridCount, setGridCount] = useState('10');
  const [investment, setInvestment] = useState('');
  const [loading, setLoading] = useState(false);
  const [errors, setErrors] = useState<FormErrors>({});

  function validate(): boolean {
    const errs: FormErrors = {};
    const upper = parseFloat(upperPrice);
    const lower = parseFloat(lowerPrice);
    const count = parseInt(gridCount, 10);
    const inv = parseFloat(investment);

    if (!isFinite(upper) || upper <= 0) errs.upper_price = 'Enter a valid upper price';
    if (!isFinite(lower) || lower <= 0) errs.lower_price = 'Enter a valid lower price';
    if (isFinite(upper) && isFinite(lower) && upper <= lower) {
      errs.upper_price = 'Upper price must be greater than lower price';
    }
    if (!isFinite(count) || count < 2 || count > 200) errs.grid_count = 'Grid count must be between 2 and 200';
    if (!isFinite(inv) || inv <= 0) errs.investment = 'Enter a valid investment amount';

    setErrors(errs);
    return Object.keys(errs).length === 0;
  }

  async function handleSubmit() {
    if (!validate()) return;
    setLoading(true);
    try {
      await createGridBot({
        symbol,
        upper_price: upperPrice,
        lower_price: lowerPrice,
        grid_count: parseInt(gridCount, 10),
        investment,
      });
      toast.success('Grid bot created');
      onCreated();
      setUpperPrice('');
      setLowerPrice('');
      setGridCount('10');
      setInvestment('');
      setErrors({});
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to create bot');
    } finally {
      setLoading(false);
    }
  }

  const count = parseInt(gridCount, 10);
  const gridSpacing = calcGridSpacing(upperPrice, lowerPrice, count);
  const perGrid = calcPerGridAmount(investment, count);
  const showPreview = gridSpacing !== '-' && perGrid !== '-';

  return (
    <div className={styles.container}>
      <Select
        label="Symbol"
        options={SYMBOL_OPTIONS}
        value={symbol}
        onChange={(e) => setSymbol(e.target.value)}
      />

      <div className={styles.priceRow}>
        <NumberInput
          label="Upper Price"
          min={0}
          step={0.01}
          placeholder="0.00"
          value={upperPrice}
          onChange={(v) => { setUpperPrice(v); setErrors((p) => ({ ...p, upper_price: undefined })); }}
          error={errors.upper_price}
          suffix="USDT"
        />
        <NumberInput
          label="Lower Price"
          min={0}
          step={0.01}
          placeholder="0.00"
          value={lowerPrice}
          onChange={(v) => { setLowerPrice(v); setErrors((p) => ({ ...p, lower_price: undefined })); }}
          error={errors.lower_price}
          suffix="USDT"
        />
      </div>

      <NumberInput
        label="Grid Count"
        step={1}
        placeholder="10"
        value={gridCount}
        onChange={(v) => { setGridCount(v); setErrors((p) => ({ ...p, grid_count: undefined })); }}
        error={errors.grid_count}
        showStepper={true}
      />

      <NumberInput
        label="Investment (USDT)"
        min={0}
        step={0.01}
        placeholder="0.00"
        value={investment}
        onChange={(v) => { setInvestment(v); setErrors((p) => ({ ...p, investment: undefined })); }}
        error={errors.investment}
        suffix="USDT"
      />

      {showPreview && (
        <Card padding="sm" className={styles.preview}>
          <div className={styles.previewTitle}>Preview</div>
          <div className={styles.previewRow}>
            <span className={styles.previewLabel}>Grid Spacing</span>
            <span className={styles.previewValue}>{gridSpacing} USDT</span>
          </div>
          <div className={styles.previewRow}>
            <span className={styles.previewLabel}>Per-Grid Amount</span>
            <span className={styles.previewValue}>{perGrid} USDT</span>
          </div>
        </Card>
      )}

      <Button
        variant="primary"
        fullWidth
        loading={loading}
        onClick={() => { void handleSubmit(); }}
      >
        Create Grid Bot
      </Button>
    </div>
  );
}
