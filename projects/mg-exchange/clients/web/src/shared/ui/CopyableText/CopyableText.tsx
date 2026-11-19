import { useState, useCallback } from 'react';
import { cn } from '@shared/utils/format';
import styles from './CopyableText.module.css';

export interface CopyableTextProps {
  value: string;
  displayValue?: string;
  className?: string;
}

const COPIED_RESET_MS = 2000;

export function CopyableText({ value, displayValue, className }: CopyableTextProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    void navigator.clipboard.writeText(value).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), COPIED_RESET_MS);
    });
  }, [value]);

  return (
    <span className={cn(styles.wrapper, className)}>
      <span className={styles.text}>{displayValue ?? value}</span>
      <button
        type="button"
        className={styles.copyBtn}
        onClick={handleCopy}
        aria-label={copied ? 'Copied' : 'Copy to clipboard'}
        title={copied ? 'Copied!' : 'Copy'}
      >
        {copied ? <span className={styles.check}>✓</span> : <span className={styles.icon}>⎘</span>}
      </button>
      {copied && <span className={styles.feedback} aria-live="polite">Copied!</span>}
    </span>
  );
}
