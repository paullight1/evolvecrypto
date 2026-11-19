export interface Chain {
  id: string;
  name: string;
  native_token: string;
  rpc_url: string;
  explorer_url: string;
}

export interface SwapHop {
  protocol: string;
  pool_address: string;
  token_in: string;
  token_out: string;
  amount_in: string;
  amount_out: string;
}

export interface DexQuote {
  from_token: string;
  to_token: string;
  from_amount: string;
  to_amount: string;
  price_impact: string;
  fee: string;
  route: SwapHop[];
  expires_at: number;
}

export interface BridgeRoute {
  bridge_name: string;
  from_chain: string;
  to_chain: string;
  token: string;
  estimated_time_seconds: number;
  fee: string;
}

export interface BridgeQuote {
  route: BridgeRoute;
  from_amount: string;
  to_amount: string;
  fee_usd: string;
  expires_at: number;
}

export interface TokenBalance {
  token: string;
  symbol: string;
  balance: string;
  usd_value: string;
  chain: string;
}

export interface Web3Wallet {
  address: string;
  chain: string;
  created_at: string;
}

export interface GetDexQuoteRequest {
  chain: string;
  from_token: string;
  to_token: string;
  amount: string;
}

export interface ExecuteSwapRequest {
  chain: string;
  from_token: string;
  to_token: string;
  amount: string;
  slippage: string;
}

export interface GetBridgeRoutesRequest {
  from_chain: string;
  to_chain: string;
  token: string;
}

export interface GetBridgeQuoteRequest {
  from_chain: string;
  to_chain: string;
  token: string;
  amount: string;
  bridge: string;
}

export interface ExecuteBridgeRequest {
  from_chain: string;
  to_chain: string;
  token: string;
  amount: string;
  bridge: string;
  to_address: string;
}
