import type { ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Card.module.css';

export type CardPadding = 'none' | 'sm' | 'md';
export type CardVariant = 'default' | 'elevated' | 'outlined';

export interface CardProps {
  children: ReactNode;
  padding?: CardPadding;
  variant?: CardVariant;
  className?: string;
}

export function Card({ children, padding = 'md', variant = 'default', className }: CardProps) {
  return (
    <div className={cn(styles.card, styles[`padding-${padding}`], styles[variant], className)}>
      {children}
    </div>
  );
}
