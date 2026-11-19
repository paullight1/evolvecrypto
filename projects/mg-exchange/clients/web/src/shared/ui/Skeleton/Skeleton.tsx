import type { CSSProperties } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Skeleton.module.css';

export type SkeletonVariant = 'text' | 'rect' | 'circle';

export interface SkeletonProps {
  width?: string | number;
  height?: string | number;
  variant?: SkeletonVariant;
  className?: string;
}

export function Skeleton({
  width,
  height,
  variant = 'rect',
  className,
}: SkeletonProps) {
  const style: CSSProperties = {};
  if (width !== undefined) style.width = typeof width === 'number' ? `${width}px` : width;
  if (height !== undefined) style.height = typeof height === 'number' ? `${height}px` : height;

  return (
    <span
      className={cn(styles.skeleton, styles[variant], className)}
      style={style}
      aria-hidden="true"
    />
  );
}
