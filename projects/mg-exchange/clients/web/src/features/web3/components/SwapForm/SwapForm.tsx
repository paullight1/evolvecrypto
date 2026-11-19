import { useState } from 'react';
import { Input } from '@shared/ui/Input';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { toast } from '@shared/ui/Toast';
import { ChainSelector } from '../ChainSelector';
import { getDexQuote, executeDexSwap } from '../../api';
import type { Chain, DexQuote } from '../../types';
import styles from './SwapForm.module.css';

const TOKEN_OPTIONS: SelectOption[] = [
  { value: 'ETH', label: 'ETH' },
  { value: 'USDC', label: 'USDC' },
  { value: 'USDT', label: 'USDT' },
  { value: 'WBTC', label: 'WBTC' },
  { value: 'DAI', label: 'DAI' },
];

interface SwapFormProps {
  chains: Chain[];
}

export function SwapForm({ chains }: SwapFormProps) {
  const [chain, setChain] = useState(chains[0]?.id ?? '');
  const [fromToken, setFromToken] = useState('ETH');
  const [toToken, setToToken] = useState('USDC');
  const [amount, setAmount] = useState('');
  const [quote, setQuote] = useState<DexQuote | null>(null);
  const [quoting, setQuoting] = useState(false);
  const [swapping, setSwapping] = useState(false);

  async function handleGetQuote() {
    if (!amount || !chain) return;
    setQuoting(true);
    try {
      const q = await getDexQuote({ chain, from_token: fromToken, to_token: toToken, amount });
      setQuote(q);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to get quote');
    } finally {
      setQuoting(false);
    }
  }

  async function handleSwap() {
    if (!quote) return;
    setSwapping(true);
    try {
      const result = await executeDexSwap({
        chain,
        from_token: fromToken,
        to_token: toToken,
        amount,
        slippage: '0.5',
      });
      toast.success(`Swap submitted: ${result.tx_hash.slice(0, 10)}...`);
      setQuote(null);
      setAmount('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Swap failed');
    } finally {
      setSwapping(false);
    }
  }

  return (
    <div className={styles.container}>
      <ChainSelector chains={chains} value={chain} onChange={(id) => { setChain(id); setQuote(null); }} />

      <div className={styles.tokenRow}>
        <Select
          label="From"
          options={TOKEN_OPTIONS}
          value={fromToken}
          onChange={(e) => { setFromToken(e.target.value); setQuote(null); }}
        />
        <Select
          label="To"
          options={TOKEN_OPTIONS}
          value={toToken}
          onChange={(e) => { setToToken(e.target.value); setQuote(null); }}
        />
      </div>

      <Input
        label="Amount"
        type="number"
        min="0"
        step="any"
        placeholder="0.00"
        value={amount}
        onChange={(e) => { setAmount(e.target.value); setQuote(null); }}
      />

      <Button variant="secondary" fullWidth loading={quoting} onClick={() => { void handleGetQuote(); }}>
        Get Quote
      </Button>

      {quote && (
        <Card padding="sm" className={styles.quote}>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>You receive</span>
            <span className={styles.quoteValue}>{quote.to_amount} {toToken}</span>
          </div>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>Price impact</span>
            <span className={styles.quoteValue}>{quote.price_impact}%</span>
          </div>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>Fee</span>
            <span className={styles.quoteValue}>{quote.fee}</span>
          </div>
          <Button variant="primary" fullWidth loading={swapping} onClick={() => { void handleSwap(); }}>
            Execute Swap
          </Button>
        </Card>
      )}
    </div>
  );
}
