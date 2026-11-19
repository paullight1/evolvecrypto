import { cn } from '@shared/utils/format';
import styles from './Badge.module.css';

export type BadgeVariant = 'success' | 'danger' | 'warning' | 'info' | 'neutral' | 'long' | 'short';
export type BadgeSize = 'sm' | 'md';

export interface BadgeProps {
  variant: BadgeVariant;
  size?: BadgeSize;
  children: React.ReactNode;
  className?: string;
}

export function Badge({ variant, size = 'md', children, className }: BadgeProps) {
  return (
    <span className={cn(styles.badge, styles[variant], styles[size], className)}>
      {children}
    </span>
  );
}
