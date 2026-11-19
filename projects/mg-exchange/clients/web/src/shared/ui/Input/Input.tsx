import type { InputHTMLAttributes, ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Input.module.css';

export type InputSize = 'sm' | 'md';

export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'prefix' | 'size'> {
  label?: string;
  error?: string;
  prefix?: ReactNode;
  suffix?: ReactNode;
  size?: InputSize;
}

export function Input({
  label,
  error,
  prefix,
  suffix,
  size = 'md',
  className,
  id,
  ...rest
}: InputProps) {
  const inputId = id ?? label?.toLowerCase().replace(/\s+/g, '-');

  return (
    <div className={styles.wrapper}>
      {label && (
        <label htmlFor={inputId} className={styles.label}>
          {label}
        </label>
      )}
      <div className={cn(styles.inputRow, styles[size], error ? styles.hasError : undefined)}>
        {prefix && <span className={styles.prefix}>{prefix}</span>}
        <input
          id={inputId}
          className={cn(styles.input, className)}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? `${inputId}-error` : undefined}
          {...rest}
        />
        {suffix && <span className={styles.suffix}>{suffix}</span>}
      </div>
      {error && (
        <span id={`${inputId}-error`} className={styles.error} role="alert">
          {error}
        </span>
      )}
    </div>
  );
}
