export interface BurnRecord {
  id: string;
  amount: string;
  tx_hash: string;
  burned_at: number;
  source: 'buyback' | 'fees' | 'manual';
}

export interface InsuranceStakeInfo {
  pool_total_staked: string;
  user_staked: string;
  user_rewards_earned: string;
  apy: string;
  lock_days: number;
}

export interface TokenDistribution {
  label: string;
  pct: number;
  color: string;
}

export interface TokenSupplyInfo {
  total_supply: string;
  circulating_supply: string;
  burned_total: string;
  price_usdt: string;
  market_cap_usdt: string;
}
