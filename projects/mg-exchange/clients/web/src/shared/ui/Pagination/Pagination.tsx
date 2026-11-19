import { cn } from '@shared/utils/format';
import styles from './Pagination.module.css';

export interface PaginationProps {
  page: number;
  totalPages: number;
  onChange: (p: number) => void;
  className?: string;
}

export function Pagination({ page, totalPages, onChange, className }: PaginationProps) {
  const hasPrev = page > 1;
  const hasNext = page < totalPages;

  return (
    <div className={cn(styles.pagination, className)} role="navigation" aria-label="Pagination">
      <button
        type="button"
        className={styles.btn}
        disabled={!hasPrev}
        onClick={() => onChange(page - 1)}
        aria-label="Previous page"
      >
        ‹
      </button>
      <span className={styles.info} aria-live="polite">
        {page} / {totalPages}
      </span>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasNext}
        onClick={() => onChange(page + 1)}
        aria-label="Next page"
      >
        ›
      </button>
    </div>
  );
}
