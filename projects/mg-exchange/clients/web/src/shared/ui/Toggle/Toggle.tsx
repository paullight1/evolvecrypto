import type { KeyboardEvent } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Toggle.module.css';

export type ToggleSize = 'sm' | 'md';

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  description?: string;
  size?: ToggleSize;
  disabled?: boolean;
  className?: string;
}

export function Toggle({
  checked,
  onChange,
  label,
  description,
  size = 'md',
  disabled = false,
  className,
}: ToggleProps) {
  function handleClick() {
    if (!disabled) {
      onChange(!checked);
    }
  }

  function handleKeyDown(e: KeyboardEvent<HTMLButtonElement>) {
    if (e.key === ' ' || e.key === 'Enter') {
      e.preventDefault();
      if (!disabled) {
        onChange(!checked);
      }
    }
  }

  const track = (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      className={cn(
        styles.track,
        styles[size],
        checked && styles.checked,
        disabled && styles.disabled,
      )}
    >
      <span className={cn(styles.thumb, styles[`thumb_${size}`])} />
    </button>
  );

  if (!label && !description) {
    return <span className={cn(styles.root, className)}>{track}</span>;
  }

  return (
    <span className={cn(styles.root, styles.withLabel, className)}>
      <span className={styles.labelGroup}>
        <span className={styles.label}>{label}</span>
        {description && <span className={styles.description}>{description}</span>}
      </span>
      {track}
    </span>
  );
}
