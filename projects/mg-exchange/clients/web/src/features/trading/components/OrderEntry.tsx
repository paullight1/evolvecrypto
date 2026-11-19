import { useState, useRef, useEffect, useCallback, type FormEvent } from 'react';
import { useTradingStore } from '../stores/trading';
import { useMarketStore } from '../stores/market';
import { useOrderBookStore } from '../stores/orderbook';
import { placeOrder, getBalances, setMarginMode as apiSetMarginMode } from '../api';
import { ApiError } from '@shared/api/client';
import { toast } from '@shared/ui/Toast';
import { Button, NumberInput, Checkbox } from '@shared/ui';
import { Tooltip } from '@shared/ui';
import { cn } from '@shared/utils/format';
import type { OrderType } from '@shared/api/types';
import type { BalanceResponse } from '../types';
import { OrderConfirmModal } from './OrderConfirmModal';
import { LeverageTierInfo } from './LeverageTierInfo';
import styles from './OrderEntry.module.css';

export interface OrderEntryProps {
  bestBid?: string;
  bestAsk?: string;
  onOrderPlaced?: () => void;
}

const ORDER_TYPES: { value: OrderType; label: string }[] = [
  { value: 'limit',         label: 'Limit' },
  { value: 'market',        label: 'Market' },
  { value: 'stop_limit',    label: 'Stop-Limit' },
  { value: 'stop_market',   label: 'Stop-Market' },
  { value: 'trailing_stop', label: 'Trailing' },
];

const LEVERAGE_TICKS = [1, 5, 10, 25, 50, 75, 100];
const QTY_PCTS = [25, 50, 75, 100];

function isStopOrder(t: OrderType): boolean {
  return t === 'stop_limit' || t === 'stop_market';
}

function isTrailingStop(t: OrderType): boolean {
  return t === 'trailing_stop';
}

function hasPrice(t: OrderType): boolean {
  return t === 'limit' || t === 'stop_limit';
}

function quoteAsset(symbol: string): string {
  if (symbol.endsWith('USDT')) return 'USDT';
  if (symbol.endsWith('USDC')) return 'USDC';
  if (symbol.endsWith('USD') && !symbol.endsWith('USDT') && !symbol.endsWith('USDC')) {
    return symbol.slice(0, -3);
  }
  if (symbol.endsWith('BTC')) return 'BTC';
  if (symbol.endsWith('ETH')) return 'ETH';
  return symbol.slice(-4);
}

function formatNotional(price: string, qty: string): string {
  const n = parseFloat(price) * parseFloat(qty);
  if (!isFinite(n)) return '?';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toFixed(2);
}

