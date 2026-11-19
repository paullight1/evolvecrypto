export interface RfqRequest {
  symbol: string;
  side: 'buy' | 'sell';
  qty: string;
  notional_usd: string;
}

export interface RfqQuote {
  id: string;
  rfq_id: string;
  maker: string;
  price: string;
  qty: string;
  notional_usd: string;
  side: 'buy' | 'sell';
  expires_at: string;
  status: 'pending' | 'accepted' | 'rejected' | 'expired';
}

export interface BlockTradeRfq {
  id: string;
  symbol: string;
  side: 'buy' | 'sell';
  qty: string;
  notional_usd: string;
  status: 'open' | 'filled' | 'cancelled';
  quotes: RfqQuote[];
  created_at: string;
}

export interface BlockTrade {
  id: string;
  symbol: string;
  side: 'buy' | 'sell';
  qty: string;
  price: string;
  notional_usd: string;
  counterparty: string;
  executed_at: string;
}
