import { useState } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Avatar.module.css';

export type AvatarSize = 'xs' | 'sm' | 'md' | 'lg';

export interface AvatarProps {
  src?: string;
  alt?: string;
  name?: string;
  size?: AvatarSize;
  online?: boolean;
  className?: string;
}

// Deterministic color index from name: cycle accent, info, purple, warning, long
const COLOR_VARS = ['--accent', '--info', '--purple', '--warning', '--long'] as const;

function getColorVar(name: string): string {
  let sum = 0;
  for (let i = 0; i < name.length; i++) {
    sum += name.charCodeAt(i);
  }
  return COLOR_VARS[sum % COLOR_VARS.length];
}

function getInitial(name: string | undefined): string {
  if (!name || name.trim().length === 0) return '?';
  return name.trim()[0].toUpperCase();
}

export function Avatar({
  src,
  alt,
  name,
  size = 'md',
  online = false,
  className,
}: AvatarProps) {
  const [imgError, setImgError] = useState(false);

  const showImage = src !== undefined && src.length > 0 && !imgError;
  const colorVar = name ? getColorVar(name) : '--accent';
  const initial = getInitial(name);

  return (
    <span
      className={cn(styles.avatar, styles[size], className)}
      aria-label={alt ?? name ?? 'avatar'}
    >
      {showImage ? (
        <img
          src={src}
          alt={alt ?? name ?? ''}
          loading="lazy"
          className={styles.img}
          onError={() => setImgError(true)}
        />
      ) : (
        <span
          className={styles.initials}
          style={{ background: `var(${colorVar})`, color: 'var(--on-accent)' }}
          aria-hidden="true"
        >
          {initial}
        </span>
      )}
      {online && <span className={styles.online} aria-label="online" />}
    </span>
  );
}
