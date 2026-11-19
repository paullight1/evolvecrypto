import { useMemo } from 'react';
import { useOrderBookStore } from '../stores/orderbook';
import type { BookLevel } from '../stores/orderbook';
import { useTradingStore } from '../stores/trading';
import { useOrderBook } from '../hooks/useOrderBook';
import styles from './OrderBook.module.css';
import { cn } from '@shared/utils/format';
import { Skeleton } from '@shared/ui/Skeleton';

interface OrderBookProps {
  symbol: string;
}

const PRECISION_OPTIONS = [0.01, 0.1, 1, 10, 50, 100];

/** Pick a sensible default precision based on the current price level. */
function autoPrecision(midPrice: string | undefined): number {
  const p = parseFloat(midPrice ?? '0');
  if (p >= 10_000) return 10;
  if (p >= 1_000) return 1;
  if (p >= 10) return 0.1;
  return 0.01;
}

function formatQty(qty: string): string {
  const n = parseFloat(qty);
  if (!isFinite(n)) return qty;
  if (n >= 1000) return n.toFixed(2);
  if (n >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

function aggregateLevels(levels: BookLevel[], precision: number, side: 'bid' | 'ask'): BookLevel[] {
  if (precision <= 0) return levels;
  const decimalPlaces = precision < 1 ? (String(precision).split('.')[1]?.length ?? 0) : 0;
  const map = new Map<string, number>();
  for (const level of levels) {
    const price = parseFloat(level.price);
    const bucketPrice = Math.floor(price / precision) * precision;
    const bucket = bucketPrice.toFixed(decimalPlaces);
    map.set(bucket, (map.get(bucket) ?? 0) + parseFloat(level.qty));
  }
  const entries = [...map.entries()].filter(([, qty]) => qty > 0);
  if (side === 'bid') entries.sort((a, b) => parseFloat(b[0]) - parseFloat(a[0]));
  else entries.sort((a, b) => parseFloat(a[0]) - parseFloat(b[0]));
  let cumQty = 0;
  const withCum = entries.map(([price, qty]) => {
    cumQty += qty;
    return { price, qty, cumQty };
  });
  const maxCum = withCum[withCum.length - 1]?.cumQty ?? 1;
  return withCum.map(({ price, qty, cumQty: cum }) => {
    const n = parseFloat(price) * qty;
    let total = n.toFixed(2);
    if (n >= 1_000_000) total = `${(n / 1_000_000).toFixed(2)}M`;
    else if (n >= 1_000) total = `${(n / 1_000).toFixed(1)}K`;
    return { price, qty: qty.toString(), total, depthPct: (cum / maxCum) * 100 };
  });
}

function SkeletonRows({ count }: { count: number }) {
  return (
    <>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className={styles.row}>
          <Skeleton height={12} width="60%" />
          <Skeleton height={12} width="45%" />
          <Skeleton height={12} width="50%" />
        </div>
      ))}
    </>
  );
}

export function OrderBook({ symbol }: OrderBookProps) {
  useOrderBook(symbol);

  const {
    bids, asks, spread, spreadPct, midPrice,
    precision, setPrecision, displayMode, setDisplayMode,
  } = useOrderBookStore();
  const { fillFromOrderbook, fillQtyFromOrderbook } = useTradingStore();

  const isLoading = bids.length === 0 && asks.length === 0;
  const aggAsks = useMemo(() => aggregateLevels(asks, precision, 'ask'), [asks, precision]);
  const aggBids = useMemo(() => aggregateLevels(bids, precision, 'bid'), [bids, precision]);

  return (
    <div className={styles.panel}>
      <div className={styles.toolbar}>
        <span className={styles.title}>Order Book</span>
        <div className={styles.controls}>
          <select
            className={styles.precision}
            value={precision}
            onChange={(e) => setPrecision(parseFloat(e.target.value))}
          >
            {PRECISION_OPTIONS.map((p) => (
              <option key={p} value={p}>{p}</option>
            ))}
          </select>
          {(['both', 'bids', 'asks'] as const).map((m) => (
            <button
              key={m}
              type="button"
              className={cn(styles.modeBtn, displayMode === m && styles.modeBtnActive)}
              onClick={() => setDisplayMode(m)}
            >
              {m === 'both' ? '⇅' : m === 'bids' ? '↑' : '↓'}
            </button>
          ))}
        </div>
      </div>

      <div className={styles.labels}>
        <span>Price</span>
        <span>Size</span>
        <span>Total</span>
      </div>

      {/* Asks */}
      {displayMode !== 'bids' && (
        <div className={cn(styles.side, styles.sideAsks, displayMode === 'asks' && styles.sideExpanded)}>
          {isLoading
            ? <SkeletonRows count={10} />
            : aggAsks.map((level) => (
              <div
                key={level.price}
                className={styles.row}
                onClick={() => fillFromOrderbook(level.price)}
                title={`Click to fill price: ${level.price}`}
              >
                <div className={cn(styles.depthBar, styles.depthBarAsk)} style={{ width: `${level.depthPct}%` }} />
                <span className={cn(styles.price, styles.priceAsk)}>{level.price}</span>
                <span
                  className={styles.qty}
                  onClick={(e) => { e.stopPropagation(); fillQtyFromOrderbook(level.qty); }}
                >
                  {formatQty(level.qty)}
                </span>
                <span className={styles.total}>{level.total}</span>
              </div>
            ))}
        </div>
      )}

      {/* Spread */}
      {displayMode === 'both' && (
        <div className={styles.spread}>
          <span>Spread</span>
          <span>
            <span className={styles.spreadVal}>{spread}</span>
            {' '}
            <span className={styles.spreadPct}>{spreadPct}</span>
          </span>
          {midPrice && <span className={styles.midPrice}>{midPrice}</span>}
        </div>
      )}

      {/* Bids */}
      {displayMode !== 'asks' && (
        <div className={cn(styles.side, displayMode === 'bids' && styles.sideExpanded)}>
          {isLoading
            ? <SkeletonRows count={10} />
            : aggBids.map((level) => (
              <div
                key={level.price}
                className={styles.row}
                onClick={() => fillFromOrderbook(level.price)}
                title={`Click to fill price: ${level.price}`}
              >
                <div className={cn(styles.depthBar, styles.depthBarBid)} style={{ width: `${level.depthPct}%` }} />
                <span className={cn(styles.price, styles.priceBid)}>{level.price}</span>
                <span
                  className={styles.qty}
                  onClick={(e) => { e.stopPropagation(); fillQtyFromOrderbook(level.qty); }}
                >
                  {formatQty(level.qty)}
                </span>
                <span className={styles.total}>{level.total}</span>
              </div>
            ))}
        </div>
      )}
    </div>
  );
}
