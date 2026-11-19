import type { ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Alert.module.css';

export type AlertVariant = 'info' | 'warning' | 'error' | 'success';

export interface AlertProps {
  variant: AlertVariant;
  title?: string;
  children: ReactNode;
  dismissible?: boolean;
  onDismiss?: () => void;
  action?: ReactNode;
  className?: string;
}

const ALERT_ROLES: Record<AlertVariant, 'alert' | 'status'> = {
  error: 'alert',
  warning: 'alert',
  info: 'status',
  success: 'status',
};

export function Alert({
  variant,
  title,
  children,
  dismissible = false,
  onDismiss,
  action,
  className,
}: AlertProps) {
  return (
    <div
      className={cn(styles.root, styles[variant], className)}
      role={ALERT_ROLES[variant]}
    >
      <div className={styles.body}>
        {title && <p className={styles.title}>{title}</p>}
        <div className={styles.content}>{children}</div>
        {action && <div className={styles.action}>{action}</div>}
      </div>
      {dismissible && (
        <button
          className={styles.dismiss}
          onClick={onDismiss}
          aria-label="Dismiss"
          type="button"
        >
          ✕
        </button>
      )}
    </div>
  );
}
