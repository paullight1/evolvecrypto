import { describe, it, expect, beforeEach } from 'vitest';
import { useOrderBookStore } from './orderbook';

function resetStore(): void {
  useOrderBookStore.getState().reset();
}

describe('useOrderBookStore — initial state', () => {
  beforeEach(resetStore);

  it('bids is empty array', () => {
    expect(useOrderBookStore.getState().bids).toEqual([]);
  });

  it('asks is empty array', () => {
    expect(useOrderBookStore.getState().asks).toEqual([]);
  });

  it('spread is em dash for empty book', () => {
    expect(useOrderBookStore.getState().spread).toBe('—');
  });

  it('spreadPct is em dash for empty book', () => {
    expect(useOrderBookStore.getState().spreadPct).toBe('—');
  });

  it('midPrice is empty string for empty book', () => {
    expect(useOrderBookStore.getState().midPrice).toBe('');
  });

  it('lastSeq is 0', () => {
    expect(useOrderBookStore.getState().lastSeq).toBe(0);
  });
});

describe('useOrderBookStore — applySnapshot', () => {
  beforeEach(resetStore);

  it('sets bids from snapshot', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1.5'], ['49900', '2.0']],
      [['50100', '0.5']],
      1,
    );
    const { bids } = useOrderBookStore.getState();
    expect(bids).toHaveLength(2);
    expect(bids[0].price).toBe('50000');
    expect(bids[0].qty).toBe('1.5');
  });

  it('sets asks from snapshot', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1.0']],
      [['50100', '0.5'], ['50200', '1.0']],
      1,
    );
    const { asks } = useOrderBookStore.getState();
    expect(asks).toHaveLength(2);
    expect(asks[0].price).toBe('50100');
  });

  it('bids are sorted descending by price', () => {
    useOrderBookStore.getState().applySnapshot(
      [['49900', '1'], ['50000', '2'], ['49800', '0.5']],
      [['50100', '1']],
      1,
    );
    const prices = useOrderBookStore.getState().bids.map((b) => parseFloat(b.price));
    expect(prices[0]).toBeGreaterThan(prices[1]);
    expect(prices[1]).toBeGreaterThan(prices[2]);
  });

  it('asks are sorted ascending by price', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1']],
      [['50300', '0.5'], ['50100', '1'], ['50200', '0.8']],
      1,
    );
    const prices = useOrderBookStore.getState().asks.map((a) => parseFloat(a.price));
    expect(prices[0]).toBeLessThan(prices[1]);
    expect(prices[1]).toBeLessThan(prices[2]);
  });

  it('computes spread as bestAsk - bestBid', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1']],
      [['50100', '1']],
      1,
    );
    expect(useOrderBookStore.getState().spread).toBe('100.00');
  });

  it('computes midPrice as (bestBid + bestAsk) / 2', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1']],
      [['50100', '1']],
      1,
    );
    expect(useOrderBookStore.getState().midPrice).toBe('50050.00');
  });

  it('updates lastSeq', () => {
    useOrderBookStore.getState().applySnapshot([], [], 42);
    expect(useOrderBookStore.getState().lastSeq).toBe(42);
  });

  it('filters out zero-qty entries in snapshot', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '0'], ['49900', '1']],
      [['50100', '1']],
      1,
    );
    const { bids } = useOrderBookStore.getState();
    expect(bids).toHaveLength(1);
    expect(bids[0].price).toBe('49900');
  });

  it('each bid level has a depthPct between 0 and 100', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1'], ['49900', '2'], ['49800', '3']],
      [['50100', '1']],
      1,
    );
    for (const b of useOrderBookStore.getState().bids) {
      expect(b.depthPct).toBeGreaterThan(0);
      expect(b.depthPct).toBeLessThanOrEqual(100);
    }
  });

  it('last bid level always has depthPct = 100', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1'], ['49900', '2']],
      [['50100', '1']],
      1,
    );
    const bids = useOrderBookStore.getState().bids;
    expect(bids[bids.length - 1].depthPct).toBe(100);
  });
});

