import { get } from '@shared/api/client';
import type { Market } from './types';

// ── Market List ─────────────────────────────────────────────────────────────

export const getAllMarkets = (): Promise<Market[]> =>
  get<Market[]>('/markets');

export const getAllMarketsDetailed = (): Promise<Market[]> =>
  get<Market[]>('/markets/all');

export const getContractInfo = (symbol: string): Promise<ContractInfo> =>
  get<ContractInfo>(`/markets/${symbol}/contract-info`);

// ── Ticker & Price ──────────────────────────────────────────────────────────

export const getTicker = (symbol: string): Promise<Ticker> =>
  get<Ticker>(`/markets/${symbol}/ticker`);

export const getMarkPrice = (symbol: string): Promise<{ mark_price: string; index_price: string }> =>
  get(`/markets/${symbol}/mark-price`);

export const getBBO = (symbol: string): Promise<{ bid: string; ask: string; bid_size: string; ask_size: string }> =>
  get(`/markets/${symbol}/bbo`);

// ── Order Book & Trades ─────────────────────────────────────────────────────

export const getOrderbook = (symbol: string, depth = 25): Promise<OrderbookSnapshot> =>
  get<OrderbookSnapshot>(`/markets/${symbol}/orderbook?depth=${depth}`);

export const getRecentTrades = (symbol: string, limit = 50): Promise<PublicTrade[]> =>
  get<PublicTrade[]>(`/markets/${symbol}/trades?limit=${limit}`);

export const getAggTrades = (symbol: string, limit = 50): Promise<PublicTrade[]> =>
  get<PublicTrade[]>(`/markets/${symbol}/agg-trades?limit=${limit}`);

// ── Klines ──────────────────────────────────────────────────────────────────

export const getKlines = (symbol: string, interval: string, limit = 200): Promise<Kline[]> =>
  get<Kline[]>(`/markets/${symbol}/klines?interval=${interval}&limit=${limit}`);

export const getMarkKlines = (symbol: string, interval: string, limit = 200): Promise<Kline[]> =>
  get<Kline[]>(`/markets/${symbol}/mark-klines?interval=${interval}&limit=${limit}`);

export const getIndexKlines = (symbol: string, interval: string, limit = 200): Promise<Kline[]> =>
  get<Kline[]>(`/markets/${symbol}/index-klines?interval=${interval}&limit=${limit}`);

export const getPremiumKlines = (symbol: string, interval: string, limit = 200): Promise<Kline[]> =>
  get<Kline[]>(`/markets/${symbol}/premium-klines?interval=${interval}&limit=${limit}`);

// ── Market Intelligence ─────────────────────────────────────────────────────

export const getInsuranceFund = (): Promise<InsuranceFundData> =>
  get<InsuranceFundData>('/market/insurance');

export const getLongShortRatio = (params?: { symbol?: string; period?: string }): Promise<LongShortRatio[]> => {
  const qs = new URLSearchParams();
  if (params?.symbol) qs.set('symbol', params.symbol);
  if (params?.period) qs.set('period', params.period);
  const q = qs.toString();
  return get<LongShortRatio[]>(`/market/long-short-ratio${q ? `?${q}` : ''}`);
};

export const getOpenInterest = (symbol: string): Promise<{ open_interest: string; open_interest_value: string }> =>
  get(`/markets/${symbol}/open-interest`);

export const getOpenInterestHistory = (symbol: string, period = '1h', limit = 50): Promise<OIHistoryPoint[]> =>
  get<OIHistoryPoint[]>(`/market/open-interest-history/${symbol}?period=${period}&limit=${limit}`);

export const getTakerVolume = (symbol: string, period = '1h', limit = 50): Promise<TakerVolumePoint[]> =>
  get<TakerVolumePoint[]>(`/market/taker-volume/${symbol}?period=${period}&limit=${limit}`);

export const getPublicLiquidations = (symbol: string, limit = 50): Promise<LiquidationEvent[]> =>
  get<LiquidationEvent[]>(`/market/liquidations/${symbol}?limit=${limit}`);

// ── Funding ─────────────────────────────────────────────────────────────────

export const getFundingRates = (): Promise<FundingRate[]> =>
  get<FundingRate[]>('/funding/rate');

export const getFundingHistory = (params?: { symbol?: string; limit?: number }): Promise<FundingPayment[]> => {
  const qs = new URLSearchParams();
  if (params?.symbol) qs.set('symbol', params.symbol);
  if (params?.limit) qs.set('limit', String(params.limit));
  const q = qs.toString();
  return get<FundingPayment[]>(`/funding/history${q ? `?${q}` : ''}`);
};

// ── Risk ────────────────────────────────────────────────────────────────────

export const getRiskLimits = (): Promise<RiskLimit[]> =>
  get<RiskLimit[]>('/market/risk-limit');

export const getRiskLimitForSymbol = (symbol: string): Promise<RiskLimit[]> =>
  get<RiskLimit[]>(`/market/risk-limit/${symbol}`);

// ── Types ───────────────────────────────────────────────────────────────────

export interface ContractInfo {
  symbol: string;
  base_asset: string;
  quote_asset: string;
  contract_type: string;
  tick_size: string;
  lot_size: string;
  max_leverage: number;
  listing_time: number;
}

export interface Ticker {
  symbol: string;
  last_price: string;
  price_change_pct_24h: string;
  high_24h: string;
  low_24h: string;
  volume_24h: string;
  turnover_24h: string;
  mark_price?: string;
  index_price?: string;
  funding_rate?: string;
  next_funding_time?: number;
  open_interest?: string;
}

export interface OrderbookSnapshot {
  bids: [string, string][];
  asks: [string, string][];
  timestamp: number;
}

export interface PublicTrade {
  id: string;
  price: string;
  qty: string;
  side: string;
  time: number;
}

export interface Kline {
  open_time: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
}

export interface InsuranceFundData {
  balance: string;
  balance_usd: string;
  history: { time: number; balance: string }[];
}

export interface LongShortRatio {
  symbol: string;
  buy_ratio: string;
  sell_ratio: string;
  time: number;
}

export interface OIHistoryPoint {
  time: number;
  open_interest: string;
  open_interest_value: string;
}

export interface TakerVolumePoint {
  time: number;
  buy_volume: string;
  sell_volume: string;
}

export interface LiquidationEvent extends Record<string, unknown> {
  symbol: string;
  side: string;
  qty: string;
  price: string;
  time: number;
}

export interface FundingRate extends Record<string, unknown> {
  symbol: string;
  funding_rate: string;
  next_funding_time: number;
}

export interface FundingPayment {
  symbol: string;
  amount: string;
  rate: string;
  time: number;
}

export interface RiskLimit {
  symbol: string;
  max_leverage: number;
  maintenance_margin: string;
  initial_margin: string;
  risk_limit_value: string;
}
