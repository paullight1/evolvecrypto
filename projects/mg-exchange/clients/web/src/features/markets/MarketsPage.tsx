import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { StatCard } from '@shared/ui/StatCard';
import { Spinner } from '@shared/ui/Spinner';
import { Badge } from '@shared/ui/Badge';
import { MarketsTable } from './components/MarketsTable';
import {
  getAllMarkets, getFundingRates, getInsuranceFund,
  getLongShortRatio, getPublicLiquidations,
  type FundingRate, type InsuranceFundData, type LongShortRatio, type LiquidationEvent,
} from './api';
import type { Market } from './types';
import styles from './MarketsPage.module.css';

const TABS: TabDef[] = [
  { key: 'all', label: 'All Markets' },
  { key: 'spot', label: 'Spot' },
  { key: 'perpetual', label: 'Perpetual' },
  { key: 'funding', label: 'Funding Rates' },
  { key: 'insurance', label: 'Insurance Fund' },
  { key: 'liquidations', label: 'Liquidations' },
];

function formatPct(raw: string): string {
  const n = parseFloat(raw);
  if (!isFinite(n)) return '—';
  return `${(n * 100).toFixed(4)}%`;
}

function formatUsd(raw: string): string {
  const n = parseFloat(raw);
  if (!isFinite(n)) return '—';
  if (n >= 1e9) return `$${(n / 1e9).toFixed(2)}B`;
  if (n >= 1e6) return `$${(n / 1e6).toFixed(2)}M`;
  if (n >= 1e3) return `$${(n / 1e3).toFixed(1)}K`;
  return `$${n.toFixed(2)}`;
}

function formatTime(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString();
}

const FUNDING_COLS: ColumnDef<FundingRate>[] = [
  { key: 'symbol', header: 'Symbol', sortable: true },
  {
    key: 'funding_rate', header: 'Funding Rate', align: 'right', sortable: true,
    render: (row) => {
      const n = parseFloat(row.funding_rate);
      const color = n > 0 ? 'var(--long)' : n < 0 ? 'var(--short)' : 'var(--text-1)';
      return <span style={{ color }}>{formatPct(row.funding_rate)}</span>;
    },
  },
  {
    key: 'next_funding_time', header: 'Next Funding', align: 'right',
    render: (row) => formatTime(row.next_funding_time),
  },
];

const LIQ_COLS: ColumnDef<LiquidationEvent>[] = [
  { key: 'symbol', header: 'Symbol' },
  {
    key: 'side', header: 'Side',
    render: (row) => <Badge variant={row.side === 'Buy' ? 'success' : 'danger'}>{row.side}</Badge>,
  },
  { key: 'price', header: 'Price', align: 'right' },
  { key: 'qty', header: 'Qty', align: 'right' },
  { key: 'time', header: 'Time', align: 'right', render: (row) => formatTime(row.time) },
];

export default function MarketsPage() {
  const [tab, setTab] = useState('all');
  const [markets, setMarkets] = useState<Market[]>([]);
  const [loading, setLoading] = useState(true);
  const [fundingRates, setFundingRates] = useState<FundingRate[]>([]);
  const [insurance, setInsurance] = useState<InsuranceFundData | null>(null);
  const [lsRatio, setLsRatio] = useState<LongShortRatio[]>([]);
  const [liquidations, setLiquidations] = useState<LiquidationEvent[]>([]);

  useEffect(() => {
    getAllMarkets()
      .then(setMarkets)
      .catch(() => setMarkets([]))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    if (tab === 'funding') {
      getFundingRates().then(setFundingRates).catch(() => setFundingRates([]));
      getLongShortRatio().then(setLsRatio).catch(() => setLsRatio([]));
    }
    if (tab === 'insurance') {
      getInsuranceFund().then(setInsurance).catch(() => setInsurance(null));
    }
    if (tab === 'liquidations') {
      getPublicLiquidations('BTCUSDT', 50).then(setLiquidations).catch(() => setLiquidations([]));
    }
  }, [tab]);

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Markets</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      {(tab === 'all' || tab === 'spot' || tab === 'perpetual') && (
        <MarketsTable markets={markets} loading={loading} category={tab} />
      )}

      {tab === 'funding' && (
        <>
          {lsRatio.length > 0 && (
            <div className={styles.statsRow}>
              {lsRatio.slice(0, 4).map((r) => (
                <StatCard
                  key={r.symbol}
                  label={`${r.symbol} L/S`}
                  value={`${(parseFloat(r.buy_ratio) * 100).toFixed(1)}% / ${(parseFloat(r.sell_ratio) * 100).toFixed(1)}%`}
                />
              ))}
            </div>
          )}
          <Table<FundingRate>
            columns={FUNDING_COLS}
            data={fundingRates}
            emptyText="No funding rates"
          />
        </>
      )}

      {tab === 'insurance' && (
        insurance ? (
          <div>
            <div className={styles.statsRow}>
              <StatCard label="Insurance Fund Balance" value={formatUsd(insurance.balance_usd)} />
              <StatCard label="Raw Balance" value={insurance.balance} />
            </div>
            {insurance.history.length > 0 && (
              <Table
                columns={[
                  { key: 'time', header: 'Time', render: (r: { time: number }) => new Date(r.time * 1000).toLocaleDateString() },
                  { key: 'balance', header: 'Balance', align: 'right' as const },
                ]}
                data={insurance.history}
                emptyText="No history"
              />
            )}
          </div>
        ) : (
          <Spinner size="lg" />
        )
      )}

      {tab === 'liquidations' && (
        <Table<LiquidationEvent>
          columns={LIQ_COLS}
          data={liquidations}
          emptyText="No recent liquidations"
        />
      )}
    </div>
  );
}
