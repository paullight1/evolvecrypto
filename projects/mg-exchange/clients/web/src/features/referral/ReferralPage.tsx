import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { StatsCards } from './components/StatsCards';
import { CommissionTable } from './components/CommissionTable';
import { ReferralTree } from './components/ReferralTree';
import { getReferralStats, getCommissions, getReferralTree } from './api';
import type { ReferralStats, ReferralCommission, ReferralNode } from './types';
import styles from './ReferralPage.module.css';

const TABS: TabDef[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'commissions', label: 'Commissions' },
  { key: 'tree', label: 'Referral Tree' },
];

const DEFAULT_STATS: ReferralStats = {
  referral_code: '—',
  referral_url: '',
  total_referrals: 0,
  active_referrals: 0,
  total_commissions_usdt: '0',
  pending_commissions_usdt: '0',
  commission_rate: '0',
};

export default function ReferralPage() {
  const [tab, setTab] = useState('overview');
  const [stats, setStats] = useState<ReferralStats>(DEFAULT_STATS);
  const [commissions, setCommissions] = useState<ReferralCommission[]>([]);
  const [commissionsLoading, setCommissionsLoading] = useState(false);
  const [tree, setTree] = useState<ReferralNode[]>([]);
  const [treeLoading, setTreeLoading] = useState(false);

  useEffect(() => {
    getReferralStats().then(setStats).catch(() => setStats(DEFAULT_STATS));
  }, []);

  useEffect(() => {
    if (tab === 'commissions') {
      setCommissionsLoading(true);
      getCommissions()
        .then(setCommissions)
        .catch(() => setCommissions([]))
        .finally(() => setCommissionsLoading(false));
    }
    if (tab === 'tree') {
      setTreeLoading(true);
      getReferralTree()
        .then(setTree)
        .catch(() => setTree([]))
        .finally(() => setTreeLoading(false));
    }
  }, [tab]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Referral Program</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'overview' && <StatsCards stats={stats} />}
        {tab === 'commissions' && <CommissionTable commissions={commissions} loading={commissionsLoading} />}
        {tab === 'tree' && <ReferralTree nodes={tree} loading={treeLoading} />}
      </div>
    </div>
  );
}
