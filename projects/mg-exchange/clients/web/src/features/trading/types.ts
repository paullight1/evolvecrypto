export interface PlaceOrderRequest {
  symbol: string;
  side: 'buy' | 'sell';
  order_type: 'limit' | 'market' | 'stop_limit' | 'stop_market' | 'trailing_stop';
  qty: string;
  price?: string;
  stop_price?: string;
  time_in_force?: string; // GTC, IOC, FOK, PostOnly, GTD (case-insensitive)
  take_profit?: string;
  stop_loss?: string;
  leverage?: number;
  trailing_stop?: string;
  trailing_type?: string;
  reduce_only?: boolean;
  position_idx?: number;
  order_link_id?: string;
  iceberg_qty?: string;
  slippage_tolerance?: string;
  close_position?: boolean;
  stp_mode?: string;
  expire_at?: number;
}

export interface Fill {
  trade_id: string; // u64 serialized — use string to avoid JS precision loss
  price: string;
  qty: string;
  role: string;
}

export interface PlaceOrderResponse {
  order_id: string;
  order_link_id?: string | null;
  status: string;
  fills: Fill[];
}

export interface OpenOrder {
  order_id: string;
  order_link_id?: string | null;
  symbol: string;
  side: string;
  order_type: string;
  price: string | null;
  qty: string;
  filled_qty: string;
  status: string; // 'open', 'partially_filled'
  created_at: number; // Unix ms (not string)
  reduce_only?: boolean;
}

export interface Position {
  symbol: string;
  side: string;
  size: string;
  entry_price: string;
  mark_price: string;
  unrealized_pnl: string;
  realized_pnl?: string;
  margin: string;
  leverage: string;
  margin_mode?: string; // 'isolated' | 'cross'
  liq_price?: string | null;
  tp_price?: string | null;
  sl_price?: string | null;
  adl_quantile?: number;
}

export interface Ticker {
  symbol: string;
  last_price: string;
  price_change_pct_24h: string;
  price_change_24h?: string;
  high_24h: string;
  low_24h: string;
  volume_24h: string;
  quote_volume_24h: string;
  funding_rate?: string;
  next_funding_time?: number;
  mark_price?: string;
  index_price?: string;
  open_interest?: string;
}

export interface KlineCandle {
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

export interface KlineResponse {
  open_time: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
  delta?: string;
}

export interface OrderBookResponse {
  bids: [string, string][];
  asks: [string, string][];
  sequence: number;
  timestamp?: number;
  best_bid?: string | null;
  best_ask?: string | null;
  best_bid_qty?: string | null;
  best_ask_qty?: string | null;
  spread?: string;
  spread_pct?: string;
  mid_price?: string;
  bid_pct?: string;
  ask_pct?: string;
}

export interface BalanceResponse {
  asset: string;
  available: string;
  locked: string;
  total: string;
}

// Normalised trade used in UI
export interface RecentTrade {
  trade_id: string;
  price: string;
  qty: string;
  side: 'Buy' | 'Sell';
  timestamp: string; // ISO-8601 string for display
}

// Raw shape returned by GET /markets/{symbol}/trades
export interface RawRestTrade {
  id: number;
  price: string;
  qty: string;
  side: 'Buy' | 'Sell';
  time: number; // unix ms
}
