import { useState, useRef, cloneElement, type ReactElement } from 'react';
import styles from './Tooltip.module.css';
import { cn } from '@shared/utils/format';

export interface TooltipProps {
  content: string;
  children: ReactElement;
  side?: 'top' | 'bottom';
}

export function Tooltip({ content, children, side = 'top' }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  function handleMouseEnter(): void {
    timerRef.current = setTimeout(() => setVisible(true), 200);
  }

  function handleMouseLeave(): void {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setVisible(false);
  }

  const child = cloneElement(children, {
    onMouseEnter: handleMouseEnter,
    onMouseLeave: handleMouseLeave,
  });

  return (
    <span className={styles.wrapper}>
      {child}
      {visible && (
        <span className={cn(styles.tooltip, styles[side])} role="tooltip">
          {content}
        </span>
      )}
    </span>
  );
}
