export interface FiatMethod {
  id: string;
  name: string;
  type: 'card' | 'bank_transfer' | 'apple_pay' | 'google_pay';
  currencies: string[];
  min_amount: string;
  max_amount: string;
  fee_pct: string;
}

export interface FiatRate {
  fiat_currency: string;
  crypto_asset: string;
  rate: string;
  last_updated: string;
}

export interface PlaceFiatOrderRequest {
  type: 'buy' | 'sell';
  fiat_currency: string;
  crypto_asset: string;
  fiat_amount: string;
  payment_method_id: string;
}

export interface FiatOrder {
  id: string;
  type: 'buy' | 'sell';
  fiat_currency: string;
  crypto_asset: string;
  fiat_amount: string;
  crypto_amount: string;
  rate: string;
  fee: string;
  payment_method: string;
  status: 'pending' | 'processing' | 'completed' | 'failed';
  created_at: string;
}
