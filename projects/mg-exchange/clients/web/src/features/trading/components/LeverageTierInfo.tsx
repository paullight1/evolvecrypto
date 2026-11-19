import styles from './LeverageTierInfo.module.css';

interface LeverageTier {
  range: string;
  maxLev: string;
  mmr: string;
}

// Tiers sourced from backend PerpetualConfig — applies to BTC/ETH and similar high-cap symbols.
// Lower-cap symbols cap out at 50x.
const DEFAULT_TIERS: LeverageTier[] = [
  { range: '0 - $2M',     maxLev: '100x', mmr: '0.5%' },
  { range: '$2M - $6M',   maxLev: '50x',  mmr: '1.0%' },
  { range: '$6M - $12M',  maxLev: '25x',  mmr: '2.0%' },
  { range: '$12M - $25M', maxLev: '15x',  mmr: '2.5%' },
  { range: '$25M+',       maxLev: '10x',  mmr: '5.0%' },
];

export interface LeverageTierInfoProps {
  symbol: string;
}

export function LeverageTierInfo({ symbol }: LeverageTierInfoProps) {
  // Future: fetch symbol-specific tiers from backend.
  // For now every perpetual uses the same tier structure.
  const tiers = DEFAULT_TIERS;

  return (
    <div className={styles.container}>
      <h4 className={styles.title}>Leverage &amp; Margin Tiers</h4>
      <p className={styles.symbol}>{symbol}</p>
      <p className={styles.note}>
        Max leverage decreases as position size grows to manage risk.
        Maintenance margin rate determines the liquidation threshold.
      </p>
      <table className={styles.table}>
        <thead>
          <tr>
            <th className={styles.th}>Position Notional</th>
            <th className={styles.th}>Max Leverage</th>
            <th className={styles.th}>Maint. Margin</th>
          </tr>
        </thead>
        <tbody>
          {tiers.map((t) => (
            <tr key={t.range} className={styles.tr}>
              <td className={styles.td}>{t.range}</td>
              <td className={`${styles.td} ${styles.accentMono}`}>{t.maxLev}</td>
              <td className={`${styles.td} ${styles.mono}`}>{t.mmr}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
