import { Card } from '@shared/ui/Card';
import type { OptionGreeks } from '../../types';
import styles from './PortfolioGreeks.module.css';

interface PortfolioGreeksProps {
  greeks: OptionGreeks | null;
  loading: boolean;
}

interface GreekItemProps {
  label: string;
  value: string;
}

function GreekItem({ label, value }: GreekItemProps) {
  return (
    <div className={styles.item}>
      <span className={styles.label}>{label}</span>
      <span className={styles.value}>{value}</span>
    </div>
  );
}

export function PortfolioGreeks({ greeks, loading }: PortfolioGreeksProps) {
  if (loading) {
    return (
      <Card>
        <p className={styles.loading}>Loading portfolio Greeks...</p>
      </Card>
    );
  }

  if (!greeks) {
    return (
      <Card>
        <p className={styles.empty}>No Greek data available.</p>
      </Card>
    );
  }

  return (
    <Card className={styles.card}>
      <h3 className={styles.title}>Portfolio Greeks</h3>
      <div className={styles.grid}>
        <GreekItem label="Total Delta (Δ)" value={greeks.totalDelta} />
        <GreekItem label="Total Gamma (Γ)" value={greeks.totalGamma} />
        <GreekItem label="Total Theta (Θ)" value={greeks.totalTheta} />
        <GreekItem label="Total Vega (V)" value={greeks.totalVega} />
      </div>
    </Card>
  );
}
