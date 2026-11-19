import { useState } from 'react';
import { NumberInput } from '@shared/ui/NumberInput';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { toast } from '@shared/ui/Toast';
import { ProgressBar } from '@shared/ui/ProgressBar';
import { Countdown } from '@shared/ui/Countdown';
import { getConvertQuote, executeConvert } from '../../api';
import type { ConvertQuote } from '../../types';
import styles from './ConvertForm.module.css';

const ASSET_OPTIONS: SelectOption[] = [
  { value: 'BTC', label: 'BTC' },
  { value: 'ETH', label: 'ETH' },
  { value: 'USDT', label: 'USDT' },
  { value: 'USDC', label: 'USDC' },
  { value: 'BNB', label: 'BNB' },
  { value: 'SOL', label: 'SOL' },
];

const QUOTE_TTL_SECONDS = 30;

interface ConvertFormProps {
  onConverted: () => void;
}

export function ConvertForm({ onConverted }: ConvertFormProps) {
  const [fromAsset, setFromAsset] = useState('BTC');
  const [toAsset, setToAsset] = useState('USDT');
  const [amount, setAmount] = useState('');
  const [quote, setQuote] = useState<ConvertQuote | null>(null);
  const [quoteExpiresAt, setQuoteExpiresAt] = useState<number>(0);
  const [quoting, setQuoting] = useState(false);
  const [executing, setExecuting] = useState(false);

  function swapAssets() {
    setFromAsset(toAsset);
    setToAsset(fromAsset);
    setQuote(null);
  }

  async function handleGetQuote() {
    if (!amount) return;
    setQuoting(true);
    try {
      const q = await getConvertQuote(fromAsset, toAsset, amount);
      setQuote(q);
      setQuoteExpiresAt(q.expires_at * 1000);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to get quote');
    } finally {
      setQuoting(false);
    }
  }

  async function handleExecute() {
    if (!quote) return;
    setExecuting(true);
    try {
      await executeConvert(quote.quote_id);
      toast.success(`Converted ${amount} ${fromAsset} to ${quote.to_amount} ${toAsset}`);
      setQuote(null);
      onConverted();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Conversion failed');
    } finally {
      setExecuting(false);
    }
  }

  return (
    <div className={styles.container}>
      <Select label="From" options={ASSET_OPTIONS} value={fromAsset} onChange={(e) => { setFromAsset(e.target.value); setQuote(null); }} />

      <button type="button" className={styles.swapButton} onClick={swapAssets} title="Swap assets">
        ⇅
      </button>

      <Select label="To" options={ASSET_OPTIONS} value={toAsset} onChange={(e) => { setToAsset(e.target.value); setQuote(null); }} />

      <NumberInput
        label="Amount"
        min={0}
        placeholder="0.00"
        value={amount}
        onChange={(v) => { setAmount(v); setQuote(null); }}
      />

      <Button variant="secondary" fullWidth loading={quoting} onClick={() => { void handleGetQuote(); }}>
        Get Quote
      </Button>

      {quote && (
        <Card padding="sm" className={styles.quoteCard}>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>You receive</span>
            <span className={styles.quoteValue}>{quote.to_amount} {toAsset}</span>
          </div>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>Rate</span>
            <span className={styles.quoteValue}>1 {fromAsset} = {quote.rate} {toAsset}</span>
          </div>
          <div className={styles.timerRow}>
            <ProgressBar value={quoteExpiresAt - Date.now()} max={QUOTE_TTL_SECONDS * 1000} variant="warning" size="sm" className={styles.timerProgress} />
            <Countdown target={quoteExpiresAt} format="ms" completedText="Quote expired" className={styles.timerText} onComplete={() => setQuote(null)} />
          </div>
          <Button
            variant="primary"
            fullWidth
            loading={executing}
            disabled={quoteExpiresAt > 0 && Date.now() >= quoteExpiresAt}
            onClick={() => { void handleExecute(); }}
          >
            Convert {fromAsset} → {toAsset}
          </Button>
        </Card>
      )}
    </div>
  );
}
