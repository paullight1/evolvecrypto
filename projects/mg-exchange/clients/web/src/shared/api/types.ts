// WebSocket outgoing message types

export interface WsOutgoingAuth {
  op: 'auth';
  args?: string[];
}

export interface WsOutgoingSubscribe {
  op: 'subscribe';
  args: string[];
}

export interface WsOutgoingUnsubscribe {
  op: 'unsubscribe';
  args: string[];
}

export interface WsOutgoingPing {
  op: 'ping';
}

export type WsOutgoing =
  | WsOutgoingAuth
  | WsOutgoingSubscribe
  | WsOutgoingUnsubscribe
  | WsOutgoingPing;

// WebSocket incoming message types

export interface WsIncomingAuth {
  op: 'auth';
  success: boolean;
  message?: string;
}

export interface WsIncomingSubscribe {
  op: 'subscribe';
  success: boolean;
  args?: string[];
  message?: string;
}

export interface WsIncomingPong {
  op: 'pong';
}

export interface WsIncomingError {
  op: 'error';
  message: string;
}

// WebSocket data payloads

export interface WsTrade {
  symbol: string;
  price: string;
  qty: string;
  side: 'Buy' | 'Sell';
  trade_id: number;
  timestamp: string;
}

export interface WsOrderBookUpdate {
  symbol: string;
  bids: [string, string][];
  asks: [string, string][];
  sequence: number;
}

export interface WsKline {
  symbol: string;
  interval: string;
  open_time: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
  confirmed: boolean;
}

export interface WsPositionUpdate {
  symbol: string;
  side: string;
  size: string;
  entry_price: string;
  mark_price: string;
  unrealized_pnl: string;
  margin: string;
  leverage: string;
  liq_price: string | null;
}

export type OrderType = 'limit' | 'market' | 'stop_limit' | 'stop_market' | 'trailing_stop';

export interface WsOpenOrderUpdate {
  order_id: string;
  symbol: string;
  side: 'buy' | 'sell';
  order_type: OrderType;
  price: string | null;
  stop_price: string | null;
  qty: string;
  filled_qty: string;
  status: string;
  reduce_only: boolean;
  created_at: string;
}

export interface WsOrderEvent {
  order_id: string;
  symbol: string;
  side: 'Buy' | 'Sell';
  price: string;
  qty: string;
  status: string;
}

export interface WsFillEvent {
  trade_id: number;
  symbol: string;
  price: string;
  qty: string;
  side: 'Buy' | 'Sell';
}

export interface WsIncomingData {
  op: 'data';
  topic: string;
  data:
    | WsTrade
    | WsOrderBookUpdate
    | WsOrderEvent
    | WsFillEvent
    | WsKline
    | WsPositionUpdate
    | WsOpenOrderUpdate;
}

export type WsIncoming =
  | WsIncomingAuth
  | WsIncomingSubscribe
  | WsIncomingPong
  | WsIncomingError
  | WsIncomingData;
