import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { PoolCard } from './components/PoolCard';
import { StakeForm } from './components/StakeForm';
import { MyStakesTable } from './components/MyStakesTable';
import { getPools, getMyStakes } from './api';
import type { LaunchpoolPool, LaunchpoolStake } from './types';
import styles from './LaunchpoolPage.module.css';

const TABS: TabDef[] = [
  { key: 'pools', label: 'Pools' },
  { key: 'my-stakes', label: 'My Stakes' },
];

export default function LaunchpoolPage() {
  const [tab, setTab] = useState('pools');
  const [pools, setPools] = useState<LaunchpoolPool[]>([]);
  const [poolsLoading, setPoolsLoading] = useState(false);
  const [stakes, setStakes] = useState<LaunchpoolStake[]>([]);
  const [stakesLoading, setStakesLoading] = useState(false);
  const [selectedPool, setSelectedPool] = useState<LaunchpoolPool | null>(null);

  function loadPools() {
    setPoolsLoading(true);
    getPools()
      .then(setPools)
      .catch(() => setPools([]))
      .finally(() => setPoolsLoading(false));
  }

  function loadStakes() {
    setStakesLoading(true);
    getMyStakes()
      .then(setStakes)
      .catch(() => setStakes([]))
      .finally(() => setStakesLoading(false));
  }

  useEffect(() => {
    if (tab === 'pools') loadPools();
    else loadStakes();
  }, [tab]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Launchpool</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'pools' && (
          poolsLoading ? (
            <div className={styles.grid}>
              {Array.from({ length: 4 }, (_, i) => <div key={i} className={styles.skeletonCard} />)}
            </div>
          ) : (
            <div className={styles.grid}>
              {pools.map((p) => (
                <PoolCard key={p.id} pool={p} onStake={setSelectedPool} />
              ))}
            </div>
          )
        )}

        {tab === 'my-stakes' && (
          <MyStakesTable stakes={stakes} loading={stakesLoading} onUpdated={loadStakes} />
        )}
      </div>

      <StakeForm
        pool={selectedPool}
        onClose={() => setSelectedPool(null)}
        onSuccess={loadStakes}
      />
    </div>
  );
}
