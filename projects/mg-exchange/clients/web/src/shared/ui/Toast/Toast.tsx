import { useToastStore, type ToastItem, type ToastType } from './store';
import styles from './Toast.module.css';
import { cn } from '@shared/utils/format';

function toastIcon(type: ToastType): string {
  switch (type) {
    case 'success': return '✓';
    case 'error': return '✕';
    case 'warning': return '⚠';
    case 'info': return 'ℹ';
  }
}

function ToastCard({ toast }: { toast: ToastItem }) {
  const removeToast = useToastStore((s) => s.removeToast);
  return (
    <div
      className={cn(styles.toast, styles[toast.type])}
      role="alert"
      aria-live="polite"
    >
      <span className={styles.icon}>{toastIcon(toast.type)}</span>
      <span className={styles.message}>{toast.message}</span>
      <button
        className={styles.dismiss}
        onClick={() => removeToast(toast.id)}
        aria-label="Dismiss notification"
      >
        ✕
      </button>
    </div>
  );
}

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  if (toasts.length === 0) return null;

  return (
    <div className={styles.container} aria-label="Notifications">
      {toasts.map((t) => (
        <ToastCard key={t.id} toast={t} />
      ))}
    </div>
  );
}
