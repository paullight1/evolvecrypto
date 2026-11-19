import { useState } from 'react';
import { Card } from '@shared/ui/Card';
import { Badge, type BadgeVariant } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import { toast } from '@shared/ui/Toast';
import { unfollowTrader } from '../../api';
import type { FollowRelation } from '../../types';
import styles from './FollowingList.module.css';

interface FollowingListProps {
  relations: FollowRelation[];
  onUpdated: () => void;
}

function statusVariant(isActive: boolean): BadgeVariant {
  return isActive ? 'success' : 'neutral';
}

function pnlClass(pnl: string): string {
  const num = parseFloat(pnl);
  if (num > 0) return styles.positive;
  if (num < 0) return styles.negative;
  return styles.neutral;
}

export function FollowingList({ relations, onUpdated }: FollowingListProps) {
  const [unfollowTarget, setUnfollowTarget] = useState<string | null>(null);
  const [unfollowing, setUnfollowing] = useState(false);

  async function handleUnfollow() {
    if (!unfollowTarget) return;
    setUnfollowing(true);
    try {
      await unfollowTrader(unfollowTarget);
      toast.success('Unfollowed trader');
      onUpdated();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to unfollow');
    } finally {
      setUnfollowing(false);
      setUnfollowTarget(null);
    }
  }

  const targetRelation = relations.find((r) => r.relation_id === unfollowTarget);

  if (relations.length === 0) {
    return (
      <p className={styles.empty}>
        You are not following any traders. Browse the Leaderboard to get started.
      </p>
    );
  }

  return (
    <>
      <div className={styles.list}>
        {relations.map((rel) => (
          <Card key={rel.relation_id} className={styles.card}>
            <div className={styles.header}>
              <div className={styles.nameRow}>
                <span className={styles.name}>{rel.master_id}</span>
                <Badge variant={statusVariant(rel.is_active)} size="sm">
                  {rel.is_active ? 'active' : 'inactive'}
                </Badge>
              </div>
              {rel.is_active && (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setUnfollowTarget(rel.relation_id)}
                >
                  Unfollow
                </Button>
              )}
            </div>

            <div className={styles.stats}>
              <div className={styles.stat}>
                <span className={styles.statLabel}>Allocation</span>
                <span className={styles.statValue}>{rel.allocation} USDT</span>
              </div>
              <div className={styles.stat}>
                <span className={styles.statLabel}>PnL since follow</span>
                <span className={pnlClass(rel.total_pnl)}>
                  {parseFloat(rel.total_pnl) >= 0 ? '+' : ''}
                  {rel.total_pnl} USDT
                </span>
              </div>
            </div>
          </Card>
        ))}
      </div>

      <ConfirmDialog
        open={unfollowTarget !== null && !unfollowing}
        onConfirm={() => { void handleUnfollow(); }}
        onCancel={() => setUnfollowTarget(null)}
        title="Unfollow Trader"
        message={
          targetRelation
            ? `Stop copying trader ${targetRelation.master_id}? Open copy positions will not be closed automatically.`
            : 'Stop copying this trader?'
        }
        confirmText="Unfollow"
        variant="warning"
      />
    </>
  );
}
