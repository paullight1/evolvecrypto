import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { BuyCryptoForm } from './components/BuyCryptoForm';
import { FiatHistory } from './components/FiatHistory';
import { getFiatHistory } from './api';
import type { FiatOrder } from './types';
import styles from './FiatPage.module.css';

const TABS: TabDef[] = [
  { key: 'buy', label: 'Buy' },
  { key: 'sell', label: 'Sell' },
  { key: 'history', label: 'History' },
];

export default function FiatPage() {
  const [tab, setTab] = useState('buy');
  const [history, setHistory] = useState<FiatOrder[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  function loadHistory() {
    setHistoryLoading(true);
    getFiatHistory()
      .then(setHistory)
      .catch(() => setHistory([]))
      .finally(() => setHistoryLoading(false));
  }

  useEffect(() => {
    if (tab === 'history') loadHistory();
  }, [tab]);

  function handleSuccess() {
    setTab('history');
    loadHistory();
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Buy Crypto</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'buy' && <BuyCryptoForm mode="buy" onSuccess={handleSuccess} />}
        {tab === 'sell' && <BuyCryptoForm mode="sell" onSuccess={handleSuccess} />}
        {tab === 'history' && <FiatHistory orders={history} loading={historyLoading} />}
      </div>
    </div>
  );
}
