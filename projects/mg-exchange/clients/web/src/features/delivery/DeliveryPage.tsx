import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { InstrumentList } from './components/InstrumentList/InstrumentList';
import { DeliveryOrderEntry } from './components/DeliveryOrderEntry/DeliveryOrderEntry';
import { DeliveryPositions } from './components/DeliveryPositions/DeliveryPositions';
import {
  getDeliveryInstruments,
  getDeliveryPositions,
  getSettlementPrices,
} from './api';
import type { DeliveryInstrument, DeliveryPosition, SettlementRecord } from './types';
import styles from './DeliveryPage.module.css';

const TABS: TabDef[] = [
  { key: 'instruments', label: 'Instruments' },
  { key: 'positions', label: 'Positions' },
  { key: 'settlements', label: 'Settlement History' },
];

interface SettlementRow extends Record<string, unknown> {
  symbol: string;
  settlement_price: string;
  settled_at: number;
  pnl: string;
}

const SETTLEMENT_COLUMNS: ColumnDef<SettlementRow>[] = [
  { key: 'symbol', header: 'Symbol', sortable: true },
  { key: 'settlement_price', header: 'Settlement Price', align: 'right' },
  {
    key: 'settled_at',
    header: 'Settled At',
    render: (row) => new Date((row.settled_at as number) * 1000).toLocaleString(),
  },
  {
    key: 'pnl',
    header: 'PnL',
    align: 'right',
    render: (row) => {
      const val = parseFloat(String(row.pnl));
      const color = val >= 0 ? 'var(--buy)' : 'var(--sell)';
      return <span style={{ color }}>{String(row.pnl)}</span>;
    },
  },
];

export default function DeliveryPage() {
  const [tab, setTab] = useState('instruments');
  const [instruments, setInstruments] = useState<DeliveryInstrument[]>([]);
  const [positions, setPositions] = useState<DeliveryPosition[]>([]);
  const [settlements, setSettlements] = useState<SettlementRecord[]>([]);
  const [instrLoading, setInstrLoading] = useState(false);
  const [posLoading, setPosLoading] = useState(false);
  const [settlLoading, setSettlLoading] = useState(false);

  useEffect(() => {
    setInstrLoading(true);
    getDeliveryInstruments()
      .then(setInstruments)
      .catch(() => setInstruments([]))
      .finally(() => setInstrLoading(false));
  }, []);

  useEffect(() => {
    if (tab !== 'positions') return;
    setPosLoading(true);
    getDeliveryPositions()
      .then(setPositions)
      .catch(() => setPositions([]))
      .finally(() => setPosLoading(false));
  }, [tab]);

  useEffect(() => {
    if (tab !== 'settlements') return;
    setSettlLoading(true);
    getSettlementPrices(50)
      .then(setSettlements)
      .catch(() => setSettlements([]))
      .finally(() => setSettlLoading(false));
  }, [tab]);

  const settlementRows: SettlementRow[] = settlements.map((s) => ({ ...s }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>Delivery Futures</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.content}>
        {tab === 'instruments' && (
          <div className={styles.instrLayout}>
            <InstrumentList
              instruments={instruments}
              loading={instrLoading}
              onSelect={() => {}}
            />
            <DeliveryOrderEntry instruments={instruments} />
          </div>
        )}
        {tab === 'positions' && (
          <DeliveryPositions positions={positions} loading={posLoading} />
        )}
        {tab === 'settlements' && (
          <Table<SettlementRow>
            columns={SETTLEMENT_COLUMNS}
            data={settlementRows}
            loading={settlLoading}
            emptyText="No settlement history"
          />
        )}
      </div>
    </div>
  );
}
