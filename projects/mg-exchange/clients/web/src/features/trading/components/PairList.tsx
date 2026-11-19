import { useState, useEffect, useRef, useMemo } from 'react';
import { useMarketStore } from '../stores/market';
import { getMarkets, type MarketInfo } from '../api';
import styles from './PairList.module.css';
import { cn } from '@shared/utils/format';

type SortKey = 'symbol' | 'price' | 'change' | 'volume';

function formatPairPrice(raw: string): string {
  if (!raw) return '—';
  const n = parseFloat(raw);
  if (!isFinite(n)) return '—';
  if (n >= 10000) return n.toLocaleString('en-US', { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  if (n >= 100) return n.toFixed(2);
  if (n >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

function formatVolume(raw: string): string {
  const n = parseFloat(raw);
  if (!isFinite(n) || n === 0) return '—';
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toFixed(0);
}

interface PairListProps {
  isOpen: boolean;
  onClose: () => void;
}

export function PairList({ isOpen, onClose }: PairListProps) {
  const { symbol, setSymbol, tradingMode } = useMarketStore();
  const [search, setSearch] = useState('');
  const [markets, setMarkets] = useState<MarketInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [sortKey, setSortKey] = useState<SortKey>('volume');
  const [sortDesc, setSortDesc] = useState(true);
  const searchRef = useRef<HTMLInputElement>(null);

  // Fetch all markets from backend
  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    setLoading(true);

    getMarkets()
      .then((data) => {
        if (!cancelled) setMarkets(data);
      })
      .catch(() => {
        if (!cancelled) setMarkets([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => { cancelled = true; };
  }, [isOpen]);

  useEffect(() => {
    if (isOpen) {
      setTimeout(() => searchRef.current?.focus(), 50);
    } else {
      setSearch('');
    }
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    function handler(e: KeyboardEvent): void {
      if (e.key === 'Escape') onClose();
    }
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [isOpen, onClose]);

  // Filter by trading mode and search
  const filtered = useMemo(() => {
    const modeFilter = tradingMode === 'spot' ? 'spot'
      : tradingMode === 'futures' ? 'perpetual'
      : 'coin_m';

    let list = markets.filter((m) => m.market_type === modeFilter);

    if (search) {
      const q = search.toLowerCase();
      list = list.filter((m) => m.symbol.toLowerCase().includes(q) || m.base_asset.toLowerCase().includes(q));
    }

    // Sort
    list.sort((a, b) => {
      let cmp = 0;
      switch (sortKey) {
        case 'symbol':
          cmp = a.symbol.localeCompare(b.symbol);
          break;
        case 'price':
          cmp = parseFloat(a.last_price || '0') - parseFloat(b.last_price || '0');
          break;
        case 'change':
          cmp = parseFloat(a.price_change_pct_24h || '0') - parseFloat(b.price_change_pct_24h || '0');
          break;
        case 'volume':
          cmp = parseFloat(a.volume_24h || '0') - parseFloat(b.volume_24h || '0');
          break;
      }
      return sortDesc ? -cmp : cmp;
    });

    return list;
  }, [markets, tradingMode, search, sortKey, sortDesc]);

  if (!isOpen) return null;

  function handleSelect(name: string): void {
    setSymbol(name);
    onClose();
  }

  function handleSort(key: SortKey): void {
    if (sortKey === key) {
      setSortDesc((d) => !d);
    } else {
      setSortKey(key);
      setSortDesc(key === 'volume' || key === 'change'); // default desc for volume/change
    }
  }

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.panel} onClick={(e) => e.stopPropagation()}>
        <div className={styles.search}>
          <input
            ref={searchRef}
            className={styles.searchInput}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search symbol..."
          />
        </div>

        <div className={styles.listHeader}>
          <span className={styles.sortable} onClick={() => handleSort('symbol')}>
            Pair {sortKey === 'symbol' ? (sortDesc ? '↓' : '↑') : ''}
          </span>
          <span className={styles.sortable} onClick={() => handleSort('price')}>
            Price {sortKey === 'price' ? (sortDesc ? '↓' : '↑') : ''}
          </span>
          <span className={styles.sortable} onClick={() => handleSort('change')}>
            24H % {sortKey === 'change' ? (sortDesc ? '↓' : '↑') : ''}
          </span>
          <span className={styles.sortable} onClick={() => handleSort('volume')}>
            Volume {sortKey === 'volume' ? (sortDesc ? '↓' : '↑') : ''}
          </span>
        </div>

        <div className={styles.list}>
          {loading ? (
            <div className={styles.empty}>Loading...</div>
          ) : filtered.length === 0 ? (
            <div className={styles.empty}>No pairs found</div>
          ) : (
            filtered.map((m) => {
              const pct = parseFloat(m.price_change_pct_24h ?? '');
              const isActive = symbol === m.symbol;

              return (
                <div
                  key={m.symbol}
                  className={cn(styles.row, isActive && styles.rowActive)}
                  onClick={() => handleSelect(m.symbol)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) => e.key === 'Enter' && handleSelect(m.symbol)}
                >
                  <span className={styles.pairName}>
                    {m.symbol}
                    <span className={cn(
                      styles.pairBadge,
                      m.market_type === 'perpetual' && styles.pairBadgePerp,
                      m.market_type === 'spot' && styles.pairBadgeSpot,
                      m.market_type === 'coin_m' && styles.pairBadgeCoinM,
                    )}>
                      {m.market_type === 'coin_m' ? 'COIN-M' : m.market_type === 'perpetual' ? 'PERP' : 'SPOT'}
                    </span>
                  </span>
                  <span className={styles.pairPrice}>
                    {m.last_price ? formatPairPrice(m.last_price) : '—'}
                  </span>
                  <span className={cn(
                    styles.pairChange,
                    isFinite(pct) && pct >= 0 ? styles.changePos : isFinite(pct) ? styles.changeNeg : '',
                  )}>
                    {isFinite(pct)
                      ? `${pct >= 0 ? '+' : ''}${pct.toFixed(2)}%`
                      : '—'}
                  </span>
                  <span className={styles.pairVolume}>
                    {formatVolume(m.volume_24h)}
                  </span>
                </div>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
