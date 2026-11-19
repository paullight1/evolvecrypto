import type { TokenDistribution } from '../../types';
import styles from './DistributionChart.module.css';

const DISTRIBUTION: TokenDistribution[] = [
  { label: 'Public Sale', pct: 30, color: '#3b82f6' },
  { label: 'Team & Advisors', pct: 20, color: '#8b5cf6' },
  { label: 'Ecosystem Fund', pct: 25, color: '#10b981' },
  { label: 'Insurance Pool', pct: 10, color: '#f59e0b' },
  { label: 'Marketing', pct: 10, color: '#ef4444' },
  { label: 'Reserve', pct: 5, color: '#6b7280' },
];

export function DistributionChart() {
  let cumulativePct = 0;

  const segments = DISTRIBUTION.map((d) => {
    const start = cumulativePct;
    cumulativePct += d.pct;
    return { ...d, start };
  });

  const radius = 60;
  const circumference = 2 * Math.PI * radius;

  return (
    <div className={styles.wrap}>
      <div className={styles.donutWrap}>
        <svg viewBox="0 0 160 160" className={styles.donut}>
          {segments.map((seg) => {
            const dashArray = (seg.pct / 100) * circumference;
            const dashOffset = circumference - (seg.start / 100) * circumference;
            return (
              <circle
                key={seg.label}
                cx="80"
                cy="80"
                r={radius}
                fill="none"
                stroke={seg.color}
                strokeWidth="28"
                strokeDasharray={`${dashArray} ${circumference - dashArray}`}
                strokeDashoffset={dashOffset}
                transform="rotate(-90 80 80)"
              />
            );
          })}
        </svg>
        <div className={styles.center}>
          <span className={styles.centerLabel}>EXC</span>
        </div>
      </div>

      <div className={styles.legend}>
        {DISTRIBUTION.map((d) => (
          <div key={d.label} className={styles.legendItem}>
            <span className={styles.legendDot} style={{ background: d.color }} />
            <span className={styles.legendLabel}>{d.label}</span>
            <span className={styles.legendPct}>{d.pct}%</span>
          </div>
        ))}
      </div>
    </div>
  );
}