export function OrderEntry({ bestBid: bestBidProp, bestAsk: bestAskProp, onOrderPlaced }: OrderEntryProps = {}) {
  const { symbol, productType } = useMarketStore();
  const isPerp = productType === 'perpetual' || productType === 'coin_m';

  // Best bid/ask: prefer props if passed, otherwise read from orderbook store
  const obBids = useOrderBookStore((s) => s.bids);
  const obAsks = useOrderBookStore((s) => s.asks);
  const bestBid = bestBidProp ?? obBids[0]?.price;
  const bestAsk = bestAskProp ?? obAsks[0]?.price;

  const {
    orderType, setOrderType,
    side, setSide,
    price, setPrice,
    qty, setQty,
    stopPrice, setStopPrice,
    leverage, setLeverage,
    marginMode, setMarginMode,
    tpPrice, setTpPrice,
    slPrice, setSlPrice,
    reduceOnly, setReduceOnly,
    showTpSl, toggleShowTpSl,
    oneClickMode, setOneClickMode,
    timeInForce, setTimeInForce,
    trailingDelta, setTrailingDelta,
    trailingType, setTrailingType,
    reset,
  } = useTradingStore();

  const [submitting, setSubmitting] = useState(false);
  const [balances, setBalances] = useState<BalanceResponse[]>([]);
  const [showLeveragePopup, setShowLeveragePopup] = useState(false);
  const [showTierInfo, setShowTierInfo] = useState(false);
  const [showConfirm, setShowConfirm] = useState(false);
  const [marginModeChanging, setMarginModeChanging] = useState(false);

  const leverageWrapRef = useRef<HTMLDivElement>(null);
  const tierInfoRef = useRef<HTMLDivElement>(null);

  const fetchBalances = useCallback(async (): Promise<void> => {
    try {
      const data = await getBalances();
      setBalances(data);
    } catch {
      // Non-critical
    }
  }, []);

  useEffect(() => {
    void fetchBalances();
  }, [fetchBalances, symbol]);

  // Close leverage popup on outside click
  useEffect(() => {
    function handler(e: MouseEvent): void {
      if (leverageWrapRef.current && !leverageWrapRef.current.contains(e.target as Node)) {
        setShowLeveragePopup(false);
      }
    }
    if (showLeveragePopup) document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [showLeveragePopup]);

  // Close tier info popover on outside click
  useEffect(() => {
    function handler(e: MouseEvent): void {
      if (tierInfoRef.current && !tierInfoRef.current.contains(e.target as Node)) {
        setShowTierInfo(false);
      }
    }
    if (showTierInfo) document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [showTierInfo]);

  // Keyboard shortcuts in one-click mode
  useEffect(() => {
    if (!oneClickMode) return;
    function handler(e: KeyboardEvent): void {
      const target = e.target as HTMLElement;
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return;
      if (e.key === 'b' || e.key === 'B') setSide('buy');
      if (e.key === 's' || e.key === 'S') setSide('sell');
    }
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [oneClickMode, setSide]);

  const quote = quoteAsset(symbol);
  const quoteBal = balances.find((b) => b.asset === quote);
  const availableNum = quoteBal ? parseFloat(quoteBal.available) : 0;

  async function handleMarginModeChange(mode: 'cross' | 'isolated'): Promise<void> {
    setMarginMode(mode);
    setMarginModeChanging(true);
    try {
      await apiSetMarginMode({ mode, symbol });
    } catch {
      // Non-critical — local state already reflects intent
    } finally {
      setMarginModeChanging(false);
    }
  }

  function fillQtyByPct(pct: number): void {
    if (!availableNum) return;
    // For market orders, use best ask (buy) or best bid (sell) as reference price
    const refPrice = orderType === 'market'
      ? (isBuy ? bestAsk : bestBid)
      : price;
    if (!refPrice) return;
    const p = parseFloat(refPrice);
    if (!isFinite(p) || p <= 0) return;
    const maxQty = (availableNum * (pct / 100)) / p;
    setQty(maxQty.toFixed(6));
  }

  function fillBestPrice(): void {
    if (side === 'buy' && bestAsk !== undefined) setPrice(bestAsk);
    else if (side === 'sell' && bestBid !== undefined) setPrice(bestBid);
  }

  function validateFields(): string | null {
    const qtyNum = parseFloat(qty);
    if (!qty || !isFinite(qtyNum) || qtyNum <= 0) return 'Quantity must be positive';
    if (hasPrice(orderType) && (!price || parseFloat(price) <= 0)) return 'Price must be positive';
    if (isStopOrder(orderType) && (!stopPrice || parseFloat(stopPrice) <= 0)) return 'Stop price must be positive';
    if (isTrailingStop(orderType) && (!trailingDelta || parseFloat(trailingDelta) <= 0)) return 'Trailing delta must be positive';
    return null;
  }

  async function executeOrder(): Promise<void> {
    setSubmitting(true);
    try {
      // TP/SL are passed on the main order so the backend creates linked
      // trigger orders that auto-cancel when the position closes. This avoids
      // the "orphan TP/SL" bug where standalone stop orders survive a manual
      // close and fire on the next position.
      const tpVal = tpPrice && parseFloat(tpPrice) > 0 ? tpPrice : undefined;
      const slVal = slPrice && parseFloat(slPrice) > 0 ? slPrice : undefined;

      const resp = await placeOrder({
        symbol,
        side,
        order_type: orderType,
        price: hasPrice(orderType) ? price : undefined,
        stop_price: isStopOrder(orderType) ? stopPrice : undefined,
        qty,
        time_in_force: hasPrice(orderType) ? timeInForce : 'ioc',
        reduce_only: reduceOnly || undefined,
        leverage: leverage > 1 ? leverage : undefined,
        trailing_stop: isTrailingStop(orderType) ? trailingDelta : undefined,
        trailing_type: isTrailingStop(orderType) ? trailingType : undefined,
        take_profit: tpVal,
        stop_loss: slVal,
      });

      const fillCount = resp.fills.length;
      const msg =
        resp.status === 'filled'
          ? `Filled — ${fillCount} fill${fillCount !== 1 ? 's' : ''} @ ${resp.fills[0]?.price ?? '?'}`
          : resp.status === 'open'
            ? `Order placed (${resp.order_id.slice(0, 8)}…)`
            : `Order ${resp.status}`;

      toast.success(msg);
      reset();
      onOrderPlaced?.();
      void fetchBalances();
    } catch (err) {
      const msg = err instanceof ApiError ? err.message : 'Request failed';
      toast.error(msg);
    } finally {
      setSubmitting(false);
    }
  }

  function handleSubmit(e: FormEvent<HTMLFormElement>): void {
    e.preventDefault();

    const validationError = validateFields();
    if (validationError !== null) {
      toast.error(validationError);
      return;
    }

    if (oneClickMode) {
      void executeOrder();
    } else {
      setShowConfirm(true);
    }
  }

  function handleConfirm(): void {
    setShowConfirm(false);
    void executeOrder();
  }

  const isBuy = side === 'buy';

  const estimatedCost = (() => {
    if (orderType === 'market') return null;
    const p = parseFloat(price);
    const q = parseFloat(qty);
    if (isFinite(p) && p > 0 && isFinite(q) && q > 0) {
      return `$${formatNotional(price, qty)}`;
    }
    return null;
  })();

  const orderPreview = (() => {
    if (!isPerp || !price || !qty) return null;
    const numPrice = parseFloat(price);
    const numQty = parseFloat(qty);
    const numLeverage = leverage || 10;
    if (!isFinite(numPrice) || numPrice <= 0 || !isFinite(numQty) || numQty <= 0) return null;

    const estMargin = (numPrice * numQty / numLeverage).toFixed(2);
    const estLiqPrice = isBuy
      ? (numPrice - numPrice / numLeverage).toFixed(2)
      : (numPrice + numPrice / numLeverage).toFixed(2);
    const estTpProfit = tpPrice && parseFloat(tpPrice) > 0
      ? (Math.abs(parseFloat(tpPrice) - numPrice) * numQty).toFixed(2)
      : null;
    const estSlLoss = slPrice && parseFloat(slPrice) > 0
      ? (Math.abs(parseFloat(slPrice) - numPrice) * numQty).toFixed(2)
      : null;

    return { estMargin, estLiqPrice, estTpProfit, estSlLoss };
  })();

  return (
    <div className={styles.panel}>
      {showConfirm && (
        <OrderConfirmModal
          symbol={symbol}
          side={side}
          orderType={orderType}
          price={price}
          qty={qty}
          leverage={leverage}
          isPerp={isPerp}
          onConfirm={handleConfirm}
          onCancel={() => setShowConfirm(false)}
          onEnableOneClick={() => setOneClickMode(true)}
        />
      )}

      {/* Side buttons — top, full width */}
      <div className={styles.sideRow}>
        <button
          type="button"
          className={cn(styles.sideBtn, styles.sideBtnBuy, isBuy && styles.sideBtnBuyActive)}
          onClick={() => setSide('buy')}
        >
          {isPerp ? 'Buy / Long' : 'Buy'}
        </button>
        <button
          type="button"
          className={cn(styles.sideBtn, styles.sideBtnSell, !isBuy && styles.sideBtnSellActive)}
          onClick={() => setSide('sell')}
        >
          {isPerp ? 'Sell / Short' : 'Sell'}
        </button>
      </div>

      {/* Order type tabs */}
      <div className={styles.typeTabs}>
        {ORDER_TYPES.map((t) => (
          <button
            key={t.value}
            type="button"
            className={cn(styles.typeTab, orderType === t.value && styles.typeTabActive)}
            onClick={() => setOrderType(t.value)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Time in force — shown for limit and stop-limit */}
      {hasPrice(orderType) && (
        <div className={styles.tifRow}>
          {([
            { value: 'gtc' as const, label: 'GTC' },
            { value: 'ioc' as const, label: 'IOC' },
            { value: 'fok' as const, label: 'FOK' },
            { value: 'post_only' as const, label: 'Post Only' },
          ]).map((t) => (
            <button
              key={t.value}
              type="button"
              className={cn(styles.tifBtn, timeInForce === t.value && styles.tifBtnActive)}
              onClick={() => setTimeInForce(t.value)}
            >
              {t.label}
            </button>
          ))}
        </div>
      )}

      {/* Margin mode + leverage — perps only */}
      {isPerp && (
        <div className={styles.marginRow}>
          <div className={styles.marginToggle}>
            {(['cross', 'isolated'] as const).map((m) => (
              <button
                key={m}
                type="button"
                className={cn(styles.marginBtn, marginMode === m && styles.marginBtnActive)}
                disabled={marginModeChanging}
                onClick={() => void handleMarginModeChange(m)}
              >
                {m.charAt(0).toUpperCase() + m.slice(1)}
              </button>
            ))}
          </div>

          <div className={styles.leverageWrap} ref={leverageWrapRef}>
            <button
              type="button"
              className={styles.leverageBtn}
              onClick={() => setShowLeveragePopup((v) => !v)}
            >
              {leverage}x
              <svg width="8" height="5" viewBox="0 0 10 6" fill="currentColor" aria-hidden="true">
                <path d="M0 0l5 6 5-6z" />
              </svg>
            </button>

            {showLeveragePopup && (
              <div className={styles.leveragePopup}>
                <div className={styles.leverageLabel}>
                  <span>Leverage</span>
                  <span className={styles.leverageValue}>{leverage}x</span>
                </div>
                <input
                  type="range"
                  className={styles.leverageSlider}
                  min={1}
                  max={100}
                  step={1}
                  value={leverage}
                  onChange={(e) => setLeverage(parseInt(e.target.value, 10))}
                />
                <div className={styles.leverageTicks}>
                  {LEVERAGE_TICKS.map((v) => (
                    <button
                      key={v}
                      type="button"
                      className={cn(styles.leverageTick, leverage === v && styles.leverageTickActive)}
                      onClick={() => { setLeverage(v); setShowLeveragePopup(false); }}
                    >
                      {v}x
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* Tier info popover */}
          <div className={styles.tierInfoWrap} ref={tierInfoRef}>
            <Tooltip
              content="View leverage &amp; margin tier schedule for this symbol"
              side="bottom"
            >
              <button
                type="button"
                className={styles.tierInfoBtn}
                onClick={() => setShowTierInfo((v) => !v)}
                aria-label="Leverage tier info"
              >
                ?
              </button>
            </Tooltip>
            {showTierInfo && (
              <div className={styles.tierInfoPopover}>
                <LeverageTierInfo symbol={symbol} />
              </div>
            )}
          </div>
        </div>
      )}

      <form className={styles.form} onSubmit={handleSubmit}>

        {/* Stop trigger price */}
        {isStopOrder(orderType) && (
          <div className={styles.field}>
            <NumberInput
              label={`Trigger Price (${quote})`}
              value={stopPrice}
              onChange={setStopPrice}
              placeholder="0.00"
              step={0.01}
              min={0}
              suffix={quote}
              showStepper={false}
            />
          </div>
        )}

        {/* Trailing stop delta */}
        {isTrailingStop(orderType) && (
          <div className={styles.field}>
            <label className={styles.label}>
              Trailing Delta
              <span className={styles.trailTypeToggle}>
                {(['fixed', 'percentage'] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    className={cn(styles.trailTypeBtn, trailingType === t && styles.trailTypeBtnActive)}
                    onClick={() => setTrailingType(t)}
                  >
                    {t === 'fixed' ? '$' : '%'}
                  </button>
                ))}
              </span>
            </label>
            <NumberInput
              value={trailingDelta}
              onChange={setTrailingDelta}
              placeholder={trailingType === 'fixed' ? '0.00' : '1.0'}
              step={trailingType === 'fixed' ? 0.01 : 0.1}
              min={0}
              suffix={trailingType === 'fixed' ? quote : '%'}
              showStepper={false}
            />
          </div>
        )}

        {/* Limit price */}
        {hasPrice(orderType) && (
          <div className={styles.field}>
            <label className={styles.label}>Price ({quote})</label>
            <div className={styles.inputWrap}>
              <NumberInput
                value={price}
                onChange={setPrice}
                placeholder="0.00"
                step={0.01}
                min={0}
                suffix={quote}
                showStepper={false}
              />
              <button
                type="button"
                className={styles.bestBtn}
                onClick={fillBestPrice}
                title={isBuy ? `Best ask: ${bestAsk ?? '—'}` : `Best bid: ${bestBid ?? '—'}`}
              >
                Best
              </button>
            </div>
          </div>
        )}

        {/* Market order info */}
        {orderType === 'market' && (
          <div className={styles.marketInfo}>
            <span>Best bid: <span className={styles.bidPrice}>{bestBid ?? '—'}</span></span>
            <span>Best ask: <span className={styles.askPrice}>{bestAsk ?? '—'}</span></span>
          </div>
        )}

        {/* Quantity */}
        <div className={styles.field}>
          <label className={styles.label}>
            <span>Amount</span>
            {quoteBal && (
              <span className={styles.labelAvail}>
                Available: {parseFloat(quoteBal.available).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })} {quote}
              </span>
            )}
          </label>
          <NumberInput
            value={qty}
            onChange={setQty}
            placeholder="0.00000"
            step={0.00001}
            min={0}
            showStepper={false}
          />
        </div>

        {/* % buttons */}
        <div className={styles.pctRow}>
          {QTY_PCTS.map((pct) => (
            <button
              key={pct}
              type="button"
              className={styles.pctBtn}
              onClick={() => fillQtyByPct(pct)}
            >
              {pct}%
            </button>
          ))}
        </div>

        {/* Estimated cost */}
        {estimatedCost !== null && (
          <div className={styles.costRow}>
            <span>Est. cost</span>
            <span className={styles.costVal}>{estimatedCost}</span>
          </div>
        )}

        {/* TP/SL + reduce-only — perps only */}
        {isPerp && (
          <>
            <Checkbox
              checked={showTpSl}
              onChange={() => toggleShowTpSl()}
              label="Take Profit / Stop Loss"
            />

            {showTpSl && (
              <div className={styles.tpslRow}>
                <div className={styles.field}>
                  <NumberInput
                    label="Take Profit"
                    value={tpPrice}
                    onChange={setTpPrice}
                    placeholder="Optional"
                    step={0.01}
                    min={0}
                    suffix={quote}
                    showStepper={false}
                  />
                </div>
                <div className={styles.field}>
                  <NumberInput
                    label="Stop Loss"
                    value={slPrice}
                    onChange={setSlPrice}
                    placeholder="Optional"
                    step={0.01}
                    min={0}
                    suffix={quote}
                    showStepper={false}
                  />
                </div>
              </div>
            )}

            <Checkbox
              checked={reduceOnly}
              onChange={(v) => setReduceOnly(v)}
              label="Reduce Only"
            />
          </>
        )}

        {/* Order preview — perps only, shown when price + qty filled */}
        {orderPreview !== null && (
          <div className={styles.orderCalc}>
            <div className={styles.calcRow}>
              <span>Est. Margin</span>
              <span className={styles.calcMono}>{orderPreview.estMargin} USDT</span>
            </div>
            <div className={styles.calcRow}>
              <Tooltip
                content="Estimated liquidation price based on entry price and leverage. Actual price may differ."
                side="top"
              >
                <span className={styles.calcHint}>Est. Liq. Price <span className={styles.calcEst}>(est.)</span></span>
              </Tooltip>
              <span className={styles.calcMono}>{orderPreview.estLiqPrice}</span>
            </div>
            {orderPreview.estTpProfit !== null && (
              <div className={styles.calcRow}>
                <span>Est. TP Profit</span>
                <span className={cn(styles.calcMono, styles.calcProfit)}>+{orderPreview.estTpProfit} USDT</span>
              </div>
            )}
            {orderPreview.estSlLoss !== null && (
              <div className={styles.calcRow}>
                <span>Est. SL Loss</span>
                <span className={cn(styles.calcMono, styles.calcLoss)}>-{orderPreview.estSlLoss} USDT</span>
              </div>
            )}
          </div>
        )}

        {/* Submit */}
        <Button
          type="submit"
          variant={isBuy ? 'long' : 'short'}
          fullWidth
          loading={submitting}
          size="lg"
          className={styles.submitBtn}
        >
          {isPerp
            ? `${isBuy ? 'Buy / Long' : 'Sell / Short'}${leverage > 1 ? ` ${leverage}x` : ''}`
            : `${isBuy ? 'Buy' : 'Sell'} ${symbol.replace(/USDT$|USDC$/, '')}`}
        </Button>

        {/* Keyboard shortcut hints — one-click mode only */}
        {oneClickMode && (
          <div className={styles.shortcutHints}>
            <span className={styles.shortcutHint}><kbd className={styles.kbd}>B</kbd> Buy</span>
            <span className={styles.shortcutHint}><kbd className={styles.kbd}>S</kbd> Sell</span>
            <span className={styles.shortcutHint}><kbd className={styles.kbd}>Esc</kbd> Cancel All</span>
            <span className={styles.shortcutHint}><kbd className={styles.kbd}>C</kbd> Close</span>
          </div>
        )}
      </form>
    </div>
  );
}
