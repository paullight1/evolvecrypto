import type { ReactNode } from 'react';
import { Header } from './Header';
import { Footer } from './Footer';
import styles from './DashboardLayout.module.css';

interface Props {
  children: ReactNode;
}

/**
 * Dashboard layout: header + scrollable content area + footer.
 * Used for all pages except the trading terminal and full-page experiences.
 */
export function DashboardLayout({ children }: Props) {
  return (
    <div className={styles.layout}>
      <Header />
      <main className={styles.content}>{children}</main>
      <Footer />
    </div>
  );
}
