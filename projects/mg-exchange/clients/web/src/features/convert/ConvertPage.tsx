import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { ConvertForm } from './components/ConvertForm';
import { ConvertHistory } from './components/ConvertHistory';
import { getConvertHistory } from './api';
import type { ConvertHistoryItem } from './types';
import styles from './ConvertPage.module.css';

const TABS: TabDef[] = [
  { key: 'convert', label: 'Convert' },
  { key: 'history', label: 'History' },
];

export default function ConvertPage() {
  const [tab, setTab] = useState('convert');
  const [history, setHistory] = useState<ConvertHistoryItem[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  function loadHistory() {
    setHistoryLoading(true);
    getConvertHistory()
      .then(setHistory)
      .catch(() => setHistory([]))
      .finally(() => setHistoryLoading(false));
  }

  useEffect(() => {
    if (tab === 'history') loadHistory();
  }, [tab]);

  function handleConverted() {
    setTab('history');
    loadHistory();
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Convert</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'convert' && <ConvertForm onConverted={handleConverted} />}
        {tab === 'history' && <ConvertHistory items={history} loading={historyLoading} />}
      </div>
    </div>
  );
}
