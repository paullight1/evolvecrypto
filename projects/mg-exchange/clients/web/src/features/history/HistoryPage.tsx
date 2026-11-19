import { useState, useEffect } from 'react';
import { Tabs, type TabDef } from '@shared/ui/Tabs';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import { Input } from '@shared/ui/Input';
import { Button } from '@shared/ui/Button';
import { getOrderHistory, getTradeHistory, getFundingHistory } from './api';
import type { TradeRecord, FundingRecord } from './types';
import type { OpenOrder } from '@features/trading/types';
import styles from './HistoryPage.module.css';

const TABS: TabDef[] = [
  { key: 'orders', label: 'Orders' },
  { key: 'trades', label: 'Trades' },
  { key: 'funding', label: 'Funding' },
];

interface OrderRow extends Record<string, unknown> {
  order_id: string; symbol: string; side: string; qty: string; status: string; created_at: number;
}
interface TradeRow extends Record<string, unknown> {
  trade_id: string; symbol: string; side: string; price: string; qty: string; fee: string; created_at: number;
}
interface FundingRow extends Record<string, unknown> {
  id: string; symbol: string; funding_rate: string; amount: string; created_at: number;
}

const ORDER_COLS: ColumnDef<OrderRow>[] = [
  { key: 'symbol', header: 'Symbol' },
  { key: 'side', header: 'Side', render: (r) => <Badge variant={r.side === 'buy' ? 'success' : 'danger'} size="sm">{String(r.side)}</Badge> },
  { key: 'qty', header: 'Qty', align: 'right' },
  { key: 'status', header: 'Status' },
  { key: 'created_at', header: 'Date', render: (r) => new Date(r.created_at as number).toLocaleDateString() },
];
const TRADE_COLS: ColumnDef<TradeRow>[] = [
  { key: 'symbol', header: 'Symbol' },
  { key: 'side', header: 'Side', render: (r) => <Badge variant={r.side === 'buy' ? 'success' : 'danger'} size="sm">{String(r.side)}</Badge> },
  { key: 'price', header: 'Price', align: 'right' },
  { key: 'qty', header: 'Qty', align: 'right' },
  { key: 'fee', header: 'Fee', align: 'right' },
  { key: 'created_at', header: 'Date', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
];
const FUNDING_COLS: ColumnDef<FundingRow>[] = [
  { key: 'symbol', header: 'Symbol' },
  { key: 'funding_rate', header: 'Rate', align: 'right' },
  { key: 'amount', header: 'Amount', align: 'right' },
  { key: 'created_at', header: 'Date', render: (r) => new Date((r.created_at as number) * 1000).toLocaleDateString() },
];

function exportCsv(data: Record<string, unknown>[], filename: string) {
  if (!data.length) return;
  const keys = Object.keys(data[0]);
  const csv = [keys.join(','), ...data.map((r) => keys.map((k) => String(r[k] ?? '')).join(','))].join('\n');
  const blob = new Blob([csv], { type: 'text/csv' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

export default function HistoryPage() {
  const [tab, setTab] = useState('orders');
  const [orders, setOrders] = useState<OpenOrder[]>([]);
  const [trades, setTrades] = useState<TradeRecord[]>([]);
  const [funding, setFunding] = useState<FundingRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [dateFrom, setDateFrom] = useState('');
  const [dateTo, setDateTo] = useState('');

  function getTimestamps() {
    const from = dateFrom ? Math.floor(new Date(dateFrom).getTime() / 1000) : undefined;
    const to = dateTo ? Math.floor(new Date(dateTo).getTime() / 1000) : undefined;
    return { from, to };
  }

  useEffect(() => {
    setLoading(true);
    const { from, to } = getTimestamps();
    if (tab === 'orders') {
      getOrderHistory(from, to).then(setOrders).catch(() => setOrders([])).finally(() => setLoading(false));
    } else if (tab === 'trades') {
      getTradeHistory(from, to).then(setTrades).catch(() => setTrades([])).finally(() => setLoading(false));
    } else {
      getFundingHistory().then(setFunding).catch(() => setFunding([])).finally(() => setLoading(false));
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab]);

  const orderRows: OrderRow[] = orders.map((o) => ({ order_id: o.order_id, symbol: o.symbol, side: o.side, qty: o.qty, status: o.status, created_at: o.created_at }));
  const tradeRows: TradeRow[] = trades.map((t) => ({ trade_id: t.trade_id, symbol: t.symbol, side: t.side, price: t.price, qty: t.qty, fee: t.fee, created_at: t.created_at }));
  const fundingRows: FundingRow[] = funding.map((f) => ({ id: f.id, symbol: f.symbol, funding_rate: f.funding_rate, amount: f.amount, created_at: f.created_at }));

  return (
    <div className={styles.page}>
      <h1 className={styles.title}>History</h1>
      <Tabs tabs={TABS} activeKey={tab} onChange={setTab} className={styles.tabs} />

      <div className={styles.toolbar}>
        <Input label="From" type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} />
        <Input label="To" type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} />
        <Button variant="secondary" size="sm" onClick={() => {
          setLoading(true);
          const { from, to } = getTimestamps();
          if (tab === 'orders') getOrderHistory(from, to).then(setOrders).catch(() => setOrders([])).finally(() => setLoading(false));
          else if (tab === 'trades') getTradeHistory(from, to).then(setTrades).catch(() => setTrades([])).finally(() => setLoading(false));
        }}>Filter</Button>
        <Button variant="ghost" size="sm" className={styles.exportBtn} onClick={() => {
          if (tab === 'orders') exportCsv(orderRows, 'orders.csv');
          else if (tab === 'trades') exportCsv(tradeRows, 'trades.csv');
          else exportCsv(fundingRows, 'funding.csv');
        }}>Export CSV</Button>
      </div>

      {tab === 'orders' && <Table<OrderRow> columns={ORDER_COLS} data={orderRows} loading={loading} emptyText="No orders" />}
      {tab === 'trades' && <Table<TradeRow> columns={TRADE_COLS} data={tradeRows} loading={loading} emptyText="No trades" />}
      {tab === 'funding' && <Table<FundingRow> columns={FUNDING_COLS} data={fundingRows} loading={loading} emptyText="No funding records" />}
    </div>
  );
}
