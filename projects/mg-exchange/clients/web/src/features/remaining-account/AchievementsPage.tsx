import { useState, useEffect } from 'react';
import { get } from '@shared/api/client';
import { ProgressBar } from '@shared/ui/ProgressBar';
import styles from './AccountPage.module.css';

interface Achievement {
  id: string;
  name: string;
  description: string;
  icon: string;
  progress: number;
  required: number;
  unlocked: boolean;
}

export default function AchievementsPage() {
  const [achievements, setAchievements] = useState<Achievement[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    get<Achievement[]>('/account/achievements')
      .then(setAchievements)
      .catch(() => setAchievements([]))
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Achievements</h1>
      {loading ? <p>Loading...</p> : (
        <div className={styles.achievementGrid}>
          {achievements.map((a) => (
            <div key={a.id} className={`${styles.achievementCard} ${!a.unlocked ? styles.achievementLocked : ''}`}>
              <div className={styles.achievementIcon}>{a.icon}</div>
              <div className={styles.achievementName}>{a.name}</div>
              <div className={styles.achievementDesc}>{a.description}</div>
              <ProgressBar value={a.progress} max={a.required} variant="accent" />
              <div style={{ fontSize: 'var(--text-xs)', color: 'var(--text-2)', marginTop: '4px' }}>
                {a.progress}/{a.required}
              </div>
            </div>
          ))}
          {achievements.length === 0 && <p style={{ color: 'var(--text-2)', gridColumn: '1/-1' }}>No achievements yet</p>}
        </div>
      )}
    </div>
  );
}
