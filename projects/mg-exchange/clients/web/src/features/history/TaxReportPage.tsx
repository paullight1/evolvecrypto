import { useState } from 'react';
import { Card } from '@shared/ui/Card';
import { Select } from '@shared/ui/Select';
import { Button } from '@shared/ui/Button';
import { toast } from '@shared/ui/Toast';
import { getTaxReport } from './api';
import type { TaxReport } from './types';
import styles from './HistoryPage.module.css';

const YEAR_OPTIONS = [2026, 2025, 2024, 2023].map((y) => ({ value: String(y), label: String(y) }));

export default function TaxReportPage() {
  const [year, setYear] = useState('2025');
  const [report, setReport] = useState<TaxReport | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleGenerate() {
    setLoading(true);
    try {
      const result = await getTaxReport(parseInt(year));
      setReport(result);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : 'Failed to generate report');
    } finally {
      setLoading(false);
    }
  }

  function handleDownload() {
    if (!report) return;
    const csv = `Year,Total Gains,Total Fees,Trade Count\n${report.year},${report.total_gains},${report.total_fees},${report.trade_count}`;
    const blob = new Blob([csv], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `tax-report-${report.year}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Tax Report</h1>
      <Card padding="md" className={styles.taxCard}>
        <div style={{ display: 'flex', gap: 'var(--pad-sm)', alignItems: 'flex-end' }}>
          <Select label="Year" options={YEAR_OPTIONS} value={year} onChange={(e) => setYear(e.target.value)} />
          <Button variant="primary" loading={loading} onClick={() => { void handleGenerate(); }}>Generate</Button>
        </div>

        {report && (
          <>
            <div className={styles.taxStats}>
              <div className={styles.taxStat}>
                <span className={styles.taxStatLabel}>Total Gains</span>
                <span className={styles.taxStatValue}>{report.total_gains}</span>
              </div>
              <div className={styles.taxStat}>
                <span className={styles.taxStatLabel}>Total Fees</span>
                <span className={styles.taxStatValue}>{report.total_fees}</span>
              </div>
              <div className={styles.taxStat}>
                <span className={styles.taxStatLabel}>Trade Count</span>
                <span className={styles.taxStatValue}>{report.trade_count.toLocaleString()}</span>
              </div>
            </div>
            <div>
              <Button variant="secondary" onClick={handleDownload}>Download CSV</Button>
            </div>
          </>
        )}
      </Card>
    </div>
  );
}
