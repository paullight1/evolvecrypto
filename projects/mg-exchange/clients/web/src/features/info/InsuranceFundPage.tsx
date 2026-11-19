import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { get } from '@shared/api/client';
import styles from './InfoPage.module.css';

interface InsuranceFund {
  balance: string;
  currency: string;
}

interface InsuranceEvent {
  id: string;
  type: string;
  amount: string;
  symbol: string;
  created_at: number;
}

interface EventRow extends Record<string, unknown> {
  id: string;
  type: string;
  symbol: string;
  amount: string;
  created_at: number;
}

const COLS: ColumnDef<EventRow>[] = [
  { key: 'type', header: 'Type' },
  { key: 'symbol', header: 'Symbol', width: '120px' },
  { key: 'amount', header: 'Amount', align: 'right' },
  { key: 'created_at', header: 'Date', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
];

export default function InsuranceFundPage() {
  const [fund, setFund] = useState<InsuranceFund | null>(null);
  const [events, setEvents] = useState<InsuranceEvent[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      get<InsuranceFund>('/insurance-fund'),
      get<InsuranceEvent[]>('/insurance-fund/history'),
    ])
      .then(([f, e]) => { setFund(f); setEvents(e); })
      .catch(() => { setFund(null); setEvents([]); })
      .finally(() => setLoading(false));
  }, []);

  const rows: EventRow[] = events.map((e) => ({
    id: e.id, type: e.type, symbol: e.symbol, amount: e.amount, created_at: e.created_at,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Insurance Fund</h1>
      {fund && (
        <div className={styles.porStats} style={{ gridTemplateColumns: 'repeat(2, 1fr)' }}>
          <div className={styles.porStat}>
            <div className={styles.porStatLabel}>Fund Balance</div>
            <div className={styles.porStatValue}>{fund.balance} {fund.currency}</div>
          </div>
        </div>
      )}
      <h2 style={{ fontSize: 'var(--text-sm)', fontWeight: 'var(--font-bold)', color: 'var(--text-1)', marginBottom: 'var(--pad-sm)' }}>
        Fund History
      </h2>
      <Table<EventRow> columns={COLS} data={rows} loading={loading} emptyText="No events" />
    </div>
  );
}
