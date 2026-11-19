import { useState, useCallback, useEffect } from 'react';
import {
  getPositions, getOpenOrders, cancelOrder, cancelAllOrders,
  placeOrder, amendOrder, getOrderHistory, getBalances,
  getConditionalOrders, cancelConditionalOrder, cancelAllConditionalOrders,
  addMargin, setTradingStop,
  type ConditionalOrder,
} from '../api';
import type { Position, OpenOrder, BalanceResponse } from '../types';
import { useMarketStore } from '../stores/market';
import { ws } from '@shared/api/websocket';
import type { WsIncoming, WsPositionUpdate, WsOpenOrderUpdate } from '@shared/api/types';
import { Tabs } from '@shared/ui/Tabs';
import { cn } from '@shared/utils/format';
import { ApiError } from '@shared/api/client';
import { toast } from '@shared/ui/Toast';
import styles from './BottomPanel.module.css';

type TabKey = 'positions' | 'orders' | 'conditional' | 'history' | 'assets';

function formatPnl(pnl: string): string {
  const n = parseFloat(pnl);
  if (!isFinite(n)) return '—';
  return `${n > 0 ? '+' : ''}${n.toFixed(4)}`;
}

function formatTime(ts: string | number): string {
  try {
    return new Date(ts).toLocaleString('en-US', {
      month: 'short', day: '2-digit',
      hour: '2-digit', minute: '2-digit', hour12: false,
    });
  } catch {
    return String(ts);
  }
}

