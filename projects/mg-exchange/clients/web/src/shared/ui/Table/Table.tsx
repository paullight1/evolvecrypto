import { useState } from 'react';
import { cn } from '@shared/utils/format';
import { Skeleton } from '../Skeleton';
import styles from './Table.module.css';

export interface ColumnDef<T = Record<string, unknown>> {
  key: string;
  header: string;
  sortable?: boolean;
  render?: (row: T, index: number) => React.ReactNode;
  width?: string;
  align?: 'left' | 'right' | 'center';
}

export interface TableProps<T extends Record<string, unknown>> {
  columns: ColumnDef<T>[];
  data: T[];
  loading?: boolean;
  emptyText?: string;
  /** Custom empty state element — overrides emptyText when provided. */
  emptyNode?: React.ReactNode;
  onRowClick?: (row: T) => void;
  className?: string;
}

type SortDir = 'asc' | 'desc';

interface SortState {
  key: string;
  dir: SortDir;
}

const SKELETON_ROW_COUNT = 5;

export function Table<T extends Record<string, unknown>>({
  columns,
  data,
  loading = false,
  emptyText = 'No data',
  emptyNode,
  onRowClick,
  className,
}: TableProps<T>) {
  const [sort, setSort] = useState<SortState | null>(null);

  function handleHeaderClick(col: ColumnDef<T>): void {
    if (!col.sortable) return;
    setSort((prev) => {
      if (prev?.key === col.key) {
        return { key: col.key, dir: prev.dir === 'asc' ? 'desc' : 'asc' };
      }
      return { key: col.key, dir: 'asc' };
    });
  }

  const sortedData = sort
    ? [...data].sort((a, b) => {
        const av = a[sort.key];
        const bv = b[sort.key];
        const aStr = String(av ?? '');
        const bStr = String(bv ?? '');
        const aNum = parseFloat(aStr);
        const bNum = parseFloat(bStr);
        if (isFinite(aNum) && isFinite(bNum)) {
          return sort.dir === 'asc' ? aNum - bNum : bNum - aNum;
        }
        return sort.dir === 'asc' ? aStr.localeCompare(bStr) : bStr.localeCompare(aStr);
      })
    : data;

  return (
    <div className={cn(styles.wrapper, className)}>
      <table className={styles.table}>
        <thead className={styles.thead}>
          <tr>
            {columns.map((col) => (
              <th
                key={col.key}
                className={cn(
                  styles.th,
                  col.sortable && styles.sortable,
                  col.align && styles[`align-${col.align}`],
                )}
                style={col.width ? { width: col.width } : undefined}
                onClick={() => handleHeaderClick(col)}
                aria-sort={
                  sort?.key === col.key
                    ? sort.dir === 'asc'
                      ? 'ascending'
                      : 'descending'
                    : undefined
                }
              >
                {col.header}
                {col.sortable && (
                  <span className={styles.sortIcon}>
                    {sort?.key === col.key ? (sort.dir === 'asc' ? ' ↑' : ' ↓') : ' ↕'}
                  </span>
                )}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {loading
            ? Array.from({ length: SKELETON_ROW_COUNT }, (_, i) => (
                <tr key={i} className={styles.skeletonRow}>
                  {columns.map((col) => (
                    <td key={col.key} className={styles.td}>
                      <Skeleton height={14} width="80%" />
                    </td>
                  ))}
                </tr>
              ))
            : sortedData.length === 0
              ? (
                <tr>
                  <td colSpan={columns.length} className={styles.empty}>
                    {emptyNode ?? emptyText}
                  </td>
                </tr>
              )
              : sortedData.map((row, i) => (
                <tr
                  key={i}
                  className={cn(styles.tr, onRowClick && styles.clickable)}
                  onClick={() => onRowClick?.(row)}
                >
                  {columns.map((col) => (
                    <td
                      key={col.key}
                      className={cn(styles.td, col.align && styles[`align-${col.align}`])}
                    >
                      {col.render ? col.render(row, i) : String(row[col.key] ?? '')}
                    </td>
                  ))}
                </tr>
              ))}
        </tbody>
      </table>
    </div>
  );
}
