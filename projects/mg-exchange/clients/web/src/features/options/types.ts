export interface OptionInstrument {
  symbol: string;
  underlying: string;
  strike: string;
  expiry: string;
  optionType: 'Call' | 'Put';
  contractSize: string;
  settleCoin: string;
  isActive: boolean;
}

export interface OptionTicker {
  symbol: string;
  markPrice: string;
  iv: string;
  openInterest: string;
  volume24h: string;
  turnover24h: string;
  expiryTimestamp: number;
}

export interface OptionMarkPrice {
  symbol: string;
  markPrice: string;
  iv: string;
  delta: string;
  gamma: string;
  theta: string;
  vega: string;
}

export interface OptionPosition {
  symbol: string;
  underlying: string;
  qty: string;
  avgEntryPremium: string;
  markPrice: string;
  unrealisedPnl: string;
  updatedAt: string;
}

export interface OptionGreeks {
  totalDelta: string;
  totalGamma: string;
  totalTheta: string;
  totalVega: string;
}

export interface PlaceOptionOrderRequest {
  symbol: string;
  side: 'buy' | 'sell';
  qty: string;
  premium: string; // required — the price per contract
}

export interface PlaceOptionOrderResponse {
  orderId: string;
  symbol: string;
  side: string;
  qty: string;
  premium: string;
  markPrice: string;
  status: string;
}
