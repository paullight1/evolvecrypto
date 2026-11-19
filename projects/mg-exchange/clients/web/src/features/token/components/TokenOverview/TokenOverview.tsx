import { Card } from '@shared/ui/Card';
import type { TokenSupplyInfo } from '../../types';
import styles from './TokenOverview.module.css';

const UTILITIES = [
  'Trading fee discounts (up to 25%)',
  'Insurance pool staking rewards',
  'Governance voting rights',
  'Launchpad allocation boosts',
  'VIP tier eligibility',
];

interface TokenOverviewProps {
  supply: TokenSupplyInfo | null;
}

export function TokenOverview({ supply }: TokenOverviewProps) {
  return (
    <div className={styles.wrap}>
      <div className={styles.statsGrid}>
        <Card className={styles.statCard}>
          <p className={styles.label}>Price</p>
          <p className={styles.value}>${supply?.price_usdt ?? '—'}</p>
        </Card>
        <Card className={styles.statCard}>
          <p className={styles.label}>Market Cap</p>
          <p className={styles.value}>${supply ? parseFloat(supply.market_cap_usdt).toLocaleString() : '—'}</p>
        </Card>
        <Card className={styles.statCard}>
          <p className={styles.label}>Circulating Supply</p>
          <p className={styles.value}>{supply ? parseFloat(supply.circulating_supply).toLocaleString() : '—'} EXC</p>
        </Card>
        <Card className={styles.statCard}>
          <p className={styles.label}>Total Burned</p>
          <p className={styles.value}>{supply ? parseFloat(supply.burned_total).toLocaleString() : '—'} EXC</p>
        </Card>
      </div>

      <Card>
        <h3 className={styles.utilTitle}>Token Utilities</h3>
        <ul className={styles.utilities}>
          {UTILITIES.map((u) => (
            <li key={u} className={styles.utilItem}>
              <span className={styles.bullet}>•</span>
              {u}
            </li>
          ))}
        </ul>
      </Card>
    </div>
  );
}
