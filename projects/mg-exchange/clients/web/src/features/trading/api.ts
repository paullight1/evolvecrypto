import { get, post, put, del } from '@shared/api/client';
import type {
  PlaceOrderRequest,
  PlaceOrderResponse,
  OpenOrder,
  Position,
  Ticker,
  KlineResponse,
  OrderBookResponse,
  BalanceResponse,
  RecentTrade,
  RawRestTrade,
} from './types';

export interface MarketInfo {
  symbol: string;
  base_asset: string;
  quote_asset: string;
  market_type: 'spot' | 'perpetual' | 'coin_m';
  last_price: string;
  price_change_pct_24h: string;
  volume_24h: string;
  high_24h: string;
  low_24h: string;
}

export const getMarkets = (): Promise<MarketInfo[]> =>
  get<MarketInfo[]>('/markets');

export const placeOrder = (req: PlaceOrderRequest): Promise<PlaceOrderResponse> =>
  post<PlaceOrderResponse>('/orders', req);

export const cancelOrder = (id: string): Promise<void> =>
  del<void>(`/orders/${id}`);

export const amendOrder = (id: string, changes: { price?: string; qty?: string }): Promise<PlaceOrderResponse> =>
  put<PlaceOrderResponse>(`/orders/${id}`, changes);

export const cancelAllOrders = (symbol?: string): Promise<{ cancelled: number }> =>
  post<{ cancelled: number }>('/orders/cancel-all', symbol ? { symbol } : undefined);

export const getOpenOrders = (symbol?: string): Promise<OpenOrder[]> =>
  get<OpenOrder[]>(`/orders${symbol ? `?symbol=${symbol}` : ''}`);

export const getOrderHistory = (params?: { limit?: number; offset?: number; symbol?: string }): Promise<OpenOrder[]> => {
  const qs = new URLSearchParams();
  if (params?.limit) qs.set('limit', String(params.limit));
  if (params?.offset) qs.set('offset', String(params.offset));
  if (params?.symbol) qs.set('symbol', params.symbol);
  const q = qs.toString();
  return get<OpenOrder[]>(`/orders/history${q ? `?${q}` : ''}`);
};

export const getOrderBook = (symbol: string, depth = 25): Promise<OrderBookResponse> =>
  get<OrderBookResponse>(`/markets/${symbol}/orderbook?depth=${depth}`);

export const getKlines = (symbol: string, interval: string, limit = 200): Promise<KlineResponse[]> =>
  get<KlineResponse[]>(`/markets/${symbol}/klines?interval=${interval}&limit=${limit}`);

export const getTicker = (symbol: string): Promise<Ticker> =>
  get<Ticker>(`/markets/${symbol}/ticker`);

export const getRecentTrades = async (symbol: string, limit = 50): Promise<RecentTrade[]> => {
  const raw = await get<RawRestTrade[]>(`/markets/${symbol}/trades?limit=${limit}`);
  return raw.map((t) => ({
    trade_id: String(t.id),
    price: t.price,
    qty: t.qty,
    side: t.side,
    timestamp: new Date(t.time).toISOString(),
  }));
};

export interface ConditionalOrder {
  id: string;
  symbol: string;
  side: string;
  trigger_type: string;
  trigger_price: string;
  order_price: string | null;
  qty: string;
  trigger_by: string;
  status: string;
  trailing_delta: string | null;
  trail_type: string | null;
  reduce_only: boolean;
  created_at: string;
}

export const getConditionalOrders = (): Promise<ConditionalOrder[]> =>
  get<ConditionalOrder[]>('/trigger-orders');

export const cancelConditionalOrder = (id: string): Promise<void> =>
  del<void>(`/trigger-orders/${id}`);

export const cancelAllConditionalOrders = (): Promise<{ cancelled: number }> =>
  post<{ cancelled: number }>('/trigger-orders/cancel-all');

export const getPositions = (): Promise<Position[]> =>
  get<Position[]>('/positions');

// ── Position Management ─────────────────────────────────────────────────────

export const setLeverage = (body: { symbol: string; leverage: number }): Promise<void> =>
  post<void>('/position/leverage', body);

export const adjustLeverage = (body: { symbol: string; leverage: number }): Promise<void> =>
  put<void>('/position/leverage', body);

export const addMargin = (body: { symbol: string; amount: string }): Promise<void> =>
  post<void>('/position/margin', body);

export const setAutoAddMargin = (body: { symbol: string; autoAddMargin: boolean }): Promise<void> =>
  put<void>('/position/auto-add-margin', body);

export const setTradingStop = (body: {
  symbol: string;
  take_profit?: string;
  stop_loss?: string;
  tp_trigger_by?: 'LastPrice' | 'MarkPrice' | 'IndexPrice';
  sl_trigger_by?: 'LastPrice' | 'MarkPrice' | 'IndexPrice';
}): Promise<void> =>
  post<void>('/position/trading-stop', body);

export interface ClosedPnlRecord {
  symbol: string;
  side: string;
  entry_price: string;
  exit_price: string;
  qty: string;
  pnl: string;
  fee: string;
  closed_at: number;
}

export const getClosedPnl = (params?: { symbol?: string; limit?: number }): Promise<ClosedPnlRecord[]> => {
  const qs = new URLSearchParams();
  if (params?.symbol) qs.set('symbol', params.symbol);
  if (params?.limit) qs.set('limit', String(params.limit));
  const q = qs.toString();
  return get<ClosedPnlRecord[]>(`/position/closed-pnl${q ? `?${q}` : ''}`);
};

export const getPositionHistory = (params?: { symbol?: string; limit?: number }): Promise<Position[]> => {
  const qs = new URLSearchParams();
  if (params?.symbol) qs.set('symbol', params.symbol);
  if (params?.limit) qs.set('limit', String(params.limit));
  const q = qs.toString();
  return get<Position[]>(`/position/history${q ? `?${q}` : ''}`);
};

export const setPositionMode = (body: { mode: 'one_way' | 'hedge'; symbol: string }): Promise<void> =>
  post<void>('/positions/mode', body);

// ── Account ─────────────────────────────────────────────────────────────────

export const getBalances = (): Promise<BalanceResponse[]> =>
  get<BalanceResponse[]>('/account/balances');

export const setMarginMode = (body: { mode: 'cross' | 'isolated'; symbol: string }): Promise<void> =>
  post<void>('/account/margin-mode', body);
