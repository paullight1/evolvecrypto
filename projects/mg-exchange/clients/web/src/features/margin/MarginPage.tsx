import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { MarginStatusCard } from './components/MarginStatusCard/MarginStatusCard';
import { BorrowForm } from './components/BorrowForm/BorrowForm';
import { RepayForm } from './components/RepayForm/RepayForm';
import { BorrowTable } from './components/BorrowTable/BorrowTable';
import { getMarginStatus, getActiveBorrows, getInterestHistory } from './api';
import type { MarginStatus, MarginBorrow, InterestRecord } from './types';
import styles from './MarginPage.module.css';

const TABS: TabDef[] = [
  { key: 'borrow', label: 'Borrow' },
  { key: 'repay', label: 'Repay' },
  { key: 'active', label: 'Active Borrows' },
  { key: 'interest', label: 'Interest History' },
];

interface InterestRow extends Record<string, unknown> {
  asset: string;
  amount: string;
  rate: string;
  timestamp: number;
}

const INTEREST_COLUMNS: ColumnDef<InterestRow>[] = [
  { key: 'asset', header: 'Asset', width: '80px' },
  { key: 'amount', header: 'Interest Paid', align: 'right' },
  { key: 'rate', header: 'Rate', align: 'right' },
  {
    key: 'timestamp',
    header: 'Date',
    render: (row) => new Date((row.timestamp as number) * 1000).toLocaleString(),
  },
];

export default function MarginPage() {
  const [tab, setTab] = useState('borrow');
  const [status, setStatus] = useState<MarginStatus | null>(null);
  const [borrows, setBorrows] = useState<MarginBorrow[]>([]);
  const [interest, setInterest] = useState<InterestRecord[]>([]);
  const [statusLoading, setStatusLoading] = useState(false);
  const [borrowsLoading, setBorrowsLoading] = useState(false);
  const [interestLoading, setInterestLoading] = useState(false);

  useEffect(() => {
    setStatusLoading(true);
    getMarginStatus()
      .then(setStatus)
      .catch(() => setStatus(null))
      .finally(() => setStatusLoading(false));
  }, []);

  useEffect(() => {
    if (tab !== 'active' && tab !== 'repay') return;
    setBorrowsLoading(true);
    getActiveBorrows()
      .then(setBorrows)
      .catch(() => setBorrows([]))
      .finally(() => setBorrowsLoading(false));
  }, [tab]);

  useEffect(() => {
    if (tab !== 'interest') return;
    setInterestLoading(true);
    getInterestHistory(50)
      .then(setInterest)
      .catch(() => setInterest([]))
      .finally(() => setInterestLoading(false));
  }, [tab]);

  const interestRows: InterestRow[] = interest.map((r) => ({ ...r }));

  return (
    <div className={styles.page}>
      <div className={styles.topSection}>
        <h1 className={styles.title}>Spot Margin</h1>
        <MarginStatusCard status={status} loading={statusLoading} />
      </div>

      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'borrow' && <BorrowForm />}
        {tab === 'repay' && <RepayForm borrows={borrows} />}
        {tab === 'active' && <BorrowTable borrows={borrows} loading={borrowsLoading} />}
        {tab === 'interest' && (
          <Table<InterestRow>
            columns={INTEREST_COLUMNS}
            data={interestRows}
            loading={interestLoading}
            emptyText="No interest history"
          />
        )}
      </div>
    </div>
  );
}
