export interface SpreadInstrument {
  id: string;
  name: string;
  near_leg: string;
  far_leg: string;
  spread_value: string;
  bid: string;
  ask: string;
  last: string;
  volume_24h: string;
}

export interface PlaceSpreadOrderRequest {
  instrument_id: string;
  side: 'buy' | 'sell';
  qty: string;
  price?: string;
  order_type: 'limit' | 'market';
}

export interface SpreadOrderResponse {
  order_id: string;
  status: string;
}
