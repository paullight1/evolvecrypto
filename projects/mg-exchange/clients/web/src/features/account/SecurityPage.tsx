import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { useState } from 'react';
import { SecuritySettings } from './components/SecuritySettings';
import { ApiKeyManager } from './components/ApiKeyManager';
import { SessionList } from './components/SessionList';
import styles from './SecurityPage.module.css';

const TABS: TabDef[] = [
  { key: 'security', label: 'Security' },
  { key: 'api-keys', label: 'API Keys' },
  { key: 'sessions', label: 'Sessions' },
];

export default function SecurityPage() {
  const [tab, setTab] = useState('security');

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Security</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'security' && <SecuritySettings />}
        {tab === 'api-keys' && <ApiKeyManager />}
        {tab === 'sessions' && <SessionList />}
      </div>
    </div>
  );
}
