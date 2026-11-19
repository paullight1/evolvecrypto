import type { ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Tag.module.css';

export type TagVariant = 'default' | 'success' | 'warning' | 'danger' | 'info' | 'purple';

export interface TagProps {
  children: ReactNode;
  variant?: TagVariant;
  removable?: boolean;
  onRemove?: () => void;
  selectable?: boolean;
  selected?: boolean;
  onClick?: () => void;
  size?: 'sm' | 'md';
  className?: string;
}

export function Tag({
  children,
  variant = 'default',
  removable = false,
  onRemove,
  selectable = false,
  selected = false,
  onClick,
  size = 'md',
  className,
}: TagProps) {
  const isInteractive = selectable || selected;

  const containerClass = cn(
    styles.tag,
    styles[variant],
    styles[size],
    selected && styles.selected,
    isInteractive && styles.selectable,
    className,
  );

  const removeButton = removable ? (
    <button
      type="button"
      className={styles.removeBtn}
      onClick={(e) => {
        e.stopPropagation();
        onRemove?.();
      }}
      aria-label={`Remove ${typeof children === 'string' ? children : ''}`}
      tabIndex={0}
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 12 12"
        fill="none"
        aria-hidden="true"
      >
        <path
          d="M9 3L3 9M3 3l6 6"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
    </button>
  ) : null;

  if (isInteractive) {
    return (
      <button
        type="button"
        className={containerClass}
        onClick={onClick}
        aria-pressed={selected}
      >
        <span className={styles.content}>{children}</span>
        {removeButton}
      </button>
    );
  }

  return (
    <span className={containerClass}>
      <span className={styles.content}>{children}</span>
      {removeButton}
    </span>
  );
}
