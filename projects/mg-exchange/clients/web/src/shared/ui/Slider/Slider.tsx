import type { ChangeEvent } from 'react';
import { useId } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Slider.module.css';

export interface SliderMark {
  value: number;
  label?: string;
}

export interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  marks?: SliderMark[];
  label?: string;
  showValue?: boolean;
  formatValue?: (value: number) => string;
  disabled?: boolean;
  className?: string;
}

export function Slider({
  value,
  onChange,
  min = 0,
  max = 100,
  step = 1,
  marks,
  label,
  showValue = false,
  formatValue,
  disabled = false,
  className,
}: SliderProps) {
  const id = useId();
  const labelId = label ? `${id}-label` : undefined;

  const range = max - min;
  const pct = range > 0 ? ((value - min) / range) * 100 : 0;

  function handleChange(e: ChangeEvent<HTMLInputElement>) {
    onChange(Number(e.target.value));
  }

  const displayValue = formatValue ? formatValue(value) : String(value);

  return (
    <div className={cn(styles.wrapper, disabled && styles.disabled, className)}>
      {(label || showValue) && (
        <div className={styles.header}>
          {label && (
            <span id={labelId} className={styles.label}>
              {label}
            </span>
          )}
          {showValue && (
            <span className={styles.valueDisplay}>{displayValue}</span>
          )}
        </div>
      )}

      <div className={styles.trackWrapper}>
        {/* Native input sits on top and handles all keyboard/mouse interaction */}
        <input
          type="range"
          id={id}
          className={styles.input}
          min={min}
          max={max}
          step={step}
          value={value}
          disabled={disabled}
          onChange={handleChange}
          aria-labelledby={labelId}
          aria-valuemin={min}
          aria-valuemax={max}
          aria-valuenow={value}
          aria-valuetext={displayValue}
          style={
            {
              '--fill-pct': `${pct}%`,
            } as React.CSSProperties
          }
        />

        {/* Mark dots rendered absolutely over the track */}
        {marks && marks.length > 0 && (
          <div className={styles.marks} aria-hidden="true">
            {marks.map((mark) => {
              const markPct =
                range > 0 ? ((mark.value - min) / range) * 100 : 0;
              const isFilled = mark.value <= value;
              return (
                <span
                  key={mark.value}
                  className={cn(styles.markDot, isFilled && styles.markDotFilled)}
                  style={{ left: `${markPct}%` }}
                />
              );
            })}
          </div>
        )}
      </div>

      {/* Mark labels in a separate row below the track */}
      {marks && marks.some((m) => m.label) && (
        <div className={styles.markLabels} aria-hidden="true">
          {marks.map((mark) => {
            if (!mark.label) return null;
            const markPct =
              range > 0 ? ((mark.value - min) / range) * 100 : 0;
            return (
              <span
                key={mark.value}
                className={styles.markLabel}
                style={{ left: `${markPct}%` }}
              >
                {mark.label}
              </span>
            );
          })}
        </div>
      )}
    </div>
  );
}
