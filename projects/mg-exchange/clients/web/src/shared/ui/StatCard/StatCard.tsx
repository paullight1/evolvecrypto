import type { ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import { Skeleton } from '@shared/ui/Skeleton';
import styles from './StatCard.module.css';

export type StatCardTrend = 'up' | 'down' | 'neutral';

export interface StatCardProps {
  label: string;
  value: string | number;
  prefix?: ReactNode;
  suffix?: ReactNode;
  change?: string;
  trend?: StatCardTrend;
  icon?: ReactNode;
  loading?: boolean;
  className?: string;
}

export function StatCard({
  label,
  value,
  prefix,
  suffix,
  change,
  trend = 'neutral',
  icon,
  loading = false,
  className,
}: StatCardProps) {
  if (loading) {
    return (
      <div className={cn(styles.card, className)}>
        <Skeleton variant="text" width="60%" height="12px" />
        <Skeleton variant="text" width="80%" height="20px" />
      </div>
    );
  }

  return (
    <div className={cn(styles.card, className)}>
      <div className={styles.header}>
        <span className={styles.label}>{label}</span>
        {icon != null && <span className={styles.icon}>{icon}</span>}
      </div>
      <div className={styles.valueRow}>
        {prefix != null && <span className={styles.affix}>{prefix}</span>}
        <span className={styles.value}>{value}</span>
        {suffix != null && <span className={styles.affix}>{suffix}</span>}
      </div>
      {change != null && (
        <span className={cn(styles.change, styles[trend])}>{change}</span>
      )}
    </div>
  );
}
