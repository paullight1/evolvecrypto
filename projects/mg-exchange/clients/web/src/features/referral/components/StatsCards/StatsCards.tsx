import { Card } from '@shared/ui/Card';
import { StatCard } from '@shared/ui/StatCard';
import { CopyableText } from '@shared/ui/CopyableText';
import type { ReferralStats } from '../../types';
import styles from './StatsCards.module.css';

interface StatsCardsProps {
  stats: ReferralStats;
}

export function StatsCards({ stats }: StatsCardsProps) {
  return (
    <div className={styles.wrap}>
      <Card className={styles.codeCard}>
        <p className={styles.codeLabel}>Your Referral Code</p>
        <CopyableText value={stats.referral_code} className={styles.code} />
        <p className={styles.urlLabel}>Referral Link</p>
        <CopyableText value={stats.referral_url} displayValue={stats.referral_url.slice(0, 40) + '…'} />
      </Card>

      <div className={styles.statsGrid}>
        <StatCard label="Total Referrals" value={stats.total_referrals} />
        <StatCard label="Active Referrals" value={stats.active_referrals} />
        <StatCard
          label="Total Commissions"
          value={parseFloat(stats.total_commissions_usdt).toFixed(2)}
          prefix="$"
        />
        <StatCard label="Commission Rate" value={stats.commission_rate} suffix="%" />
      </div>
    </div>
  );
}
