import type { ReactNode } from 'react';
import { Header } from './Header';
import styles from './TerminalLayout.module.css';

interface Props {
  children: ReactNode;
}

/**
 * Terminal layout: header + content fills the full viewport height with
 * overflow hidden. No footer — the trading terminal uses every pixel.
 */
export function TerminalLayout({ children }: Props) {
  return (
    <div className={styles.layout}>
      <Header />
      <div className={styles.content}>{children}</div>
    </div>
  );
}
