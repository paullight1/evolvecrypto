import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { getMyProfile } from './api';
import type { TraderProfile } from './types';
import { get } from '@shared/api/client';
import type { Position } from '@features/trading/types';
import styles from './SocialPage.module.css';

interface PosRow extends Record<string, unknown> {
  symbol: string;
  side: string;
  size: string;
  entry_price: string;
  unrealized_pnl: string;
}

const POS_COLS: ColumnDef<PosRow>[] = [
  { key: 'symbol', header: 'Symbol' },
  { key: 'side', header: 'Side', width: '80px' },
  { key: 'size', header: 'Size', align: 'right' },
  { key: 'entry_price', header: 'Entry', align: 'right' },
  { key: 'unrealized_pnl', header: 'PnL', align: 'right' },
];

export default function TraderProfilePage() {
  const [profile, setProfile] = useState<TraderProfile | null>(null);
  const [positions, setPositions] = useState<Position[]>([]);
  const [loading, setLoading] = useState(true);

  // In a real app, this would use router params for user_id
  useEffect(() => {
    Promise.all([
      getMyProfile(),
      get<Position[]>('/positions'),
    ])
      .then(([p, pos]) => { setProfile(p); setPositions(pos); })
      .catch(() => { setProfile(null); setPositions([]); })
      .finally(() => setLoading(false));
  }, []);

  const posRows: PosRow[] = positions.map((p) => ({
    symbol: p.symbol, side: p.side, size: p.size, entry_price: p.entry_price, unrealized_pnl: p.unrealized_pnl,
  }));

  if (loading) return <div className={styles.page}><p>Loading...</p></div>;

  return (
    <div className={styles.page}>
      <div className={styles.profileHeader}>
        <div className={styles.avatar}>{profile?.username?.[0]?.toUpperCase() ?? '?'}</div>
        <div>
          <div className={styles.profileName}>{profile?.username ?? 'Trader'}</div>
          <div className={styles.profileBio}>{profile?.bio ?? ''}</div>
        </div>
      </div>

      <div className={styles.profileStats}>
        <div className={styles.profileStat}>
          <div className={styles.profileStatLabel}>Win Rate</div>
          <div className={styles.profileStatValue}>{profile?.win_rate ?? '—'}%</div>
        </div>
        <div className={styles.profileStat}>
          <div className={styles.profileStatLabel}>30d ROI</div>
          <div className={styles.profileStatValue}>{profile?.roi_30d ?? '—'}%</div>
        </div>
        <div className={styles.profileStat}>
          <div className={styles.profileStatLabel}>Total PnL</div>
          <div className={styles.profileStatValue}>{profile?.total_pnl ?? '—'}</div>
        </div>
        <div className={styles.profileStat}>
          <div className={styles.profileStatLabel}>Followers</div>
          <div className={styles.profileStatValue}>{profile?.followers ?? 0}</div>
        </div>
      </div>

      <Card padding="md">
        <h2 style={{ fontSize: 'var(--text-sm)', fontWeight: 'var(--font-bold)', color: 'var(--text-1)', marginBottom: 'var(--pad-sm)' }}>
          Current Positions
        </h2>
        <Table<PosRow> columns={POS_COLS} data={posRows} emptyText="No open positions" />
      </Card>
    </div>
  );
}