describe('useOrderBookStore — applyDelta', () => {
  beforeEach(() => {
    resetStore();
    // Seed a base snapshot
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1.0'], ['49900', '2.0']],
      [['50100', '0.5'], ['50200', '1.0']],
      10,
    );
  });

  it('adds a new bid level', () => {
    useOrderBookStore.getState().applyDelta([['49800', '0.3']], [], 11);
    const prices = useOrderBookStore.getState().bids.map((b) => b.price);
    expect(prices).toContain('49800');
  });

  it('updates an existing ask level with new qty', () => {
    useOrderBookStore.getState().applyDelta([], [['50100', '9.9']], 11);
    const ask = useOrderBookStore.getState().asks.find((a) => a.price === '50100');
    expect(ask?.qty).toBe('9.9');
  });

  it('removes a bid level when qty=0', () => {
    useOrderBookStore.getState().applyDelta([['50000', '0']], [], 11);
    const prices = useOrderBookStore.getState().bids.map((b) => b.price);
    expect(prices).not.toContain('50000');
  });

  it('removes an ask level when qty=0', () => {
    useOrderBookStore.getState().applyDelta([], [['50100', '0']], 11);
    const prices = useOrderBookStore.getState().asks.map((a) => a.price);
    expect(prices).not.toContain('50100');
  });

  it('ignores delta with seq <= lastSeq (stale update)', () => {
    const bidsBefore = [...useOrderBookStore.getState().bids];
    useOrderBookStore.getState().applyDelta([['48000', '5']], [], 5);
    expect(useOrderBookStore.getState().bids).toEqual(bidsBefore);
  });

  it('ignores delta with seq equal to lastSeq', () => {
    const bidsBefore = [...useOrderBookStore.getState().bids];
    useOrderBookStore.getState().applyDelta([['48000', '5']], [], 10);
    expect(useOrderBookStore.getState().bids).toEqual(bidsBefore);
  });

  it('updates lastSeq after a valid delta', () => {
    useOrderBookStore.getState().applyDelta([], [], 15);
    expect(useOrderBookStore.getState().lastSeq).toBe(15);
  });

  it('recomputes spread after delta changes best prices', () => {
    // Remove current best ask at 50100, making 50200 the new best ask
    useOrderBookStore.getState().applyDelta([], [['50100', '0']], 11);
    // New spread: 50200 - 50000 = 200
    expect(useOrderBookStore.getState().spread).toBe('200.00');
  });
});

describe('useOrderBookStore — reset', () => {
  it('clears all book state', () => {
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1']],
      [['50100', '1']],
      5,
    );
    useOrderBookStore.getState().reset();
    const state = useOrderBookStore.getState();
    expect(state.bids).toEqual([]);
    expect(state.asks).toEqual([]);
    expect(state.spread).toBe('—');
    expect(state.midPrice).toBe('');
    expect(state.lastSeq).toBe(0);
  });
});

// === Precision Display Setting ===

describe('useOrderBookStore — precision setting', () => {
  beforeEach(resetStore);

  it('setPrecision updates the precision field', () => {
    useOrderBookStore.getState().setPrecision(1);
    expect(useOrderBookStore.getState().precision).toBe(1);
  });

  it('precision setting does not mutate the stored price levels', () => {
    // The store stores raw price strings from the exchange.
    // Aggregation is done at render time using the precision setting — not in the store.
    // Changing precision just updates the display config.
    useOrderBookStore.getState().applySnapshot(
      [['65000.10', '1'], ['65000.20', '2'], ['65000.30', '3']],
      [['65000.40', '1']],
      1,
    );
    useOrderBookStore.getState().setPrecision(1);

    // Levels are NOT aggregated in the store — all 3 bid levels remain distinct
    const { bids } = useOrderBookStore.getState();
    expect(bids.length).toBe(3);
    expect(bids.map((b) => b.price)).toContain('65000.30');
    expect(bids.map((b) => b.price)).toContain('65000.20');
    expect(bids.map((b) => b.price)).toContain('65000.10');
  });
});

// === Large Book Performance ===

