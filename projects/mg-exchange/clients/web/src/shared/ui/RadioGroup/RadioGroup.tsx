import { useRef } from 'react';
import { cn } from '@shared/utils/format';
import styles from './RadioGroup.module.css';

export interface RadioOption {
  value: string;
  label: string;
  description?: string;
  disabled?: boolean;
}

export type RadioGroupVariant = 'default' | 'button';
export type RadioGroupDirection = 'horizontal' | 'vertical';

export interface RadioGroupProps {
  options: RadioOption[];
  value: string;
  onChange: (value: string) => void;
  name: string;
  label?: string;
  variant?: RadioGroupVariant;
  direction?: RadioGroupDirection;
  size?: 'sm' | 'md';
  error?: string;
  className?: string;
}

export function RadioGroup({
  options,
  value,
  onChange,
  name,
  label,
  variant = 'default',
  direction = 'horizontal',
  size = 'md',
  error,
  className,
}: RadioGroupProps) {
  const groupId = `radio-group-${name}`;
  const labelId = label ? `${groupId}-label` : undefined;
  const inputRefs = useRef<(HTMLInputElement | null)[]>([]);

  function handleKeyDown(
    e: React.KeyboardEvent<HTMLInputElement>,
    index: number,
  ): void {
    const isHorizontal = direction === 'horizontal';
    const prevKey = isHorizontal ? 'ArrowLeft' : 'ArrowUp';
    const nextKey = isHorizontal ? 'ArrowRight' : 'ArrowDown';

    let targetIndex: number | null = null;

    if (e.key === nextKey) {
      targetIndex = (index + 1) % options.length;
    } else if (e.key === prevKey) {
      targetIndex = (index - 1 + options.length) % options.length;
    } else {
      return;
    }

    e.preventDefault();

    // Skip disabled options
    let attempts = 0;
    while (options[targetIndex].disabled && attempts < options.length) {
      targetIndex =
        e.key === nextKey
          ? (targetIndex + 1) % options.length
          : (targetIndex - 1 + options.length) % options.length;
      attempts++;
    }

    if (!options[targetIndex].disabled) {
      inputRefs.current[targetIndex]?.focus();
      onChange(options[targetIndex].value);
    }
  }

  return (
    <div className={cn(styles.wrapper, className)}>
      {label && (
        <span id={labelId} className={styles.groupLabel}>
          {label}
        </span>
      )}
      <div
        role="radiogroup"
        aria-labelledby={labelId}
        className={cn(
          styles.group,
          styles[direction],
          variant === 'button' && styles.buttonGroup,
          variant === 'button' && styles[size],
        )}
      >
        {options.map((option, i) => {
          const isSelected = option.value === value;
          const isDisabled = Boolean(option.disabled);

          if (variant === 'button') {
            return (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={isSelected}
                disabled={isDisabled}
                className={cn(
                  styles.buttonOption,
                  isSelected && styles.buttonSelected,
                  styles[size],
                )}
                onClick={() => !isDisabled && onChange(option.value)}
              >
                {option.label}
              </button>
            );
          }

          return (
            <label
              key={option.value}
              className={cn(styles.optionLabel, isDisabled && styles.optionDisabled)}
            >
              <span className={styles.radioControl}>
                <input
                  ref={(el) => { inputRefs.current[i] = el; }}
                  type="radio"
                  name={name}
                  value={option.value}
                  checked={isSelected}
                  disabled={isDisabled}
                  onChange={() => onChange(option.value)}
                  onKeyDown={(e) => handleKeyDown(e, i)}
                  className={styles.radioInput}
                />
                <span className={cn(styles.radioCircle, isSelected && styles.radioSelected)} />
              </span>
              <span className={styles.optionTextGroup}>
                <span className={styles.optionText}>{option.label}</span>
                {option.description && (
                  <span className={styles.optionDescription}>{option.description}</span>
                )}
              </span>
            </label>
          );
        })}
      </div>
      {error && (
        <span className={styles.error} role="alert">
          {error}
        </span>
      )}
    </div>
  );
}
