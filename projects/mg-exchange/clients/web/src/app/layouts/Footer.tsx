import { navigate } from '../Router';
import styles from './Footer.module.css';

export function Footer() {
  return (
    <footer className={styles.footer}>
      <nav className={styles.footerNav} aria-label="Footer navigation">
        <button className={styles.footerLink} type="button" onClick={() => navigate('fees')}>Fees</button>
        <button className={styles.footerLink} type="button" onClick={() => navigate('status')}>Status</button>
        <button className={styles.footerLink} type="button" onClick={() => navigate('help')}>Help</button>
        <button className={styles.footerLink} type="button" onClick={() => navigate('legal')}>Legal</button>
        <button className={styles.footerLink} type="button" onClick={() => navigate('api-docs')}>API Docs</button>
        <button className={styles.footerLink} type="button" onClick={() => navigate('proof-of-reserves')}>PoR</button>
        <button className={styles.footerLink} type="button" onClick={() => navigate('insurance-fund')}>Insurance Fund</button>
      </nav>
    </footer>
  );
}
