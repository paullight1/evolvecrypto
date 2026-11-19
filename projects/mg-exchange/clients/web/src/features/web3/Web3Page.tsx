import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { SwapForm } from './components/SwapForm';
import { BridgeForm } from './components/BridgeForm';
import { WalletBalances } from './components/WalletBalances';
import { getDexChains, getWeb3Balances } from './api';
import type { Chain, TokenBalance } from './types';
import styles from './Web3Page.module.css';

const TABS: TabDef[] = [
  { key: 'swap', label: 'Swap' },
  { key: 'bridge', label: 'Bridge' },
  { key: 'wallet', label: 'Wallet' },
];

export default function Web3Page() {
  const [tab, setTab] = useState('swap');
  const [chains, setChains] = useState<Chain[]>([]);
  const [balances, setBalances] = useState<TokenBalance[]>([]);
  const [balancesLoading, setBalancesLoading] = useState(false);

  useEffect(() => {
    getDexChains()
      .then(setChains)
      .catch(() => setChains([]));
  }, []);

  useEffect(() => {
    if (tab === 'wallet') {
      setBalancesLoading(true);
      getWeb3Balances()
        .then(setBalances)
        .catch(() => setBalances([]))
        .finally(() => setBalancesLoading(false));
    }
  }, [tab]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Web3 / DEX</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'swap' && <SwapForm chains={chains} />}
        {tab === 'bridge' && <BridgeForm chains={chains} />}
        {tab === 'wallet' && <WalletBalances balances={balances} loading={balancesLoading} />}
      </div>
    </div>
  );
}
