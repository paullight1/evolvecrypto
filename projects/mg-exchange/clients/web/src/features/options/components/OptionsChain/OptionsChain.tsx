import { Table, type ColumnDef } from '@shared/ui/Table';
import { Badge } from '@shared/ui/Badge';
import type { OptionInstrument, OptionTicker } from '../../types';
import styles from './OptionsChain.module.css';

interface ChainRow extends Record<string, unknown> {
  strike: string;
  call_bid: string;
  call_ask: string;
  call_last: string;
  call_iv: string;
  call_delta: string;
  call_oi: string;
  put_bid: string;
  put_ask: string;
  put_last: string;
  put_iv: string;
  put_delta: string;
  put_oi: string;
  call_symbol: string;
  put_symbol: string;
}

interface OptionsChainProps {
  instruments: OptionInstrument[];
  tickers: OptionTicker[];
  loading: boolean;
  expiry: string;
  onSelectSymbol: (symbol: string, side: 'buy' | 'sell') => void;
}

function tickerMap(tickers: OptionTicker[]): Map<string, OptionTicker> {
  const m = new Map<string, OptionTicker>();
  for (const t of tickers) m.set(t.symbol, t);
  return m;
}

const CALL_COLUMNS: ColumnDef<ChainRow>[] = [
  { key: 'call_oi', header: 'OI', align: 'right', sortable: true },
  { key: 'call_delta', header: 'Delta', align: 'right' },
  { key: 'call_iv', header: 'IV%', align: 'right' },
  { key: 'call_last', header: 'Last', align: 'right' },
  { key: 'call_ask', header: 'Ask', align: 'right' },
  { key: 'call_bid', header: 'Bid', align: 'right' },
];

const STRIKE_COLUMN: ColumnDef<ChainRow> = {
  key: 'strike',
  header: 'Strike',
  align: 'center',
  render: (row) => (
    <Badge variant="neutral">{String(row.strike)}</Badge>
  ),
};

const PUT_COLUMNS: ColumnDef<ChainRow>[] = [
  { key: 'put_bid', header: 'Bid', align: 'right' },
  { key: 'put_ask', header: 'Ask', align: 'right' },
  { key: 'put_last', header: 'Last', align: 'right' },
  { key: 'put_iv', header: 'IV%', align: 'right' },
  { key: 'put_delta', header: 'Delta', align: 'right' },
  { key: 'put_oi', header: 'OI', align: 'right', sortable: true },
];

export function OptionsChain({ instruments, tickers, loading, expiry, onSelectSymbol }: OptionsChainProps) {
  const tMap = tickerMap(tickers);
  const byExpiry = instruments.filter((i) => i.expiry === expiry);

  const strikes = Array.from(new Set(byExpiry.map((i) => i.strike))).sort(
    (a, b) => parseFloat(a) - parseFloat(b),
  );

  const rows: ChainRow[] = strikes.map((strike) => {
    const call = byExpiry.find((i) => i.strike === strike && i.optionType === 'Call');
    const put = byExpiry.find((i) => i.strike === strike && i.optionType === 'Put');
    const ct = call ? tMap.get(call.symbol) : undefined;
    const pt = put ? tMap.get(put.symbol) : undefined;

    return {
      strike,
      call_bid: '—',
      call_ask: '—',
      call_last: ct?.markPrice ?? '—',
      call_iv: ct?.iv ?? '—',
      call_delta: '—',
      call_oi: ct?.openInterest ?? '—',
      put_bid: '—',
      put_ask: '—',
      put_last: pt?.markPrice ?? '—',
      put_iv: pt?.iv ?? '—',
      put_delta: '—',
      put_oi: pt?.openInterest ?? '—',
      call_symbol: call?.symbol ?? '',
      put_symbol: put?.symbol ?? '',
    };
  });

  const allColumns: ColumnDef<ChainRow>[] = [
    ...CALL_COLUMNS,
    STRIKE_COLUMN,
    ...PUT_COLUMNS,
  ];

  return (
    <div className={styles.wrapper}>
      <div className={styles.labels}>
        <span className={styles.callLabel}>Calls</span>
        <span className={styles.putLabel}>Puts</span>
      </div>
      <Table<ChainRow>
        columns={allColumns}
        data={rows}
        loading={loading}
        emptyText="No instruments for this expiry"
        onRowClick={(row) => {
          const sym = String(row.call_symbol) || String(row.put_symbol);
          if (sym) onSelectSymbol(sym, 'buy');
        }}
      />
    </div>
  );
}
