import { Card } from '@shared/ui/Card';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import type { LaunchpoolPool } from '../../types';
import styles from './PoolCard.module.css';

function statusVariant(status: LaunchpoolPool['status']): BadgeVariant {
  if (status === 'active') return 'success';
  if (status === 'upcoming') return 'info';
  return 'neutral';
}

function farmingPeriod(pool: LaunchpoolPool): string {
  const start = new Date(pool.start_time).toLocaleDateString();
  const end = new Date(pool.end_time).toLocaleDateString();
  return `${start} – ${end}`;
}

interface PoolCardProps {
  pool: LaunchpoolPool;
  onStake: (pool: LaunchpoolPool) => void;
}

export function PoolCard({ pool, onStake }: PoolCardProps) {
  return (
    <Card className={styles.card}>
      <div className={styles.header}>
        <div>
          <div className={styles.rewardToken}>Earn {pool.reward_token}</div>
          <div className={styles.stakeToken}>Stake {pool.stake_token}</div>
        </div>
        <Badge variant={statusVariant(pool.status)} size="sm">
          {pool.status.charAt(0).toUpperCase() + pool.status.slice(1)}
        </Badge>
      </div>

      <div className={styles.apy}>
        <span className={styles.apyValue}>{pool.apy}</span>
        <span className={styles.apyLabel}>APY</span>
      </div>

      <div className={styles.details}>
        <div className={styles.detailRow}>
          <span className={styles.detailLabel}>Total Staked</span>
          <span className={styles.detailValue}>{parseFloat(pool.total_staked).toLocaleString()} {pool.stake_token}</span>
        </div>
        <div className={styles.detailRow}>
          <span className={styles.detailLabel}>Max Stake</span>
          <span className={styles.detailValue}>{parseFloat(pool.max_stake).toLocaleString()} {pool.stake_token}</span>
        </div>
        <div className={styles.detailRow}>
          <span className={styles.detailLabel}>Farming Period</span>
          <span className={styles.detailValue}>{farmingPeriod(pool)}</span>
        </div>
      </div>

      <Button
        variant="primary"
        fullWidth
        onClick={() => onStake(pool)}
        disabled={pool.status !== 'active'}
      >
        {pool.status === 'active' ? 'Stake' : pool.status === 'upcoming' ? 'Coming Soon' : 'Ended'}
      </Button>
    </Card>
  );
}
