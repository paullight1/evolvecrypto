export interface LaunchpoolPool {
  id: string;
  reward_token: string;
  stake_token: string;
  apy: string;
  total_staked: string;
  max_stake: string;
  start_time: string;
  end_time: string;
  status: 'active' | 'upcoming' | 'ended';
}

export interface LaunchpoolStake {
  id: string;
  pool_id: string;
  reward_token: string;
  stake_token: string;
  staked_amount: string;
  rewards_earned: string;
  staked_at: number;
}

export interface StakeRequest {
  pool_id: string;
  amount: string;
}
