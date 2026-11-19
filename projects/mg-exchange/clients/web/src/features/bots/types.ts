export interface Bot {
  bot_id: string;
  type: 'grid' | 'dca' | 'infinity_grid' | 'futures_grid' | 'arb' | 'martingale';
  symbol: string;
  status: 'active' | 'paused' | 'stopped'; // backend uses 'active' not 'running'
  realized_pnl: string;
  created_at: string; // ISO-8601 string (not unix ms)
}

export interface CreateGridBotRequest {
  symbol: string;
  upper_price: string;
  lower_price: string;
  grid_count: number;
  investment: string;
}

export interface CreateDcaBotRequest {
  symbol: string;
  amount_per_order: string; // backend field name (not 'amount')
  interval_secs: number; // backend expects seconds (not hours)
  total_orders: number;
  price_deviation_pct?: string;
}
