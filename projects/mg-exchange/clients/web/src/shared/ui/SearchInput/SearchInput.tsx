import { useRef, useEffect, useState } from 'react';
import { Input } from '@shared/ui/Input';
import type { InputSize } from '@shared/ui/Input/Input';
import styles from './SearchInput.module.css';

export interface SearchInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  onClear?: () => void;
  debounceMs?: number;
  size?: InputSize;
  autoFocus?: boolean;
  className?: string;
}

const SearchIcon = () => (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    aria-hidden="true"
  >
    <circle cx="11" cy="11" r="8" />
    <path d="M21 21l-4.35-4.35" />
  </svg>
);

const ClearIcon = () => (
  <svg
    width="12"
    height="12"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2.5"
    aria-hidden="true"
  >
    <path d="M18 6L6 18M6 6l12 12" />
  </svg>
);

export function SearchInput({
  value,
  onChange,
  placeholder = 'Search...',
  onClear,
  debounceMs = 0,
  size = 'md',
  autoFocus = false,
  className,
}: SearchInputProps) {
  // Internal display value used when debounce is active so the input
  // is not visually delayed while the caller's state catches up.
  const [displayValue, setDisplayValue] = useState(value);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wrapperRef = useRef<HTMLDivElement>(null);

  // Sync external value into display value when not debouncing
  useEffect(() => {
    if (debounceMs === 0) {
      setDisplayValue(value);
    }
  }, [value, debounceMs]);

  // autoFocus: native prop handles initial focus; use effect for programmatic focus
  useEffect(() => {
    if (autoFocus && wrapperRef.current) {
      const input = wrapperRef.current.querySelector('input');
      input?.focus();
    }
  }, [autoFocus]);

  // Flush pending debounce on unmount to prevent stale state updates
  useEffect(() => {
    return () => {
      if (debounceRef.current !== null) {
        clearTimeout(debounceRef.current);
      }
    };
  }, []);

  function handleChange(raw: string) {
    if (debounceMs > 0) {
      setDisplayValue(raw);
      if (debounceRef.current !== null) {
        clearTimeout(debounceRef.current);
      }
      debounceRef.current = setTimeout(() => {
        onChange(raw);
      }, debounceMs);
    } else {
      onChange(raw);
    }
  }

  function handleClear() {
    if (debounceRef.current !== null) {
      clearTimeout(debounceRef.current);
    }
    setDisplayValue('');
    onChange('');
    onClear?.();
    wrapperRef.current?.querySelector('input')?.focus();
  }

  const currentValue = debounceMs > 0 ? displayValue : value;

  const clearButton = currentValue.length > 0 ? (
    <button
      type="button"
      className={styles.clearBtn}
      onClick={handleClear}
      aria-label="Clear search"
      tabIndex={-1}
    >
      <ClearIcon />
    </button>
  ) : undefined;

  return (
    <div ref={wrapperRef}>
      <Input
        type="search"
        role="searchbox"
        size={size}
        placeholder={placeholder}
        value={currentValue}
        prefix={<SearchIcon />}
        suffix={clearButton}
        className={className}
        onChange={(e) => handleChange(e.target.value)}
      />
    </div>
  );
}
