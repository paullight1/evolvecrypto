import { Card } from '@shared/ui/Card';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import type { Competition, CompetitionMetric, CompetitionStatus } from '../../types';
import styles from './CompetitionCard.module.css';

function statusVariant(status: CompetitionStatus): BadgeVariant {
  if (status === 'active') return 'success';
  if (status === 'upcoming') return 'info';
  return 'neutral';
}

function metricLabel(metric: CompetitionMetric): string {
  const labels: Record<CompetitionMetric, string> = {
    pnl: 'Best PnL',
    roi: 'Best ROI',
    volume: 'Most Volume',
    trades: 'Most Trades',
  };
  return labels[metric];
}

function countdown(target: string): string {
  const ms = new Date(target).getTime() - Date.now();
  if (ms <= 0) return 'Ended';
  const d = Math.floor(ms / 86_400_000);
  const h = Math.floor((ms % 86_400_000) / 3_600_000);
  return d > 0 ? `${d}d ${h}h left` : `${h}h left`;
}

interface CompetitionCardProps {
  competition: Competition;
  onJoin: (competition: Competition) => void;
  onViewLeaderboard: (competition: Competition) => void;
}

export function CompetitionCard({ competition, onJoin, onViewLeaderboard }: CompetitionCardProps) {
  const isActive = competition.status === 'active';

  return (
    <Card className={styles.card}>
      <div className={styles.header}>
        <Badge variant={statusVariant(competition.status)} size="sm">
          {competition.status.charAt(0).toUpperCase() + competition.status.slice(1)}
        </Badge>
        <Badge variant="info" size="sm">{metricLabel(competition.metric)}</Badge>
      </div>

      <h3 className={styles.name}>{competition.name}</h3>
      <p className={styles.description}>{competition.description}</p>

      <div className={styles.stats}>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Prize Pool</span>
          <span className={styles.statValue}>{parseFloat(competition.prize_pool).toLocaleString()} {competition.prize_currency}</span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Participants</span>
          <span className={styles.statValue}>{competition.participants.toLocaleString()}</span>
        </div>
        {isActive && (
          <div className={styles.stat}>
            <span className={styles.statLabel}>Ends</span>
            <span className={styles.statValue}>{countdown(competition.end_time)}</span>
          </div>
        )}
        {competition.status === 'upcoming' && (
          <div className={styles.stat}>
            <span className={styles.statLabel}>Starts</span>
            <span className={styles.statValue}>{new Date(competition.start_time).toLocaleDateString()}</span>
          </div>
        )}
      </div>

      <div className={styles.actions}>
        <Button variant="ghost" size="sm" onClick={() => onViewLeaderboard(competition)}>
          Leaderboard
        </Button>
        {!competition.is_joined && isActive && (
          <Button variant="primary" size="sm" onClick={() => onJoin(competition)}>
            Join
          </Button>
        )}
        {competition.is_joined && (
          <Badge variant="success" size="sm">Joined</Badge>
        )}
      </div>
    </Card>
  );
}
