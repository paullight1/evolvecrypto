import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { TwapForm } from './components/TwapForm';
import { VwapForm } from './components/VwapForm';
import { AlgoOrderTable } from './components/AlgoOrderTable';
import { getAlgoOrders } from './api';
import type { AlgoOrder } from './types';
import styles from './AlgoPage.module.css';

const TABS: TabDef[] = [
  { key: 'twap', label: 'TWAP' },
  { key: 'vwap', label: 'VWAP' },
  { key: 'active', label: 'Active Orders' },
  { key: 'history', label: 'History' },
];

export default function AlgoPage() {
  const [tab, setTab] = useState('twap');
  const [activeOrders, setActiveOrders] = useState<AlgoOrder[]>([]);
  const [historyOrders, setHistoryOrders] = useState<AlgoOrder[]>([]);
  const [activeLoading, setActiveLoading] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);

  function loadActive() {
    setActiveLoading(true);
    getAlgoOrders('running')
      .then(setActiveOrders)
      .catch(() => setActiveOrders([]))
      .finally(() => setActiveLoading(false));
  }

  function loadHistory() {
    setHistoryLoading(true);
    getAlgoOrders('completed')
      .then(setHistoryOrders)
      .catch(() => setHistoryOrders([]))
      .finally(() => setHistoryLoading(false));
  }

  useEffect(() => {
    if (tab === 'active') loadActive();
    if (tab === 'history') loadHistory();
  }, [tab]);

  function handleOrderCreated() {
    setTab('active');
    loadActive();
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Algo Execution</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'twap' && <TwapForm onCreated={handleOrderCreated} />}
        {tab === 'vwap' && <VwapForm onCreated={handleOrderCreated} />}
        {tab === 'active' && (
          <AlgoOrderTable orders={activeOrders} loading={activeLoading} onCancelled={loadActive} showActions />
        )}
        {tab === 'history' && (
          <AlgoOrderTable orders={historyOrders} loading={historyLoading} onCancelled={loadHistory} />
        )}
      </div>
    </div>
  );
}
