import { useState, useEffect } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Button } from '@shared/ui/Button';
import { get } from '@shared/api/client';
import styles from './HistoryPage.module.css';

interface Statement {
  id: string;
  month: string;
  year: number;
  total_trades: number;
  net_pnl: string;
  download_url: string;
}

interface StmtRow extends Record<string, unknown> {
  id: string;
  month: string;
  year: number;
  total_trades: number;
  net_pnl: string;
  download_url: string;
}

const COLUMNS: ColumnDef<StmtRow>[] = [
  { key: 'month', header: 'Month' },
  { key: 'year', header: 'Year', width: '80px' },
  { key: 'total_trades', header: 'Trades', align: 'right' },
  { key: 'net_pnl', header: 'Net PnL', align: 'right' },
  { key: 'download_url', header: 'Download',
    render: (r) => (
      <Button variant="ghost" size="sm" onClick={() => { window.open(String(r.download_url), '_blank'); }}>
        Download
      </Button>
    ) },
];

export default function StatementsPage() {
  const [statements, setStatements] = useState<Statement[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    get<Statement[]>('/account/statements')
      .then(setStatements)
      .catch(() => setStatements([]))
      .finally(() => setLoading(false));
  }, []);

  const rows: StmtRow[] = statements.map((s) => ({
    id: s.id, month: s.month, year: s.year, total_trades: s.total_trades, net_pnl: s.net_pnl, download_url: s.download_url,
  }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Monthly Statements</h1>
      <Table<StmtRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No statements available" />
    </div>
  );
}
