export interface DeliveryInstrument {
  symbol: string;
  underlying: string;
  delivery_date: string;
  days_remaining: number;
  last_price: string;
  mark_price: string;
  basis: string;
  oi: string;
}

export interface DeliveryPosition {
  symbol: string;
  side: string;
  size: string;
  entry_price: string;
  mark_price: string;
  unrealized_pnl: string;
  margin: string;
  delivery_date: string;
}

export interface DeliveryPrice {
  symbol: string;
  delivery_price: string;
  settlement_time: number;
}

export interface SettlementRecord {
  symbol: string;
  settlement_price: string;
  settled_at: number;
  pnl: string;
}

export interface PlaceDeliveryOrderRequest {
  symbol: string;
  side: 'buy' | 'sell';
  order_type: 'limit' | 'market';
  qty: string;
  price?: string;
}

export interface PlaceDeliveryOrderResponse {
  order_id: string;
  status: string;
}
