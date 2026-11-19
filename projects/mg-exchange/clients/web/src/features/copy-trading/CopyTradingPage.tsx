import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { LeaderboardTable } from './components/LeaderboardTable';
import { FollowModal } from './components/FollowModal';
import { FollowingList } from './components/FollowingList';
import { getLeaderboard, getMyFollowing } from './api';
import type { MasterTrader, FollowRelation } from './types';
import styles from './CopyTradingPage.module.css';

const TABS: TabDef[] = [
  { key: 'leaderboard', label: 'Leaderboard' },
  { key: 'following', label: 'Following' },
];

export default function CopyTradingPage() {
  const [tab, setTab] = useState('leaderboard');
  const [traders, setTraders] = useState<MasterTrader[]>([]);
  const [tradersLoading, setTradersLoading] = useState(false);
  const [relations, setRelations] = useState<FollowRelation[]>([]);
  const [, setRelationsLoading] = useState(false);
  const [followTarget, setFollowTarget] = useState<MasterTrader | null>(null);

  function loadLeaderboard() {
    setTradersLoading(true);
    getLeaderboard()
      .then(setTraders)
      .catch(() => setTraders([]))
      .finally(() => setTradersLoading(false));
  }

  function loadFollowing() {
    setRelationsLoading(true);
    getMyFollowing()
      .then(setRelations)
      .catch(() => setRelations([]))
      .finally(() => setRelationsLoading(false));
  }

  useEffect(() => {
    if (tab === 'leaderboard') loadLeaderboard();
    if (tab === 'following') loadFollowing();
  }, [tab]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Copy Trading</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'leaderboard' && (
          <LeaderboardTable
            traders={traders}
            loading={tradersLoading}
            onFollow={setFollowTarget}
          />
        )}

        {tab === 'following' && (
          <FollowingList
            relations={relations}
            onUpdated={loadFollowing}
          />
        )}
      </div>

      <FollowModal
        trader={followTarget}
        onClose={() => setFollowTarget(null)}
        onSuccess={() => {
          setFollowTarget(null);
          loadFollowing();
          setTab('following');
        }}
      />
    </div>
  );
}
