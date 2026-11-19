import { useEffect, useRef } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Tabs.module.css';

export interface TabDef {
  key: string;
  label: string;
  badge?: number;
}

export interface TabsProps {
  tabs: TabDef[];
  activeKey: string;
  onChange: (key: string) => void;
  className?: string;
}

export function Tabs({ tabs, activeKey, onChange, className }: TabsProps) {
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  function handleKeyDown(e: React.KeyboardEvent<HTMLButtonElement>, index: number): void {
    let nextIndex = index;
    if (e.key === 'ArrowRight') {
      nextIndex = (index + 1) % tabs.length;
    } else if (e.key === 'ArrowLeft') {
      nextIndex = (index - 1 + tabs.length) % tabs.length;
    } else if (e.key === 'Home') {
      nextIndex = 0;
    } else if (e.key === 'End') {
      nextIndex = tabs.length - 1;
    } else {
      return;
    }
    e.preventDefault();
    tabRefs.current[nextIndex]?.focus();
    onChange(tabs[nextIndex].key);
  }

  useEffect(() => {
    tabRefs.current = tabRefs.current.slice(0, tabs.length);
  }, [tabs.length]);

  return (
    <div role="tablist" className={cn(styles.tabs, className)}>
      {tabs.map((tab, i) => (
        <button
          key={tab.key}
          ref={(el) => { tabRefs.current[i] = el; }}
          role="tab"
          type="button"
          aria-selected={activeKey === tab.key}
          tabIndex={activeKey === tab.key ? 0 : -1}
          className={cn(styles.tab, activeKey === tab.key && styles.active)}
          onClick={() => onChange(tab.key)}
          onKeyDown={(e) => handleKeyDown(e, i)}
        >
          {tab.label}
          {tab.badge !== undefined && tab.badge > 0 && (
            <span className={styles.badge}>{tab.badge}</span>
          )}
        </button>
      ))}
    </div>
  );
}
