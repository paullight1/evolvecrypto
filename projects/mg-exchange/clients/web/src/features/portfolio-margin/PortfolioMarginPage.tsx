import { useState, useEffect } from 'react';
import { Card } from '@shared/ui/Card';
import { Badge } from '@shared/ui/Badge';
import { Button } from '@shared/ui/Button';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { ConfirmDialog } from '@shared/ui/ConfirmDialog';
import {
  getPortfolioMarginStatus,
  enablePortfolioMargin,
  disablePortfolioMargin,
  getPortfolioAnalysis,
} from './api';
import type { PortfolioMarginStatus, PortfolioAnalysis, ScenarioAnalysis } from './types';
import styles from './PortfolioMarginPage.module.css';

interface ScenarioRow extends Record<string, unknown> {
  scenario: string;
  pnl: string;
  margin_required: string;
  margin_ratio: string;
}

const SCENARIO_COLUMNS: ColumnDef<ScenarioRow>[] = [
  { key: 'scenario', header: 'Scenario' },
  {
    key: 'pnl',
    header: 'Est. PnL',
    align: 'right',
    render: (row) => {
      const val = parseFloat(String(row.pnl));
      const color = val >= 0 ? 'var(--buy)' : 'var(--sell)';
      return <span style={{ color }}>{String(row.pnl)}</span>;
    },
  },
  { key: 'margin_required', header: 'Margin Required', align: 'right' },
  { key: 'margin_ratio', header: 'Margin Ratio', align: 'right' },
];

interface StatRowProps {
  label: string;
  value: string;
}

function StatRow({ label, value }: StatRowProps) {
  return (
    <div className={styles.statRow}>
      <span className={styles.statLabel}>{label}</span>
      <span className={styles.statValue}>{value}</span>
    </div>
  );
}

export default function PortfolioMarginPage() {
  const [status, setStatus] = useState<PortfolioMarginStatus | null>(null);
  const [analysis, setAnalysis] = useState<PortfolioAnalysis | null>(null);
  const [statusLoading, setStatusLoading] = useState(false);
  const [analysisLoading, setAnalysisLoading] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirmAction, setConfirmAction] = useState<'enable' | 'disable'>('enable');

  useEffect(() => {
    setStatusLoading(true);
    getPortfolioMarginStatus()
      .then(setStatus)
      .catch(() => setStatus(null))
      .finally(() => setStatusLoading(false));

    setAnalysisLoading(true);
    getPortfolioAnalysis()
      .then(setAnalysis)
      .catch(() => setAnalysis(null))
      .finally(() => setAnalysisLoading(false));
  }, []);

  function requestToggle(action: 'enable' | 'disable') {
    setConfirmAction(action);
    setConfirmOpen(true);
  }

  async function handleConfirm() {
    setConfirmOpen(false);
    setToggling(true);
    try {
      const next =
        confirmAction === 'enable'
          ? await enablePortfolioMargin()
          : await disablePortfolioMargin();
      setStatus(next);
    } catch {
      // status unchanged on error
    } finally {
      setToggling(false);
    }
  }

  const scenarioRows: ScenarioRow[] = (analysis?.scenarios ?? []).map(
    (s: ScenarioAnalysis) => ({ ...s }),
  );

  const isEnabled = status?.enabled ?? false;

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Portfolio Margin</h1>

      <div className={styles.topRow}>
        <Card className={styles.statusCard}>
          <div className={styles.statusHeader}>
            <span className={styles.statusTitle}>Portfolio Margin Mode</span>
            {statusLoading ? (
              <Badge variant="neutral">Loading...</Badge>
            ) : (
              <Badge variant={isEnabled ? 'success' : 'neutral'}>
                {isEnabled ? 'Enabled' : 'Disabled'}
              </Badge>
            )}
          </div>

          {status && (
            <div className={styles.statsGrid}>
              <StatRow label="Margin Level" value={status.margin_level} />
              <StatRow label="Initial Margin" value={status.initial_margin} />
              <StatRow label="Maintenance Margin" value={status.maintenance_margin} />
              <StatRow label="Available Balance" value={status.available_balance} />
              <StatRow label="Total Equity" value={status.total_equity} />
            </div>
          )}

          <div className={styles.toggleAction}>
            {isEnabled ? (
              <Button
                variant="danger"
                size="sm"
                loading={toggling}
                onClick={() => requestToggle('disable')}
              >
                Disable Portfolio Margin
              </Button>
            ) : (
              <Button
                variant="primary"
                size="sm"
                loading={toggling}
                onClick={() => requestToggle('enable')}
              >
                Enable Portfolio Margin
              </Button>
            )}
          </div>
        </Card>

        {analysis && (
          <Card className={styles.summaryCard}>
            <h3 className={styles.sectionTitle}>Risk Summary</h3>
            <div className={styles.statsGrid}>
              <StatRow label="Net Delta" value={analysis.net_delta} />
              <StatRow label="Net Gamma" value={analysis.net_gamma} />
              <StatRow label="Stress Loss" value={analysis.stress_loss} />
            </div>
          </Card>
        )}
      </div>

      <section className={styles.scenarios}>
        <h2 className={styles.sectionTitle}>Scenario Analysis</h2>
        <Table<ScenarioRow>
          columns={SCENARIO_COLUMNS}
          data={scenarioRows}
          loading={analysisLoading}
          emptyText="No scenario data available"
        />
      </section>

      <ConfirmDialog
        open={confirmOpen}
        title={confirmAction === 'enable' ? 'Enable Portfolio Margin' : 'Disable Portfolio Margin'}
        message={
          confirmAction === 'enable'
            ? 'Enabling portfolio margin allows cross-product netting. Are you sure?'
            : 'Disabling portfolio margin will revert to standard margin mode. Are you sure?'
        }
        confirmText={confirmAction === 'enable' ? 'Enable' : 'Disable'}
        variant={confirmAction === 'disable' ? 'danger' : 'warning'}
        onConfirm={() => { void handleConfirm(); }}
        onCancel={() => setConfirmOpen(false)}
      />
    </div>
  );
}
