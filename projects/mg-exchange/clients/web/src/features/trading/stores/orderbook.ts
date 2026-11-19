import { create } from 'zustand';

export interface BookLevel {
  price: string;
  qty: string;
  total: string;
  depthPct: number; // 0–100, cumulative depth bar %
}

export interface OrderBookState {
  bids: BookLevel[];
  asks: BookLevel[];
  spread: string;
  spreadPct: string;
  midPrice: string;
  lastSeq: number;

  // Internal maps for fast updates (mutated in place for perf)
  _bidsMap: Map<string, string>;
  _asksMap: Map<string, string>;

  // Display settings
  precision: number;
  displayMode: 'both' | 'bids' | 'asks';

  setPrecision: (p: number) => void;
  setDisplayMode: (m: 'both' | 'bids' | 'asks') => void;
  applySnapshot: (bids: [string, string][], asks: [string, string][], seq: number) => void;
  applyDelta: (bids: [string, string][], asks: [string, string][], seq: number) => void;
  reset: () => void;
}

const DEPTH = 20;

function buildLevels(map: Map<string, string>, side: 'bid' | 'ask'): BookLevel[] {
  const entries = [...map.entries()].filter(([, q]) => parseFloat(q) > 0);

  if (side === 'bid') {
    entries.sort((a, b) => parseFloat(b[0]) - parseFloat(a[0]));
  } else {
    entries.sort((a, b) => parseFloat(a[0]) - parseFloat(b[0]));
  }

  const top = entries.slice(0, DEPTH);

  let cumQty = 0;
  const withCum = top.map(([price, qty]) => {
    cumQty += parseFloat(qty);
    return { price, qty, cumQty };
  });

  const maxCum = withCum[withCum.length - 1]?.cumQty ?? 1;

  return withCum.map(({ price, qty, cumQty: cum }) => ({
    price,
    qty,
    total: formatTotal(price, qty),
    depthPct: (cum / maxCum) * 100,
  }));
}

function formatTotal(price: string, qty: string): string {
  const n = parseFloat(price) * parseFloat(qty);
  if (!isFinite(n)) return '0';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toFixed(2);
}

function computeSpread(
  bids: BookLevel[],
  asks: BookLevel[],
): { spread: string; spreadPct: string; midPrice: string } {
  if (bids.length === 0 || asks.length === 0) {
    return { spread: '—', spreadPct: '—', midPrice: '' };
  }
  const bestBid = parseFloat(bids[0].price);
  const bestAsk = parseFloat(asks[0].price);
  const spread = bestAsk - bestBid;
  const mid = (bestBid + bestAsk) / 2;
  return {
    spread: spread.toFixed(2),
    spreadPct: ((spread / bestBid) * 100).toFixed(4) + '%',
    midPrice: mid.toFixed(2),
  };
}

export const useOrderBookStore = create<OrderBookState>((set, get) => ({
  bids: [],
  asks: [],
  spread: '—',
  spreadPct: '—',
  midPrice: '',
  lastSeq: 0,
  _bidsMap: new Map(),
  _asksMap: new Map(),
  precision: 0.01,
  _precisionAutoSet: false,
  displayMode: 'both',

  setPrecision: (p) => set({ precision: p }),
  setDisplayMode: (m) => set({ displayMode: m }),

  applySnapshot: (rawBids, rawAsks, seq) => {
    const bidsMap = new Map(rawBids.map(([p, q]) => [p, q]));
    const asksMap = new Map(rawAsks.map(([p, q]) => [p, q]));
    const bids = buildLevels(bidsMap, 'bid');
    const asks = buildLevels(asksMap, 'ask');
    const { spread, spreadPct, midPrice } = computeSpread(bids, asks);

    // Auto-set precision on first snapshot based on price level
    const state = get();
    let precision = state.precision;
    if (!state._precisionAutoSet && bids.length > 0) {
      const p = parseFloat(bids[0].price);
      if (p >= 10_000) precision = 10;
      else if (p >= 1_000) precision = 1;
      else if (p >= 10) precision = 0.1;
      else precision = 0.01;
    }

    set({ bids, asks, spread, spreadPct, midPrice, lastSeq: seq, _bidsMap: bidsMap, _asksMap: asksMap, precision, _precisionAutoSet: true });
  },

  applyDelta: (rawBids, rawAsks, seq) => {
    const state = get();
    if (seq <= state.lastSeq) return;

    const bidsMap = state._bidsMap;
    const asksMap = state._asksMap;

    for (const [price, qty] of rawBids) {
      if (parseFloat(qty) === 0) bidsMap.delete(price);
      else bidsMap.set(price, qty);
    }
    for (const [price, qty] of rawAsks) {
      if (parseFloat(qty) === 0) asksMap.delete(price);
      else asksMap.set(price, qty);
    }

    const bids = buildLevels(bidsMap, 'bid');
    const asks = buildLevels(asksMap, 'ask');
    const { spread, spreadPct, midPrice } = computeSpread(bids, asks);
    set({ bids, asks, spread, spreadPct, midPrice, lastSeq: seq });
  },

  reset: () =>
    set({
      bids: [],
      asks: [],
      spread: '—',
      spreadPct: '—',
      midPrice: '',
      lastSeq: 0,
      _bidsMap: new Map(),
      _asksMap: new Map(),
    }),
}));
