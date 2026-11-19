export interface EarnProduct {
  id: string;
  coin: string;
  name: string;
  apy: string;
  productType: 'flexible' | 'fixed_30d' | 'staking';
  minAmount: string;
  maxAmount: string;
  lockDays: number;
  supportsAutoCompound: boolean;
}

export interface EarnAllocation {
  allocationId: string;
  productId: string;
  coin: string;
  amount: string;
  earnedInterest: string;
  status: 'active' | 'redeemed';
  startedAt: string;
  unlockAt?: string | null;
  autoCompound: boolean;
}

export interface StakingBalance {
  asset: string;
  staked: string;
  rewards_earned: string;
  apy: string;
}

export interface EarnSubscribeRequest {
  productId: string;
  amount: string;
}

export interface EarnRedeemRequest {
  allocationId: string;
}

export interface StakingStakeRequest {
  asset: string;
  amount: string;
}

export interface StakingUnstakeRequest {
  asset: string;
  amount: string;
}
