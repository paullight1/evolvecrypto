export type P2PPaymentMethod =
  | 'bank_transfer'
  | 'paypal'
  | 'sepa'
  | 'cash_app'
  | 'zelle'
  | 'wise';

export type P2POrderStatus =
  | 'Created'
  | 'PaymentSent'
  | 'Released'
  | 'Disputed'
  | 'Cancelled'
  | 'RefundedToSeller'
  | 'ReleasedToBuyer';

export interface P2PAd {
  id: string;
  user_id: string;
  side: 'buy' | 'sell';
  asset: string;
  fiat_currency: string;
  price: string;
  min_amount: string;
  max_amount: string;
  payment_methods: P2PPaymentMethod[];
  created_at: string;
}

export interface P2POrder {
  id: string;
  adId: string;
  buyerId: string;
  sellerId: string;
  asset: string;
  fiatCurrency: string;
  cryptoAmount: string;
  fiatAmount: string;
  status: P2POrderStatus;
  createdAt: number;
}

export interface CreateP2POrderRequest {
  ad_id: string;
  amount: string; // backend field name (not 'crypto_amount')
}

export interface CreateP2PAdRequest {
  side: 'buy' | 'sell';
  asset: string;
  fiat_currency: string; // backend field name (not 'fiat')
  price: string;
  min_amount: string;
  max_amount: string;
  payment_methods: P2PPaymentMethod[];
}
