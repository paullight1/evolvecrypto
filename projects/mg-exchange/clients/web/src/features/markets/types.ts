export interface Market {
  symbol: string;
  base_asset: string;
  quote_asset: string;
  market_type: 'spot' | 'perpetual' | 'delivery';
  last_price: string;
  price_change_pct_24h: string;
  volume_24h: string;
  high_24h: string;
  low_24h: string;
}
