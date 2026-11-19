import { useState, useEffect, useRef } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Countdown.module.css';

export type CountdownFormat = 'dhms' | 'hms' | 'ms' | 'auto';

export interface CountdownProps {
  target: Date | number;
  format?: CountdownFormat;
  onComplete?: () => void;
  prefix?: string;
  completedText?: string;
  className?: string;
}

interface Timeparts {
  days: number;
  hours: number;
  minutes: number;
  seconds: number;
}

function getRemainingMs(target: Date | number): number {
  const targetMs = target instanceof Date ? target.getTime() : target;
  return Math.max(0, targetMs - Date.now());
}

function decompose(ms: number): Timeparts {
  const totalSeconds = Math.floor(ms / 1000);
  const seconds = totalSeconds % 60;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const minutes = totalMinutes % 60;
  const totalHours = Math.floor(totalMinutes / 60);
  const hours = totalHours % 24;
  const days = Math.floor(totalHours / 24);
  return { days, hours, minutes, seconds };
}

function pad2(n: number): string {
  return String(n).padStart(2, '0');
}

function formatTime(parts: Timeparts, fmt: CountdownFormat): string {
  const resolved: CountdownFormat =
    fmt === 'auto'
      ? parts.days > 0
        ? 'dhms'
        : parts.hours > 0
          ? 'hms'
          : 'ms'
      : fmt;

  switch (resolved) {
    case 'dhms':
      return `${parts.days}d ${pad2(parts.hours)}h ${pad2(parts.minutes)}m ${pad2(parts.seconds)}s`;
    case 'hms':
      return `${pad2(parts.hours)}:${pad2(parts.minutes)}:${pad2(parts.seconds)}`;
    case 'ms':
      return `${pad2(parts.minutes)}:${pad2(parts.seconds)}`;
  }
}

export function Countdown({
  target,
  format = 'auto',
  onComplete,
  prefix,
  completedText = 'Ended',
  className,
}: CountdownProps) {
  const [remainingMs, setRemainingMs] = useState(() => getRemainingMs(target));
  const completedFired = useRef(false);

  useEffect(() => {
    // Reset when target changes
    completedFired.current = false;
    setRemainingMs(getRemainingMs(target));

    const id = setInterval(() => {
      const ms = getRemainingMs(target);
      setRemainingMs(ms);

      if (ms === 0 && !completedFired.current) {
        completedFired.current = true;
        onComplete?.();
        clearInterval(id);
      }
    }, 1000);

    return () => {
      clearInterval(id);
    };
  }, [target, onComplete]);

  if (remainingMs === 0) {
    return (
      <span className={cn(styles.countdown, styles.completed, className)}>
        {completedText}
      </span>
    );
  }

  const parts = decompose(remainingMs);
  const text = formatTime(parts, format);

  return (
    <span className={cn(styles.countdown, className)}>
      {prefix != null && <span className={styles.prefix}>{prefix}</span>}
      <span className={styles.time}>{text}</span>
    </span>
  );
}
