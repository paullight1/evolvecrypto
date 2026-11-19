import { cn } from '@shared/utils/format';
import styles from './Divider.module.css';

export type DividerOrientation = 'horizontal' | 'vertical';
export type DividerSpacing = 'sm' | 'md' | 'lg';

export interface DividerProps {
  orientation?: DividerOrientation;
  label?: string;
  spacing?: DividerSpacing;
  className?: string;
}

export function Divider({
  orientation = 'horizontal',
  label,
  spacing = 'md',
  className,
}: DividerProps) {
  if (orientation === 'vertical') {
    return (
      <span
        className={cn(styles.vertical, styles[`spacing-${spacing}`], className)}
        role="separator"
        aria-orientation="vertical"
      />
    );
  }

  if (label) {
    return (
      <div
        className={cn(styles.withLabel, styles[`spacing-${spacing}`], className)}
        role="separator"
        aria-orientation="horizontal"
      >
        <span className={styles.line} />
        <span className={styles.labelText}>{label}</span>
        <span className={styles.line} />
      </div>
    );
  }

  return (
    <hr
      className={cn(styles.horizontal, styles[`spacing-${spacing}`], className)}
      aria-orientation="horizontal"
    />
  );
}
