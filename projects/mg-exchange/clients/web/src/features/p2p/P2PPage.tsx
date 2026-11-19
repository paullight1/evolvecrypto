import { useState } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { AdList } from './components/AdList';
import { OrderFlow } from './components/OrderFlow';
import { CreateAdForm } from './components/CreateAdForm';
import styles from './P2PPage.module.css';

const TABS: TabDef[] = [
  { key: 'buy', label: 'Buy' },
  { key: 'sell', label: 'Sell' },
  { key: 'my-orders', label: 'My Orders' },
  { key: 'my-ads', label: 'My Ads' },
];

export default function P2PPage() {
  const [tab, setTab] = useState('buy');
  const [refreshKey, setRefreshKey] = useState(0);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>P2P Trading</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />
      <div className={styles.content}>
        {tab === 'buy' && <AdList side="buy" key={`buy-${refreshKey}`} />}
        {tab === 'sell' && <AdList side="sell" key={`sell-${refreshKey}`} />}
        {tab === 'my-orders' && <OrderFlow />}
        {tab === 'my-ads' && <CreateAdForm onCreated={() => { setRefreshKey((k) => k + 1); setTab('buy'); }} />}
      </div>
    </div>
  );
}
