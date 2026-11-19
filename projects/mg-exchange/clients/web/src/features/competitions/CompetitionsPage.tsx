import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { CompetitionCard } from './components/CompetitionCard';
import { LeaderboardTable } from './components/LeaderboardTable';
import { JoinForm } from './components/JoinForm';
import { getCompetitions } from './api';
import type { Competition } from './types';
import styles from './CompetitionsPage.module.css';

const TABS: TabDef[] = [
  { key: 'active', label: 'Active' },
  { key: 'upcoming', label: 'Upcoming' },
  { key: 'ended', label: 'Past' },
];

export default function CompetitionsPage() {
  const [tab, setTab] = useState('active');
  const [competitions, setCompetitions] = useState<Competition[]>([]);
  const [loading, setLoading] = useState(false);
  const [leaderboardTarget, setLeaderboardTarget] = useState<Competition | null>(null);
  const [joinTarget, setJoinTarget] = useState<Competition | null>(null);

  function load() {
    setLoading(true);
    getCompetitions()
      .then(setCompetitions)
      .catch(() => setCompetitions([]))
      .finally(() => setLoading(false));
  }

  useEffect(() => { load(); }, []);

  const filtered = competitions.filter((c) => c.status === tab);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Competitions</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {loading ? (
          <div className={styles.grid}>
            {Array.from({ length: 4 }, (_, i) => <div key={i} className={styles.skeletonCard} />)}
          </div>
        ) : (
          <div className={styles.grid}>
            {filtered.map((c) => (
              <CompetitionCard
                key={c.id}
                competition={c}
                onJoin={setJoinTarget}
                onViewLeaderboard={setLeaderboardTarget}
              />
            ))}
          </div>
        )}
      </div>

      <LeaderboardTable
        competition={leaderboardTarget}
        onClose={() => setLeaderboardTarget(null)}
      />
      <JoinForm
        competition={joinTarget}
        onClose={() => setJoinTarget(null)}
        onSuccess={load}
      />
    </div>
  );
}
