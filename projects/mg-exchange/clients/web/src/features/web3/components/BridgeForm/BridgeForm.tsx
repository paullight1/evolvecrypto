import { useState } from 'react';
import { Input } from '@shared/ui/Input';
import { Select, type SelectOption } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { Card } from '@shared/ui/Card';
import { toast } from '@shared/ui/Toast';
import { ChainSelector } from '../ChainSelector';
import { getBridgeRoutes, getBridgeQuote, executeBridge } from '../../api';
import type { Chain, BridgeRoute, BridgeQuote } from '../../types';
import styles from './BridgeForm.module.css';

const TOKEN_OPTIONS: SelectOption[] = [
  { value: 'USDC', label: 'USDC' },
  { value: 'USDT', label: 'USDT' },
  { value: 'ETH', label: 'ETH' },
  { value: 'WBTC', label: 'WBTC' },
];

interface BridgeFormProps {
  chains: Chain[];
}

export function BridgeForm({ chains }: BridgeFormProps) {
  const [fromChain, setFromChain] = useState(chains[0]?.id ?? '');
  const [toChain, setToChain] = useState(chains[1]?.id ?? '');
  const [token, setToken] = useState('USDC');
  const [amount, setAmount] = useState('');
  const [routes, setRoutes] = useState<BridgeRoute[]>([]);
  const [selectedRoute, setSelectedRoute] = useState<string>('');
  const [quote, setQuote] = useState<BridgeQuote | null>(null);
  const [loading, setLoading] = useState(false);
  const [bridging, setBridging] = useState(false);

  async function handleGetQuote() {
    if (!amount || !fromChain || !toChain) return;
    setLoading(true);
    try {
      const routeList = await getBridgeRoutes({ from_chain: fromChain, to_chain: toChain, token });
      setRoutes(routeList);
      const bridge = routeList[0]?.bridge_name ?? '';
      setSelectedRoute(bridge);
      if (bridge) {
        const q = await getBridgeQuote({ from_chain: fromChain, to_chain: toChain, token, amount, bridge });
        setQuote(q);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to get bridge quote');
    } finally {
      setLoading(false);
    }
  }

  async function handleBridge() {
    if (!quote || !selectedRoute) return;
    setBridging(true);
    try {
      const result = await executeBridge({
        from_chain: fromChain,
        to_chain: toChain,
        token,
        amount,
        bridge: selectedRoute,
        to_address: '',
      });
      toast.success(`Bridge submitted: ${result.tx_hash.slice(0, 10)}...`);
      setQuote(null);
      setRoutes([]);
      setAmount('');
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Bridge failed');
    } finally {
      setBridging(false);
    }
  }

  const routeOptions: SelectOption[] = routes.map((r) => ({
    value: r.bridge_name,
    label: `${r.bridge_name} (~${Math.round(r.estimated_time_seconds / 60)}m, fee: ${r.fee})`,
  }));

  return (
    <div className={styles.container}>
      <div className={styles.chainRow}>
        <ChainSelector chains={chains} value={fromChain} onChange={(id) => { setFromChain(id); setQuote(null); }} label="From Chain" />
        <ChainSelector chains={chains} value={toChain} onChange={(id) => { setToChain(id); setQuote(null); }} label="To Chain" />
      </div>

      <Select
        label="Token"
        options={TOKEN_OPTIONS}
        value={token}
        onChange={(e) => { setToken(e.target.value); setQuote(null); }}
      />

      <Input
        label="Amount"
        type="number"
        min="0"
        step="any"
        placeholder="0.00"
        value={amount}
        onChange={(e) => { setAmount(e.target.value); setQuote(null); }}
      />

      <Button variant="secondary" fullWidth loading={loading} onClick={() => { void handleGetQuote(); }}>
        Get Bridge Quote
      </Button>

      {routes.length > 0 && (
        <Select
          label="Bridge Route"
          options={routeOptions}
          value={selectedRoute}
          onChange={(e) => setSelectedRoute(e.target.value)}
        />
      )}

      {quote && (
        <Card padding="sm" className={styles.quote}>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>You receive</span>
            <span className={styles.quoteValue}>{quote.to_amount} {token}</span>
          </div>
          <div className={styles.quoteRow}>
            <span className={styles.quoteLabel}>Fee (USD)</span>
            <span className={styles.quoteValue}>${quote.fee_usd}</span>
          </div>
          <Button variant="primary" fullWidth loading={bridging} onClick={() => { void handleBridge(); }}>
            Bridge {token}
          </Button>
        </Card>
      )}
    </div>
  );
}
