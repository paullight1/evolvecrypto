import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { TokenOverview } from './components/TokenOverview';
import { BurnHistory } from './components/BurnHistory';
import { InsuranceStaking } from './components/InsuranceStaking';
import { DistributionChart } from './components/DistributionChart';
import { getBurnHistory, getInsurancePool, getTokenSupply } from './api';
import type { BurnRecord, InsuranceStakeInfo, TokenSupplyInfo } from './types';
import styles from './TokenPage.module.css';

const TABS: TabDef[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'burns', label: 'Burn History' },
  { key: 'insurance', label: 'Insurance Staking' },
  { key: 'distribution', label: 'Distribution' },
];

export default function TokenPage() {
  const [tab, setTab] = useState('overview');
  const [supply, setSupply] = useState<TokenSupplyInfo | null>(null);
  const [burns, setBurns] = useState<BurnRecord[]>([]);
  const [burnsLoading, setBurnsLoading] = useState(false);
  const [insurance, setInsurance] = useState<InsuranceStakeInfo | null>(null);

  useEffect(() => {
    getTokenSupply().then(setSupply).catch(() => setSupply(null));
  }, []);

  useEffect(() => {
    if (tab === 'burns') {
      setBurnsLoading(true);
      getBurnHistory()
        .then(setBurns)
        .catch(() => setBurns([]))
        .finally(() => setBurnsLoading(false));
    }
    if (tab === 'insurance') {
      getInsurancePool().then(setInsurance).catch(() => setInsurance(null));
    }
  }, [tab]);

  function refreshInsurance() {
    getInsurancePool().then(setInsurance).catch(() => setInsurance(null));
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>EXC Token</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'overview' && <TokenOverview supply={supply} />}
        {tab === 'burns' && <BurnHistory records={burns} loading={burnsLoading} />}
        {tab === 'insurance' && <InsuranceStaking info={insurance} onUpdated={refreshInsurance} />}
        {tab === 'distribution' && (
          <>
            <h2 className={styles.sectionTitle}>Token Distribution</h2>
            <DistributionChart />
          </>
        )}
      </div>
    </div>
  );
}
