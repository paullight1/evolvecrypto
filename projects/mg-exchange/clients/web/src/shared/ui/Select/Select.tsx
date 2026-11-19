import type { SelectHTMLAttributes } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Select.module.css';

export interface SelectOption {
  value: string;
  label: string;
}

export type SelectSize = 'sm' | 'md';

export interface SelectProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'size'> {
  options: SelectOption[];
  label?: string;
  size?: SelectSize;
  error?: string;
}

export function Select({ options, label, size = 'md', error, className, id, ...rest }: SelectProps) {
  const selectId = id ?? label?.toLowerCase().replace(/\s+/g, '-');

  return (
    <div className={styles.wrapper}>
      {label && (
        <label htmlFor={selectId} className={styles.label}>
          {label}
        </label>
      )}
      <select
        id={selectId}
        className={cn(styles.select, styles[size], error ? styles.hasError : undefined, className)}
        aria-invalid={Boolean(error)}
        aria-describedby={error ? `${selectId}-error` : undefined}
        {...rest}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
      {error && (
        <span id={`${selectId}-error`} className={styles.error} role="alert">
          {error}
        </span>
      )}
    </div>
  );
}
