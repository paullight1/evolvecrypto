import {
  useState,
  useRef,
  useEffect,
  useCallback,
  type ReactNode,
  type KeyboardEvent,
} from 'react';
import { createPortal } from 'react-dom';
import { cn } from '@shared/utils/format';
import styles from './Dropdown.module.css';

export type DropdownPlacement = 'bottom-start' | 'bottom-end' | 'top-start' | 'top-end';

export interface DropdownProps {
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  placement?: DropdownPlacement;
  className?: string;
}

export interface DropdownItemProps {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  destructive?: boolean;
  icon?: ReactNode;
  className?: string;
}

export interface DropdownSeparatorProps {
  className?: string;
}

interface PanelPosition {
  top: number;
  left: number;
}

function computePosition(
  triggerRect: DOMRect,
  panelEl: HTMLElement,
  placement: DropdownPlacement,
): PanelPosition {
  const panelHeight = panelEl.offsetHeight;
  const panelWidth = panelEl.offsetWidth;
  const scrollX = window.scrollX;
  const scrollY = window.scrollY;

  let top: number;
  let left: number;

  if (placement.startsWith('bottom')) {
    top = triggerRect.bottom + scrollY + 4;
  } else {
    top = triggerRect.top + scrollY - panelHeight - 4;
  }

  if (placement.endsWith('start')) {
    left = triggerRect.left + scrollX;
  } else {
    left = triggerRect.right + scrollX - panelWidth;
  }

  return { top, left };
}

export function Dropdown({
  trigger,
  children,
  open: controlledOpen,
  onOpenChange,
  placement = 'bottom-start',
  className,
}: DropdownProps) {
  const isControlled = controlledOpen !== undefined;
  const [internalOpen, setInternalOpen] = useState(false);
  const open = isControlled ? controlledOpen : internalOpen;

  const triggerRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<PanelPosition>({ top: 0, left: 0 });

  const setOpen = useCallback(
    (next: boolean) => {
      if (!isControlled) setInternalOpen(next);
      onOpenChange?.(next);
    },
    [isControlled, onOpenChange],
  );

  const toggle = useCallback(() => setOpen(!open), [open, setOpen]);

  // Compute panel position whenever it opens or placement changes
  useEffect(() => {
    if (!open || !triggerRef.current || !panelRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    setPosition(computePosition(rect, panelRef.current, placement));
  }, [open, placement]);

  // Reposition on scroll/resize while open
  useEffect(() => {
    if (!open) return;
    function reposition() {
      if (!triggerRef.current || !panelRef.current) return;
      const rect = triggerRef.current.getBoundingClientRect();
      setPosition(computePosition(rect, panelRef.current, placement));
    }
    window.addEventListener('scroll', reposition, true);
    window.addEventListener('resize', reposition);
    return () => {
      window.removeEventListener('scroll', reposition, true);
      window.removeEventListener('resize', reposition);
    };
  }, [open, placement]);

  // Close on outside pointer-down
  useEffect(() => {
    if (!open) return;
    function handlePointerDown(e: PointerEvent) {
      const target = e.target as Node;
      const outsideTrigger = !triggerRef.current?.contains(target);
      const outsidePanel = !panelRef.current?.contains(target);
      if (outsideTrigger && outsidePanel) {
        setOpen(false);
      }
    }
    document.addEventListener('pointerdown', handlePointerDown);
    return () => document.removeEventListener('pointerdown', handlePointerDown);
  }, [open, setOpen]);

  // Close on Escape
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape') setOpen(false);
    }
    document.addEventListener('keydown', handleKey as unknown as EventListener);
    return () =>
      document.removeEventListener('keydown', handleKey as unknown as EventListener);
  }, [open, setOpen]);

  // Arrow-key focus navigation within the menu
  function handlePanelKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    e.preventDefault();
    const panel = panelRef.current;
    if (!panel) return;
    const items = Array.from(
      panel.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)'),
    );
    if (items.length === 0) return;
    const focused = document.activeElement as HTMLElement;
    const currentIndex = items.indexOf(focused as HTMLButtonElement);
    let nextIndex: number;
    if (e.key === 'ArrowDown') {
      nextIndex = currentIndex < items.length - 1 ? currentIndex + 1 : 0;
    } else {
      nextIndex = currentIndex > 0 ? currentIndex - 1 : items.length - 1;
    }
    items[nextIndex]?.focus();
  }

  const panel = open
    ? createPortal(
        <div
          ref={panelRef}
          role="menu"
          className={cn(styles.panel, className)}
          style={{ top: position.top, left: position.left }}
          onKeyDown={handlePanelKeyDown}
        >
          {children}
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <div ref={triggerRef} className={styles.trigger} onClick={toggle}>
        {trigger}
      </div>
      {panel}
    </>
  );
}

export function DropdownItem({
  children,
  onClick,
  disabled = false,
  destructive = false,
  icon,
  className,
}: DropdownItemProps) {
  return (
    <button
      role="menuitem"
      className={cn(
        styles.item,
        destructive && styles.destructive,
        disabled && styles.disabled,
        className,
      )}
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      tabIndex={disabled ? -1 : 0}
    >
      {icon && <span className={styles.icon}>{icon}</span>}
      <span>{children}</span>
    </button>
  );
}

export function DropdownSeparator({ className }: DropdownSeparatorProps) {
  return <hr className={cn(styles.separator, className)} />;
}
