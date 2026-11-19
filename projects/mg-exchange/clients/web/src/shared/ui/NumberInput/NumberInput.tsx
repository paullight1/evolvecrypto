import { useRef, type ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import styles from './NumberInput.module.css';

export type NumberInputSize = 'sm' | 'md';

export interface NumberInputProps {
  value: string;
  onChange: (value: string) => void;
  min?: number;
  max?: number;
  step?: number;
  precision?: number;
  label?: string;
  error?: string;
  prefix?: ReactNode;
  suffix?: ReactNode;
  size?: NumberInputSize;
  showStepper?: boolean;
  disabled?: boolean;
  placeholder?: string;
  className?: string;
}

const LONG_PRESS_DELAY_MS = 500;
const REPEAT_INTERVAL_MS = 100;

function parseNum(value: string): number {
  const n = parseFloat(value);
  return isNaN(n) ? 0 : n;
}

function formatValue(n: number, precision: number | undefined): string {
  if (precision !== undefined) {
    return n.toFixed(precision);
  }
  return String(n);
}

export function NumberInput({
  value,
  onChange,
  min,
  max,
  step = 1,
  precision,
  label,
  error,
  prefix,
  suffix,
  size = 'md',
  showStepper = true,
  disabled = false,
  placeholder,
  className,
}: NumberInputProps) {
  const inputId = label?.toLowerCase().replace(/\s+/g, '-');
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  function clamp(n: number): number {
    let result = n;
    if (min !== undefined && result < min) result = min;
    if (max !== undefined && result > max) result = max;
    return result;
  }

  function applyStep(direction: 1 | -1) {
    const current = parseNum(value);
    const next = clamp(current + direction * step);
    onChange(formatValue(next, precision));
  }

  function stopStepper() {
    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    if (timeoutRef.current !== null) {
      clearTimeout(timeoutRef.current);
      timeoutRef.current = null;
    }
  }

  function startStepper(direction: 1 | -1) {
    applyStep(direction);
    timeoutRef.current = setTimeout(() => {
      intervalRef.current = setInterval(() => applyStep(direction), REPEAT_INTERVAL_MS);
    }, LONG_PRESS_DELAY_MS);
  }

  function handleChange(raw: string) {
    // Allow: empty, optional leading minus, digits, single decimal dot
    if (raw === '' || raw === '-' || /^-?\d*\.?\d*$/.test(raw)) {
      onChange(raw);
    }
  }

  function handleBlur() {
    if (value === '' || value === '-') return;
    let n = parseNum(value);
    n = clamp(n);
    onChange(formatValue(n, precision));
  }

  return (
    <div className={styles.wrapper}>
      {label && (
        <label htmlFor={inputId} className={styles.label}>
          {label}
        </label>
      )}
      <div
        className={cn(
          styles.inputRow,
          styles[size],
          error ? styles.hasError : undefined,
          disabled ? styles.isDisabled : undefined,
        )}
      >
        {prefix && <span className={styles.prefix}>{prefix}</span>}
        <input
          id={inputId}
          className={cn(styles.input, className)}
          inputMode="decimal"
          value={value}
          placeholder={placeholder}
          disabled={disabled}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? `${inputId}-error` : undefined}
          onChange={(e) => handleChange(e.target.value)}
          onBlur={handleBlur}
        />
        {suffix && <span className={styles.suffix}>{suffix}</span>}
        {showStepper && (
          <div className={styles.stepper}>
            <button
              type="button"
              className={styles.stepBtn}
              tabIndex={-1}
              disabled={disabled}
              aria-label="Increment"
              onPointerDown={() => startStepper(1)}
              onPointerUp={stopStepper}
              onPointerLeave={stopStepper}
            >
              +
            </button>
            <button
              type="button"
              className={styles.stepBtn}
              tabIndex={-1}
              disabled={disabled}
              aria-label="Decrement"
              onPointerDown={() => startStepper(-1)}
              onPointerUp={stopStepper}
              onPointerLeave={stopStepper}
            >
              −
            </button>
          </div>
        )}
      </div>
      {error && (
        <span id={`${inputId}-error`} className={styles.error} role="alert">
          {error}
        </span>
      )}
    </div>
  );
}
