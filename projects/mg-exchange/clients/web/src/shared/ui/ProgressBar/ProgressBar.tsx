import { cn } from '@shared/utils/format';
import styles from './ProgressBar.module.css';

export type ProgressBarVariant = 'default' | 'long' | 'short' | 'warning' | 'accent';
export type ProgressBarSize = 'sm' | 'md';

export interface ProgressBarProps {
  value: number;
  max?: number;
  variant?: ProgressBarVariant;
  size?: ProgressBarSize;
  label?: string;
  showValue?: boolean;
  className?: string;
}

export function ProgressBar({
  value,
  max = 100,
  variant = 'accent',
  size = 'md',
  label,
  showValue = false,
  className,
}: ProgressBarProps) {
  const clamped = Math.min(Math.max(value, 0), max);
  const pct = max > 0 ? (clamped / max) * 100 : 0;
  const displayPct = Math.round(pct);

  return (
    <div className={cn(styles.wrapper, className)}>
      {(label || showValue) && (
        <div className={styles.header}>
          {label && <span className={styles.label}>{label}</span>}
          {showValue && <span className={styles.value}>{displayPct}%</span>}
        </div>
      )}
      <div
        role="progressbar"
        aria-valuenow={clamped}
        aria-valuemin={0}
        aria-valuemax={max}
        className={cn(styles.track, styles[size])}
      >
        <div
          className={cn(styles.fill, styles[variant])}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}
