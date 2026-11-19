export interface LeveragedToken {
  id: string;
  symbol: string;
  underlying: string;
  leverage: string;
  nav: string;
  change_24h: string;
  total_supply: string;
  management_fee: string;
}

export interface LtPurchaseRequest {
  token_id: string;
  usdt_amount: string;
}

export interface LtRedeemRequest {
  token_id: string;
  token_amount: string;
}