describe('useOrderBookStore — large book', () => {
  beforeEach(resetStore);

  it('handles 1000 bid and 1000 ask levels without error', () => {
    const bids: [string, string][] = [];
    const asks: [string, string][] = [];
    for (let i = 0; i < 1000; i++) {
      bids.push([(50000 - i).toString(), '1']);
      asks.push([(50001 + i).toString(), '1']);
    }

    useOrderBookStore.getState().applySnapshot(bids, asks, 1);

    const state = useOrderBookStore.getState();
    // Store slices to DEPTH=20, so max 20 levels per side
    expect(state.bids.length).toBeLessThanOrEqual(20);
    expect(state.asks.length).toBeLessThanOrEqual(20);
    // Best bid should be 50000, best ask 50001
    expect(state.bids[0].price).toBe('50000');
    expect(state.asks[0].price).toBe('50001');
    // Spread should be correct
    expect(state.spread).toBe('1.00');
  });

  it('correctly sorts 1000 bids in descending price order', () => {
    const bids: [string, string][] = [];
    for (let i = 0; i < 1000; i++) {
      // Insert in random order: ascending
      bids.push([(40000 + i).toString(), '1']);
    }
    useOrderBookStore.getState().applySnapshot(bids, [['50000', '1']], 1);

    const { bids: storedBids } = useOrderBookStore.getState();
    // Top of book (index 0) should be the highest price: 40999
    expect(parseFloat(storedBids[0].price)).toBe(40999);
    // Prices should be in descending order
    for (let i = 0; i < storedBids.length - 1; i++) {
      expect(parseFloat(storedBids[i].price)).toBeGreaterThan(parseFloat(storedBids[i + 1].price));
    }
  });
});

// === Delta Edge Cases ===

describe('useOrderBookStore — delta edge cases', () => {
  beforeEach(() => {
    resetStore();
    useOrderBookStore.getState().applySnapshot(
      [['50000', '1.0'], ['49900', '2.0']],
      [['50100', '0.5'], ['50200', '1.0']],
      10,
    );
  });

  it('delta with qty=0 removes the level completely from bids', () => {
    const bidsBefore = useOrderBookStore.getState().bids.length;
    useOrderBookStore.getState().applyDelta([['50000', '0']], [], 11);

    const { bids } = useOrderBookStore.getState();
    expect(bids.length).toBe(bidsBefore - 1);
    expect(bids.map((b) => b.price)).not.toContain('50000');
  });

  it('delta with qty=0 removes the level completely from asks', () => {
    const asksBefore = useOrderBookStore.getState().asks.length;
    useOrderBookStore.getState().applyDelta([], [['50100', '0']], 11);

    const { asks } = useOrderBookStore.getState();
    expect(asks.length).toBe(asksBefore - 1);
    expect(asks.map((a) => a.price)).not.toContain('50100');
  });

  it('delta for a non-existent price level adds it to bids', () => {
    const bidsBefore = useOrderBookStore.getState().bids.length;
    // 49500 does not exist in snapshot
    useOrderBookStore.getState().applyDelta([['49500', '3.0']], [], 11);

    const { bids } = useOrderBookStore.getState();
    expect(bids.length).toBe(bidsBefore + 1);
    expect(bids.map((b) => b.price)).toContain('49500');
  });

  it('delta for a non-existent price level adds it to asks', () => {
    const asksBefore = useOrderBookStore.getState().asks.length;
    // 50300 does not exist in snapshot
    useOrderBookStore.getState().applyDelta([], [['50300', '2.0']], 11);

    const { asks } = useOrderBookStore.getState();
    expect(asks.length).toBe(asksBefore + 1);
    expect(asks.map((a) => a.price)).toContain('50300');
  });

  it('applying delta to an empty book (after reset) adds the new level', () => {
    resetStore();
    // After reset, lastSeq = 0, so seq=1 is valid
    useOrderBookStore.getState().applyDelta([['49000', '5']], [], 1);

    const { bids } = useOrderBookStore.getState();
    expect(bids.length).toBe(1);
    expect(bids[0].price).toBe('49000');
    expect(bids[0].qty).toBe('5');
  });

  it('multiple zero-qty deltas in one batch remove all specified levels', () => {
    useOrderBookStore.getState().applyDelta(
      [['50000', '0'], ['49900', '0']],
      [],
      11,
    );

    const { bids } = useOrderBookStore.getState();
    expect(bids.length).toBe(0);
  });
});
