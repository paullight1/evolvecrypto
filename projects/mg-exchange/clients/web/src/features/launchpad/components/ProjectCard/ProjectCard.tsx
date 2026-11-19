import { Card } from '@shared/ui/Card';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { ProgressBar } from '@shared/ui/ProgressBar';
import { Countdown } from '@shared/ui/Countdown';
import type { LaunchpadProject, LaunchpadProjectStatus } from '../../types';
import styles from './ProjectCard.module.css';

function statusVariant(status: LaunchpadProjectStatus): BadgeVariant {
  if (status === 'active') return 'success';
  if (status === 'upcoming') return 'info';
  if (status === 'sold_out') return 'warning';
  return 'neutral';
}

function statusLabel(status: LaunchpadProjectStatus): string {
  if (status === 'sold_out') return 'Sold Out';
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function progressPct(committed: string, total: string): number {
  const c = parseFloat(committed);
  const t = parseFloat(total);
  if (!isFinite(c) || !isFinite(t) || t === 0) return 0;
  return Math.min((c / t) * 100, 100);
}

interface ProjectCardProps {
  project: LaunchpadProject;
  onSelect: (project: LaunchpadProject) => void;
}

export function ProjectCard({ project, onSelect }: ProjectCardProps) {
  const pct = progressPct(project.committed_usdt, project.total_raise_usdt);
  const isActive = project.status === 'active';

  return (
    <Card className={styles.card}>
      <div className={styles.header}>
        <div className={styles.token}>
          <span className={styles.tokenIcon}>{project.token_symbol.slice(0, 2)}</span>
          <div>
            <div className={styles.tokenName}>{project.token_name}</div>
            <div className={styles.tokenSymbol}>{project.token_symbol}</div>
          </div>
        </div>
        <Badge variant={statusVariant(project.status)} size="sm">
          {statusLabel(project.status)}
        </Badge>
      </div>

      <p className={styles.description}>{project.description}</p>

      <div className={styles.stats}>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Total Raise</span>
          <span className={styles.statValue}>${parseFloat(project.total_raise_usdt).toLocaleString()}</span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>Token Price</span>
          <span className={styles.statValue}>${project.token_price_usdt}</span>
        </div>
      </div>

      {(isActive || project.status === 'sold_out') && (
        <div className={styles.progress}>
          <ProgressBar value={pct} variant="accent" />
          <div className={styles.progressLabels}>
            <span className={styles.progressText}>{pct.toFixed(1)}% filled</span>
            <span className={styles.progressText}>
              {project.status === 'sold_out'
                ? new Date(project.end_time).toLocaleDateString()
                : <Countdown target={new Date(project.end_time)} format="auto" completedText="Ended" />}
            </span>
          </div>
        </div>
      )}

      {!isActive && project.status !== 'sold_out' && (
        <p className={styles.timeLabel}>
          {project.status === 'upcoming'
            ? <Countdown target={new Date(project.start_time)} format="auto" prefix="Starts in " completedText="Starting soon" />
            : new Date(project.end_time).toLocaleDateString()}
        </p>
      )}

      <Button
        variant={isActive ? 'primary' : 'ghost'}
        fullWidth
        onClick={() => onSelect(project)}
        disabled={project.status === 'completed'}
      >
        {isActive ? 'Commit Now' : project.status === 'upcoming' ? 'View Details' : 'View'}
      </Button>
    </Card>
  );
}
