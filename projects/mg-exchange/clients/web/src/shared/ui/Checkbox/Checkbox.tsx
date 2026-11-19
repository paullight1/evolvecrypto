import { useRef, useEffect } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Checkbox.module.css';

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label?: string;
  description?: string;
  indeterminate?: boolean;
  disabled?: boolean;
  error?: string;
  className?: string;
}

export function Checkbox({
  checked,
  onChange,
  label,
  description,
  indeterminate = false,
  disabled = false,
  error,
  className,
}: CheckboxProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.indeterminate = indeterminate;
    }
  }, [indeterminate]);

  const ariaChecked = indeterminate ? 'mixed' : checked;

  return (
    <label
      className={cn(
        styles.wrapper,
        disabled && styles.disabled,
        error && styles.hasError,
        className,
      )}
    >
      <span className={styles.control}>
        <input
          ref={inputRef}
          type="checkbox"
          className={styles.input}
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          disabled={disabled}
          aria-checked={ariaChecked}
          aria-invalid={Boolean(error)}
        />
        <span
          className={cn(
            styles.box,
            checked && styles.checked,
            indeterminate && styles.indeterminate,
          )}
        />
      </span>
      {(label ?? description) && (
        <span className={styles.textGroup}>
          {label && <span className={styles.label}>{label}</span>}
          {description && <span className={styles.description}>{description}</span>}
        </span>
      )}
      {error && (
        <span className={styles.error} role="alert">
          {error}
        </span>
      )}
    </label>
  );
}