export function BottomPanel() {
  const { symbol, productType } = useMarketStore();
  const isPerp = productType === 'perpetual' || productType === 'coin_m';

  const [activeTab, setActiveTab] = useState<TabKey>('orders');
  const [positions, setPositions] = useState<Position[]>([]);
  const [orders, setOrders] = useState<OpenOrder[]>([]);
  const [history, setHistory] = useState<OpenOrder[]>([]);
  const [balances, setBalances] = useState<BalanceResponse[]>([]);
  const [posLoading, setPosLoading] = useState(true);
  const [ordLoading, setOrdLoading] = useState(true);
  const [histLoading, setHistLoading] = useState(false);
  const [balLoading, setBalLoading] = useState(false);
  const [cancellingOrders, setCancellingOrders] = useState<Set<string>>(new Set());
  const [closingPositions, setClosingPositions] = useState<Set<string>>(new Set());
  const [cancellingAll, setCancellingAll] = useState(false);
  const [conditionals, setConditionals] = useState<ConditionalOrder[]>([]);
  const [condLoading, setCondLoading] = useState(false);
  const [cancellingConds, setCancellingConds] = useState<Set<string>>(new Set());

  const fetchConditionals = useCallback(async (): Promise<void> => {
    setCondLoading(true);
    try {
      const data = await getConditionalOrders();
      setConditionals(data.filter((c) => c.status === 'Active'));
    } catch {
      setConditionals([]);
    } finally {
      setCondLoading(false);
    }
  }, []);

  const fetchPositions = useCallback(async (): Promise<void> => {
    setPosLoading(true);
    try {
      const data = await getPositions();
      setPositions(data.filter((p) => parseFloat(p.size) > 0));
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) setPositions([]);
    } finally {
      setPosLoading(false);
    }
  }, []);

  const fetchOrders = useCallback(async (): Promise<void> => {
    setOrdLoading(true);
    try {
      const data = await getOpenOrders(symbol);
      setOrders(data);
    } catch (err) {
      if (err instanceof ApiError && err.status === 404) setOrders([]);
    } finally {
      setOrdLoading(false);
    }
  }, [symbol]);

  useEffect(() => {
    void fetchPositions();
    void fetchOrders();
    void fetchConditionals();
  }, [fetchPositions, fetchOrders, fetchConditionals]);

  useEffect(() => {
    if (activeTab === 'history' && history.length === 0) {
      setHistLoading(true);
      getOrderHistory({ limit: HISTORY_PAGE_SIZE })
        .then((data) => setHistory(data))
        .catch(() => setHistory([]))
        .finally(() => setHistLoading(false));
    }
    if (activeTab === 'assets' && balances.length === 0) {
      setBalLoading(true);
      getBalances()
        .then((data) => setBalances(data))
        .catch(() => setBalances([]))
        .finally(() => setBalLoading(false));
    }
  }, [activeTab, history.length, balances.length]);

  // WS updates — server uses 'orders' (not 'orders.symbol') for all order events
  useEffect(() => {
    const topics = ['positions', 'orders'];
    ws.subscribe(topics);

    const removeHandler = ws.onMessage((msg: WsIncoming) => {
      if (msg.op !== 'data') return;
      if (msg.topic === 'positions') {
        const u = msg.data as WsPositionUpdate;
        setPositions((prev) => {
          const filtered = prev.filter((p) => p.symbol !== u.symbol);
          if (parseFloat(u.size) > 0) return [...filtered, u as unknown as Position];
          return filtered;
        });
      } else if (msg.topic === 'orders') {
        const u = msg.data as WsOpenOrderUpdate;
        // Only update orders for the currently viewed symbol
        if (u.symbol !== symbol) return;
        setOrders((prev) => {
          const filtered = prev.filter((o) => o.order_id !== u.order_id);
          if (u.status === 'open' || u.status === 'partial') {
            return [...filtered, u as unknown as OpenOrder];
          }
          return filtered;
        });
      }
    });

    return () => {
      ws.unsubscribe(topics);
      removeHandler();
    };
  }, [symbol]);

  async function handleCancelOrder(orderId: string): Promise<void> {
    setCancellingOrders((prev) => new Set([...prev, orderId]));
    try {
      await cancelOrder(orderId);
      setOrders((prev) => prev.filter((o) => o.order_id !== orderId));
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to cancel order'); } finally {
      setCancellingOrders((prev) => { const n = new Set(prev); n.delete(orderId); return n; });
    }
  }

  async function handleCancelAll(): Promise<void> {
    setCancellingAll(true);
    try {
      await cancelAllOrders(symbol);
      setOrders([]);
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to cancel orders'); } finally {
      setCancellingAll(false);
    }
  }

  async function handleCancelConditional(id: string): Promise<void> {
    setCancellingConds((prev) => new Set([...prev, id]));
    try {
      await cancelConditionalOrder(id);
      setConditionals((prev) => prev.filter((c) => c.id !== id));
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to cancel order'); } finally {
      setCancellingConds((prev) => { const n = new Set(prev); n.delete(id); return n; });
    }
  }

  async function handleCancelAllConditionals(): Promise<void> {
    try {
      await cancelAllConditionalOrders();
      setConditionals([]);
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to cancel orders'); }
  }

  async function handleAmendOrder(orderId: string, changes: { price?: string; qty?: string }): Promise<void> {
    try {
      await amendOrder(orderId, changes);
      // Refresh orders to show updated values
      void fetchOrders();
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to amend order'); }
  }

  async function handleAddMargin(symbol: string, amount: string): Promise<void> {
    try {
      await addMargin({ symbol, amount });
      void fetchPositions();
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to add margin'); }
  }

  async function handleSetTradingStop(symbol: string, tp?: string, sl?: string): Promise<void> {
    try {
      await setTradingStop({
        symbol,
        take_profit: tp && parseFloat(tp) > 0 ? tp : undefined,
        stop_loss: sl && parseFloat(sl) > 0 ? sl : undefined,
      });
      void fetchPositions();
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to set TP/SL'); }
  }

  async function handleClosePosition(pos: Position, pct = 100): Promise<void> {
    const key = `${pos.symbol}-${pos.side}`;
    setClosingPositions((prev) => new Set([...prev, key]));
    try {
      // Compute partial qty: floor to 6 decimal places to avoid dust
      const fullSize = parseFloat(pos.size);
      const closeQty = pct >= 100
        ? pos.size
        : (Math.floor(fullSize * (pct / 100) * 1e6) / 1e6).toFixed(6);

      await placeOrder({
        symbol: pos.symbol,
        side: pos.side.toLowerCase() === 'buy' ? 'sell' : 'buy',
        order_type: 'market',
        qty: closeQty,
        reduce_only: true,
      });
      if (pct >= 100) {
        setPositions((prev) => prev.filter((p) => !(p.symbol === pos.symbol && p.side === pos.side)));
      } else {
        // Refresh positions to get updated size
        void fetchPositions();
      }
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to close position'); } finally {
      setClosingPositions((prev) => { const n = new Set(prev); n.delete(key); return n; });
    }
  }

  const tabs = isPerp
    ? [
        { key: 'positions' as TabKey, label: 'Positions', badge: positions.length },
        { key: 'orders' as TabKey, label: 'Open Orders', badge: orders.length },
        { key: 'conditional' as TabKey, label: 'Conditional', badge: conditionals.length },
        { key: 'history' as TabKey, label: 'Order History' },
        { key: 'assets' as TabKey, label: 'Assets' },
      ]
    : [
        { key: 'orders' as TabKey, label: 'Open Orders', badge: orders.length },
        { key: 'conditional' as TabKey, label: 'Conditional', badge: conditionals.length },
        { key: 'history' as TabKey, label: 'Order History' },
        { key: 'assets' as TabKey, label: 'Balances' },
      ];

  return (
    <div className={styles.panel}>
      <div className={styles.tabsRow}>
        <Tabs
          tabs={tabs}
          activeKey={activeTab}
          onChange={(k) => setActiveTab(k as TabKey)}
        />
        {activeTab === 'orders' && orders.length > 0 && (
          <button
            type="button"
            className={styles.cancelAllBtn}
            disabled={cancellingAll}
            onClick={() => void handleCancelAll()}
          >
            {cancellingAll ? 'Cancelling…' : 'Cancel All'}
          </button>
        )}
        {activeTab === 'conditional' && conditionals.length > 0 && (
          <button
            type="button"
            className={styles.cancelAllBtn}
            onClick={() => void handleCancelAllConditionals()}
          >
            Cancel All
          </button>
        )}
      </div>

      <div className={styles.content}>
        {activeTab === 'positions' && (
          isPerp ? (
            <PositionsContent
              positions={positions}
              loading={posLoading}
              closingPositions={closingPositions}
              onClose={handleClosePosition}
              onAddMargin={handleAddMargin}
              onSetTradingStop={handleSetTradingStop}
            />
          ) : (
            <div className={styles.empty}>Positions are only available for perpetual futures.</div>
          )
        )}

        {activeTab === 'orders' && (
          <OrdersContent
            orders={orders}
            loading={ordLoading}
            cancellingOrders={cancellingOrders}
            onCancel={handleCancelOrder}
            onAmend={handleAmendOrder}
          />
        )}

        {activeTab === 'conditional' && (
          <ConditionalContent
            orders={conditionals}
            loading={condLoading}
            cancellingOrders={cancellingConds}
            onCancel={handleCancelConditional}
          />
        )}

        {activeTab === 'history' && (
          <HistoryContent records={history} loading={histLoading} />
        )}

        {activeTab === 'assets' && (
          <AssetsContent balances={balances} loading={balLoading} />
        )}
      </div>
    </div>
  );
}

// Sub-components

const CLOSE_PCTS = [25, 50, 75, 100];

function PositionsContent({
  positions, loading, closingPositions, onClose, onAddMargin, onSetTradingStop,
}: {
  positions: Position[];
  loading: boolean;
  closingPositions: Set<string>;
  onClose: (pos: Position, pct?: number) => void;
  onAddMargin?: (symbol: string, amount: string) => void;
  onSetTradingStop?: (symbol: string, tp?: string, sl?: string) => void;
}) {
  const [marginSymbol, setMarginSymbol] = useState<string | null>(null);
  const [marginAmount, setMarginAmount] = useState('');
  const [tpSlSymbol, setTpSlSymbol] = useState<string | null>(null);
  const [tpInput, setTpInput] = useState('');
  const [slInput, setSlInput] = useState('');

  if (loading) return <div className={styles.loading}>Loading positions...</div>;
  if (positions.length === 0) return <div className={styles.empty}>No open positions</div>;

  return (
    <div className={styles.tableWrap}>
      <table className={styles.table}>
        <thead>
          <tr>
            <th>Symbol</th><th>Side</th><th>Size</th>
            <th>Entry</th><th>Mark</th><th>Liq.</th>
            <th>TP</th><th>SL</th>
            <th>PnL</th><th>Margin</th>
            <th>ADL</th><th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {positions.map((pos) => {
            const key = `${pos.symbol}-${pos.side}`;
            const isLong = pos.side.toLowerCase() === 'buy';
            const pnlNum = parseFloat(pos.unrealized_pnl);
            const adl = pos.adl_quantile ?? 0;
            return (
              <tr key={key}>
                <td>{pos.symbol}</td>
                <td className={isLong ? styles.long : styles.short}>
                  {isLong ? 'Long' : 'Short'}
                </td>
                <td className={styles.mono}>{pos.size}</td>
                <td className={styles.mono}>{pos.entry_price}</td>
                <td className={styles.mono}>{pos.mark_price}</td>
                <td className={cn(styles.mono, styles.short)}>{pos.liq_price ?? '—'}</td>
                <td className={cn(styles.mono, styles.long)}>{pos.tp_price ?? '—'}</td>
                <td className={cn(styles.mono, styles.short)}>{pos.sl_price ?? '—'}</td>
                <td className={cn(styles.mono, isFinite(pnlNum) && pnlNum >= 0 ? styles.long : styles.short)}>
                  {formatPnl(pos.unrealized_pnl)}
                </td>
                <td className={styles.mono}>{pos.margin}</td>
                <td className={styles.mono} title={`ADL quantile: ${adl}/5`}>
                  {'█'.repeat(adl)}{'░'.repeat(5 - adl)}
                </td>
                <td className={styles.closeBtns}>
                  {CLOSE_PCTS.map((pct) => (
                    <button
                      key={pct}
                      type="button"
                      className={cn(styles.closeBtn, pct === 100 && styles.closeBtnFull)}
                      disabled={closingPositions.has(key)}
                      onClick={() => onClose(pos, pct)}
                      title={`Close ${pct}% of position`}
                    >
                      {closingPositions.has(key) ? '…' : `${pct}%`}
                    </button>
                  ))}
                  {onAddMargin && (
                    <button
                      type="button"
                      className={styles.closeBtn}
                      onClick={() => { setMarginSymbol(pos.symbol); setMarginAmount(''); }}
                      title="Add margin to this position"
                    >
                      +Margin
                    </button>
                  )}
                  {onSetTradingStop && (
                    <button
                      type="button"
                      className={styles.closeBtn}
                      onClick={() => { setTpSlSymbol(pos.symbol); setTpInput(pos.tp_price ?? ''); setSlInput(pos.sl_price ?? ''); }}
                      title="Set TP/SL on this position"
                    >
                      TP/SL
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      {/* Add Margin inline dialog */}
      {marginSymbol && onAddMargin && (
        <div className={styles.inlineDialog}>
          <span>Add margin to {marginSymbol}:</span>
          <input
            type="number"
            className={styles.editInput}
            value={marginAmount}
            onChange={(e) => setMarginAmount(e.target.value)}
            placeholder="Amount (USDT)"
            autoFocus
          />
          <button
            type="button"
            className={styles.actionBtn}
            onClick={() => { onAddMargin(marginSymbol, marginAmount); setMarginSymbol(null); }}
          >
            Confirm
          </button>
          <button type="button" className={styles.actionBtn} onClick={() => setMarginSymbol(null)}>Cancel</button>
        </div>
      )}

      {/* Set TP/SL inline dialog */}
      {tpSlSymbol && onSetTradingStop && (
        <div className={styles.inlineDialog}>
          <span>TP/SL for {tpSlSymbol}:</span>
          <input
            type="number"
            className={styles.editInput}
            value={tpInput}
            onChange={(e) => setTpInput(e.target.value)}
            placeholder="Take Profit"
          />
          <input
            type="number"
            className={styles.editInput}
            value={slInput}
            onChange={(e) => setSlInput(e.target.value)}
            placeholder="Stop Loss"
          />
          <button
            type="button"
            className={styles.actionBtn}
            onClick={() => { onSetTradingStop(tpSlSymbol, tpInput, slInput); setTpSlSymbol(null); }}
          >
            Set
          </button>
          <button type="button" className={styles.actionBtn} onClick={() => setTpSlSymbol(null)}>Cancel</button>
        </div>
      )}
    </div>
  );
}

function EditableCell({ value, orderId, field, onAmend }: {
  value: string | null;
  orderId: string;
  field: 'price' | 'qty';
  onAmend: (id: string, changes: { price?: string; qty?: string }) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState(value ?? '');

  if (!value || value === 'Market') return <td className={styles.mono}>{value ?? 'Market'}</td>;

  if (!editing) {
    return (
      <td
        className={cn(styles.mono, styles.editable)}
        onClick={() => { setEditValue(value); setEditing(true); }}
        title="Click to edit"
      >
        {value}
      </td>
    );
  }

  return (
    <td className={styles.mono}>
      <input
        type="number"
        className={styles.editInput}
        value={editValue}
        onChange={(e) => setEditValue(e.target.value)}
        onBlur={() => {
          if (editValue !== value && editValue) {
            void onAmend(orderId, { [field]: editValue });
          }
          setEditing(false);
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            (e.target as HTMLInputElement).blur();
          }
          if (e.key === 'Escape') {
            setEditing(false);
          }
        }}
        autoFocus
        step="any"
      />
    </td>
  );
}

function OrdersContent({
  orders, loading, cancellingOrders, onCancel, onAmend,
}: {
  orders: OpenOrder[];
  loading: boolean;
  cancellingOrders: Set<string>;
  onCancel: (id: string) => void;
  onAmend: (id: string, changes: { price?: string; qty?: string }) => Promise<void>;
}) {
  if (loading) return <div className={styles.loading}>Loading orders...</div>;
  if (orders.length === 0) return <div className={styles.empty}>No open orders</div>;

  return (
    <div className={styles.tableWrap}>
      <table className={styles.table}>
        <thead>
          <tr>
            <th>Symbol</th><th>Side</th><th>Type</th>
            <th>Price</th><th>Trigger</th><th>Amount</th><th>Filled</th>
            <th>Status</th><th>Time</th><th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {orders.map((order) => (
            <tr key={order.order_id}>
              <td>{order.symbol}</td>
              <td className={order.side === 'buy' ? styles.long : styles.short}>
                {order.side === 'buy' ? 'Buy' : 'Sell'}
              </td>
              <td>{order.order_type}</td>
              <EditableCell value={order.price} orderId={order.order_id} field="price" onAmend={onAmend} />
              <td className={styles.mono}>{'—'}</td>
              <EditableCell value={order.qty} orderId={order.order_id} field="qty" onAmend={onAmend} />
              <td className={styles.mono}>{order.filled_qty}</td>
              <td>
                <span className={cn(styles.statusBadge, styles[`status-${order.status}`])}>
                  {order.status}
                </span>
              </td>
              <td className={styles.timeCell}>{formatTime(order.created_at)}</td>
              <td>
                <button
                  type="button"
                  className={styles.actionBtn}
                  disabled={cancellingOrders.has(order.order_id)}
                  onClick={() => onCancel(order.order_id)}
                >
                  {cancellingOrders.has(order.order_id) ? '…' : 'Cancel'}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ConditionalContent({
  orders, loading, cancellingOrders, onCancel,
}: {
  orders: ConditionalOrder[];
  loading: boolean;
  cancellingOrders: Set<string>;
  onCancel: (id: string) => void;
}) {
  if (loading) return <div className={styles.loading}>Loading conditional orders...</div>;
  if (orders.length === 0) return <div className={styles.empty}>No conditional orders</div>;

  return (
    <div className={styles.tableWrap}>
      <table className={styles.table}>
        <thead>
          <tr>
            <th>Symbol</th><th>Side</th><th>Type</th>
            <th>Trigger</th><th>Price</th><th>Amount</th>
            <th>Trigger By</th><th>Status</th><th>Actions</th>
          </tr>
        </thead>
        <tbody>
          {orders.map((order) => (
            <tr key={order.id}>
              <td>{order.symbol}</td>
              <td className={order.side === 'Buy' ? styles.long : styles.short}>
                {order.side}
              </td>
              <td>{order.trigger_type.replace(/_/g, ' ')}</td>
              <td className={styles.mono}>
                {order.trailing_delta
                  ? `Δ ${order.trailing_delta}${order.trail_type === 'Percentage' ? '%' : ''}`
                  : order.trigger_price}
              </td>
              <td className={styles.mono}>{order.order_price ?? 'Market'}</td>
              <td className={styles.mono}>{order.qty}</td>
              <td>{order.trigger_by}</td>
              <td>
                <span className={cn(styles.statusBadge, styles['status-open'])}>
                  {order.status}
                </span>
              </td>
              <td>
                <button
                  type="button"
                  className={styles.actionBtn}
                  disabled={cancellingOrders.has(order.id)}
                  onClick={() => onCancel(order.id)}
                >
                  {cancellingOrders.has(order.id) ? '…' : 'Cancel'}
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const HISTORY_PAGE_SIZE = 50;

function HistoryContent({ records, loading }: { records: OpenOrder[]; loading: boolean }) {
  const [filterSymbol, setFilterSymbol] = useState('');
  const [loadingMore, setLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [allRecords, setAllRecords] = useState<OpenOrder[]>([]);

  // Sync external records into local state
  useEffect(() => {
    setAllRecords(records);
    setHasMore(records.length >= HISTORY_PAGE_SIZE);
  }, [records]);

  const filtered = filterSymbol
    ? allRecords.filter((r) => r.symbol.toLowerCase().includes(filterSymbol.toLowerCase()))
    : allRecords;

  async function loadMore(): Promise<void> {
    setLoadingMore(true);
    try {
      const more = await getOrderHistory({ limit: HISTORY_PAGE_SIZE, offset: allRecords.length });
      setAllRecords((prev) => [...prev, ...more]);
      setHasMore(more.length >= HISTORY_PAGE_SIZE);
    } catch (e) { toast.error(e instanceof Error ? e.message : 'Failed to load history'); } finally {
      setLoadingMore(false);
    }
  }

  if (loading) return <div className={styles.loading}>Loading history...</div>;
  if (allRecords.length === 0) return <div className={styles.empty}>No order history</div>;

  return (
    <div className={styles.tableWrap}>
      <div className={styles.filterRow}>
        <input
          className={styles.filterInput}
          placeholder="Filter by symbol..."
          value={filterSymbol}
          onChange={(e) => setFilterSymbol(e.target.value)}
        />
        <span className={styles.filterCount}>{filtered.length} orders</span>
      </div>
      <table className={styles.table}>
        <thead>
          <tr>
            <th>Symbol</th><th>Side</th><th>Type</th>
            <th>Price</th><th>Qty</th><th>Filled</th><th>Status</th><th>Time</th>
          </tr>
        </thead>
        <tbody>
          {filtered.map((rec) => (
            <tr key={rec.order_id}>
              <td>{rec.symbol}</td>
              <td className={rec.side === 'buy' ? styles.long : styles.short}>
                {rec.side === 'buy' ? 'Buy' : 'Sell'}
              </td>
              <td>{rec.order_type}</td>
              <td className={styles.mono}>{rec.price ?? 'Market'}</td>
              <td className={styles.mono}>{rec.qty}</td>
              <td className={styles.mono}>{rec.filled_qty}</td>
              <td>
                <span className={cn(styles.statusBadge, styles[`status-${rec.status}`])}>
                  {rec.status}
                </span>
              </td>
              <td className={styles.timeCell}>{formatTime(rec.created_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {hasMore && (
        <button
          type="button"
          className={styles.loadMoreBtn}
          disabled={loadingMore}
          onClick={() => void loadMore()}
        >
          {loadingMore ? 'Loading…' : 'Load More'}
        </button>
      )}
    </div>
  );
}

function AssetsContent({ balances, loading }: { balances: BalanceResponse[]; loading: boolean }) {
  if (loading) return <div className={styles.loading}>Loading balances...</div>;
  const nonZero = balances.filter((b) => parseFloat(b.total) > 0);
  if (nonZero.length === 0) return <div className={styles.empty}>No assets — deposit funds to start trading</div>;

  return (
    <div className={styles.tableWrap}>
      <table className={styles.table}>
        <thead>
          <tr>
            <th>Asset</th><th>Available</th><th>Locked</th><th>Total</th>
          </tr>
        </thead>
        <tbody>
          {nonZero.map((b) => (
            <tr key={b.asset}>
              <td>{b.asset}</td>
              <td className={styles.mono}>{b.available}</td>
              <td className={cn(styles.mono, styles.muted)}>{b.locked}</td>
              <td className={styles.mono}>{b.total}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
