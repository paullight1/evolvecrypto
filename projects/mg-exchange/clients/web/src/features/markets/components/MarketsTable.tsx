import { useState, useMemo } from 'react';
import { Table, type ColumnDef } from '@shared/ui/Table';
import { Input } from '@shared/ui/Input';
import { Badge } from '@shared/ui/Badge';
import type { Market } from '../types';
import styles from '../MarketsPage.module.css';

type SortKey = 'symbol' | 'last_price' | 'price_change_pct_24h' | 'volume_24h';
type SortDir = 'asc' | 'desc';

interface MarketRow extends Record<string, unknown> {
  symbol: string;
  last_price: string;
  price_change_pct_24h: string;
  volume_24h: string;
  high_24h: string;
  low_24h: string;
}

interface Props {
  markets: Market[];
  loading: boolean;
  category: string;
}

export function MarketsTable({ markets, loading, category }: Props) {
  const [search, setSearch] = useState('');
  const [sortKey, setSortKey] = useState<SortKey>('volume_24h');
  const [sortDir, setSortDir] = useState<SortDir>('desc');

  function toggleSort(key: SortKey) {
    if (sortKey === key) {
      setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortKey(key);
      setSortDir('desc');
    }
  }

  const filtered = useMemo(() => {
    let list = category === 'all' ? markets : markets.filter((m) => m.market_type === category);
    if (search.trim()) {
      const q = search.trim().toUpperCase();
      list = list.filter((m) => m.symbol.includes(q));
    }
    return [...list].sort((a, b) => {
      const av = parseFloat(a[sortKey]);
      const bv = parseFloat(b[sortKey]);
      return sortDir === 'asc' ? av - bv : bv - av;
    });
  }, [markets, category, search, sortKey, sortDir]);

  const COLUMNS: ColumnDef<MarketRow>[] = [
    { key: 'symbol', header: 'Symbol', width: '150px' },
    { key: 'last_price', header: 'Price', align: 'right' },
    {
      key: 'price_change_pct_24h',
      header: '24h %',
      align: 'right',
      render: (r) => {
        const pct = parseFloat(String(r.price_change_pct_24h));
        return (
          <Badge variant={pct >= 0 ? 'success' : 'danger'} size="sm">
            {pct >= 0 ? '+' : ''}{pct.toFixed(2)}%
          </Badge>
        );
      },
    },
    { key: 'volume_24h', header: '24h Volume', align: 'right' },
    { key: 'high_24h', header: '24h High', align: 'right' },
    { key: 'low_24h', header: '24h Low', align: 'right' },
  ];

  const rows: MarketRow[] = filtered.map((m) => ({
    symbol: m.symbol,
    last_price: m.last_price,
    price_change_pct_24h: m.price_change_pct_24h,
    volume_24h: m.volume_24h,
    high_24h: m.high_24h,
    low_24h: m.low_24h,
  }));

  return (
    <div className={styles.tableWrapper}>
      <div className={styles.tableToolbar}>
        <Input
          placeholder="Search symbol..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <div className={styles.sortButtons}>
          {(['symbol', 'last_price', 'price_change_pct_24h', 'volume_24h'] as SortKey[]).map((k) => (
            <button
              key={k}
              type="button"
              className={`${styles.sortBtn} ${sortKey === k ? styles.sortBtnActive : ''}`}
              onClick={() => toggleSort(k)}
            >
              {k === 'price_change_pct_24h' ? '24h %' : k.replace('_', ' ')}
              {sortKey === k ? (sortDir === 'asc' ? ' ↑' : ' ↓') : ''}
            </button>
          ))}
        </div>
      </div>
      <Table<MarketRow> columns={COLUMNS} data={rows} loading={loading} emptyText="No markets found" />
    </div>
  );
}
