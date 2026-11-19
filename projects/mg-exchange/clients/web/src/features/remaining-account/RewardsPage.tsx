import { useState, useEffect } from 'react';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { get, post } from '@shared/api/client';
import styles from './AccountPage.module.css';

interface Campaign {
  id: string;
  name: string;
  description: string;
  reward: string;
  ends_at: number;
  enrolled: boolean;
}

interface PointsStore {
  points: number;
}

export default function RewardsPage() {
  const [campaigns, setCampaigns] = useState<Campaign[]>([]);
  const [store, setStore] = useState<PointsStore>({ points: 0 });
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      get<Campaign[]>('/rewards/campaigns'),
      get<PointsStore>('/rewards/points'),
    ])
      .then(([c, s]) => { setCampaigns(c); setStore(s); })
      .catch(() => { setCampaigns([]); setStore({ points: 0 }); })
      .finally(() => setLoading(false));
  }, []);

  async function handleEnroll(id: string) {
    try {
      await post(`/rewards/campaigns/${id}/enroll`, undefined);
      toast.success('Enrolled in campaign');
      setCampaigns((prev) => prev.map((c) => c.id === id ? { ...c, enrolled: true } : c));
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed');
    }
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Rewards</h1>

      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--pad)', marginBottom: 'var(--pad)' }}>
        <span style={{ fontSize: 'var(--text-sm)', color: 'var(--text-2)' }}>Your Points:</span>
        <span style={{ fontSize: 'var(--text-xl)', fontWeight: 'var(--font-bold)', color: 'var(--accent)', fontFamily: 'var(--font-data)' }}>
          {store.points.toLocaleString()}
        </span>
      </div>

      <div className={styles.rewardSection}>
        <h2 className={styles.sectionTitle}>Active Campaigns</h2>
        {loading ? <p>Loading...</p> : campaigns.map((c) => (
          <div key={c.id} className={styles.campaignCard}>
            <div className={styles.campaignInfo}>
              <div className={styles.campaignName}>{c.name}</div>
              <div className={styles.campaignDesc}>{c.description} — Reward: {c.reward}</div>
              <div style={{ fontSize: 'var(--text-xs)', color: 'var(--text-2)', marginTop: '4px' }}>
                Ends: {new Date(c.ends_at * 1000).toLocaleDateString()}
              </div>
            </div>
            {c.enrolled ? (
              <span style={{ fontSize: 'var(--text-xs)', color: 'var(--color-buy)' }}>Enrolled</span>
            ) : (
              <Button variant="primary" size="sm" onClick={() => { void handleEnroll(c.id); }}>Join</Button>
            )}
          </div>
        ))}
        {!loading && campaigns.length === 0 && <p style={{ color: 'var(--text-2)' }}>No active campaigns</p>}
      </div>
    </div>
  );
}
