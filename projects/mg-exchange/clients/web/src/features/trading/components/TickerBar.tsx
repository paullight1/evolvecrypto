import { useState, useRef, useEffect, useCallback } from 'react';
import { useMarketStore } from '../stores/market';
import type { TradingMode } from '../stores/market';
import { useTicker } from '../hooks/useTicker';
import { ws } from '@shared/api/websocket';
import type { ConnectionStatus } from '@shared/api/websocket';
import { Divider } from '@shared/ui/Divider';
import { Countdown } from '@shared/ui/Countdown';
import styles from './TickerBar.module.css';
import { cn } from '@shared/utils/format';

interface TickerBarProps {
  onPairListOpen: () => void;
}

const TRADING_MODE_TABS: { id: TradingMode; label: string }[] = [
  { id: 'spot', label: 'Spot' },
  { id: 'futures', label: 'USDT-M' },
  { id: 'coin_m', label: 'COIN-M' },
];

function formatPrice(p: string): string {
  if (!p) return '—';
  const n = parseFloat(p);
  if (!isFinite(n)) return p;
  if (n >= 10000) return n.toLocaleString('en-US', { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  if (n >= 100) return n.toFixed(2);
  if (n >= 1) return n.toFixed(4);
  return n.toFixed(6);
}

function formatVolume(v: string): string {
  if (!v) return '—';
  const n = parseFloat(v);
  if (!isFinite(n)) return v;
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toFixed(2);
}


export function TickerBar({ onPairListOpen }: TickerBarProps) {
  const {
    symbol,
    productType,
    tradingMode,
    setTradingMode,
    lastPrice,
    priceDirection,
    priceChangePct24h,
    high24h,
    low24h,
    volume24h,
    fundingRate,
    nextFundingTime,
    markPrice,
    indexPrice,
    lastTickerUpdateTime,
  } = useMarketStore();

  useTicker(symbol);

  const isPerp = productType === 'perpetual' || productType === 'coin_m';
  const [flashClass, setFlashClass] = useState('');
  const prevPriceRef = useRef(lastPrice);
  const [wsStatus, setWsStatus] = useState<ConnectionStatus>(() => ws.getStatus());

  const STALE_THRESHOLD_MS = 10_000;
  const [isStale, setIsStale] = useState(false);

  const checkStaleness = useCallback(() => {
    if (lastTickerUpdateTime === 0) {
      setIsStale(false);
    } else {
      setIsStale(Date.now() - lastTickerUpdateTime > STALE_THRESHOLD_MS);
    }
  }, [lastTickerUpdateTime]);

  useEffect(() => {
    checkStaleness();
    const id = setInterval(checkStaleness, 1000);
    return () => clearInterval(id);
  }, [checkStaleness]);

  useEffect(() => {
    return ws.onStatus((status) => setWsStatus(status));
  }, []);

  useEffect(() => {
    if (lastPrice && lastPrice !== prevPriceRef.current) {
      const cls = priceDirection === 'up' ? styles.flashUp : styles.flashDown;
      setFlashClass(cls);
      const t = setTimeout(() => setFlashClass(''), 500);
      prevPriceRef.current = lastPrice;
      return () => clearTimeout(t);
    }
  }, [lastPrice, priceDirection]);

  const pctNum = parseFloat(priceChangePct24h);
  const pctPositive = isFinite(pctNum) && pctNum >= 0;

  return (
    <div className={styles.bar}>
      {/* Trading mode */}
      <div className={styles.modeTabs}>
        {TRADING_MODE_TABS.map((tab) => (
          <button
            key={tab.id}
            type="button"
            className={cn(styles.modeTab, tradingMode === tab.id && styles.modeTabActive)}
            onClick={() => setTradingMode(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Symbol selector */}
      <button type="button" className={styles.symbolBtn} onClick={onPairListOpen}>
        <span className={styles.symbolName}>{symbol}</span>
        <span className={styles.productType}>
          {productType === 'coin_m' ? 'COIN-M' : productType === 'perpetual' ? 'PERP' : 'SPOT'}
        </span>
        <svg width="10" height="6" viewBox="0 0 10 6" fill="currentColor" className={styles.chevron}>
          <path d="M0 0l5 6 5-6z" />
        </svg>
      </button>

      {/* Price + change */}
      <div className={cn(styles.priceBlock, isStale && styles.stale)}>
        <span
          className={cn(
            styles.price,
            priceDirection === 'up' ? styles.priceUp : priceDirection === 'down' ? styles.priceDown : '',
            flashClass,
          )}
        >
          {lastPrice ? formatPrice(lastPrice) : '—'}
        </span>
        {priceChangePct24h && (
          <span className={cn(styles.change, pctPositive ? styles.changePos : styles.changeNeg)}>
            {pctPositive ? '+' : ''}{pctNum.toFixed(2)}%
          </span>
        )}
      </div>

      {/* Stats divider */}
      <Divider orientation="vertical" spacing="sm" />

      {/* 24h stats */}
      <div className={styles.stats}>
        <div className={styles.stat}>
          <span className={styles.statLabel}>24h High</span>
          <span className={styles.statValue}>{formatPrice(high24h)}</span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>24h Low</span>
          <span className={styles.statValue}>{formatPrice(low24h)}</span>
        </div>
        <div className={styles.stat}>
          <span className={styles.statLabel}>24h Vol</span>
          <span className={styles.statValue}>{formatVolume(volume24h)}</span>
        </div>

        {isPerp && (
          <>
            <Divider orientation="vertical" spacing="sm" />
            <div className={styles.stat}>
              <span className={styles.statLabel}>Mark</span>
              <span className={styles.statValue}>{formatPrice(markPrice)}</span>
            </div>
            <div className={styles.stat}>
              <span className={styles.statLabel}>Index</span>
              <span className={styles.statValue}>{formatPrice(indexPrice)}</span>
            </div>
            <div className={styles.stat}>
              <span className={styles.statLabel}>Funding / Next</span>
              <span className={cn(
                styles.statValue,
                fundingRate
                  ? parseFloat(fundingRate) >= 0 ? styles.statValuePos : styles.statValueNeg
                  : '',
              )}>
                {fundingRate ? `${(parseFloat(fundingRate) * 100).toFixed(4)}%` : '—'}
                {' '}{nextFundingTime > 0 && <Countdown target={nextFundingTime} format="hms" completedText="—" className={styles.countdown} />}
              </span>
            </div>
          </>
        )}
      </div>

      {/* Spacer pushes connection dot to far right */}
      <div className={styles.spacer} />

      {/* WS connection status dot — far right */}
      <span
        className={cn(
          styles.connDot,
          wsStatus === 'connected' && styles.connDotConnected,
          wsStatus === 'connecting' && styles.connDotConnecting,
          (wsStatus === 'disconnected' || wsStatus === 'error') && styles.connDotDisconnected,
        )}
        title={`WebSocket: ${wsStatus}`}
      />
    </div>
  );
}
