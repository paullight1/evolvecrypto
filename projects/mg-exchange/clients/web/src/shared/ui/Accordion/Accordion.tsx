import { useState, useRef } from 'react';
import type { ReactNode } from 'react';
import { cn } from '@shared/utils/format';
import styles from './Accordion.module.css';

export interface AccordionItem {
  key: string;
  title: ReactNode;
  content: ReactNode;
  disabled?: boolean;
}

export interface AccordionProps {
  items: AccordionItem[];
  defaultOpenKeys?: string[];
  multiple?: boolean;
  className?: string;
}

interface PanelProps {
  item: AccordionItem;
  isOpen: boolean;
  onToggle: (key: string) => void;
  index: number;
}

function AccordionPanel({ item, isOpen, onToggle, index }: PanelProps) {
  const contentRef = useRef<HTMLDivElement>(null);
  const panelId = `accordion-panel-${item.key}`;
  const headerId = `accordion-header-${item.key}`;

  // Force a reflow so the transition fires when going from 0 to scrollHeight.
  // We use a CSS grid trick: grid-template-rows 0fr / 1fr which does not
  // require knowing scrollHeight and animates smoothly.
  return (
    <div
      className={cn(styles.item, item.disabled && styles.disabled)}
      data-testid={`accordion-item-${index}`}
    >
      <button
        id={headerId}
        type="button"
        aria-expanded={isOpen}
        aria-controls={panelId}
        disabled={item.disabled}
        className={cn(styles.header, isOpen && styles.headerOpen)}
        onClick={() => onToggle(item.key)}
      >
        <span className={styles.title}>{item.title}</span>
        <span className={cn(styles.chevron, isOpen && styles.chevronOpen)} aria-hidden="true">
          ▼
        </span>
      </button>

      <div
        id={panelId}
        role="region"
        aria-labelledby={headerId}
        className={cn(styles.panelWrapper, isOpen && styles.panelWrapperOpen)}
      >
        <div ref={contentRef} className={styles.panelContent}>
          {item.content}
        </div>
      </div>
    </div>
  );
}

export function Accordion({
  items,
  defaultOpenKeys = [],
  multiple = false,
  className,
}: AccordionProps) {
  const [openKeys, setOpenKeys] = useState<Set<string>>(() => new Set(defaultOpenKeys));

  function toggle(key: string): void {
    setOpenKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        if (!multiple) {
          next.clear();
        }
        next.add(key);
      }
      return next;
    });
  }

  return (
    <div className={cn(styles.accordion, className)}>
      {items.map((item, i) => (
        <AccordionPanel
          key={item.key}
          item={item}
          isOpen={openKeys.has(item.key)}
          onToggle={toggle}
          index={i}
        />
      ))}
    </div>
  );
}
