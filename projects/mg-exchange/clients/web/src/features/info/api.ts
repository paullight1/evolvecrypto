import { get } from '@shared/api/client';

export interface VipTier {
  level: number;
  name: string;
  min_volume_30d: string;
  min_balance: string;
  maker_fee: string;
  taker_fee: string;
}

export interface FeeSchedule {
  vip_tiers: VipTier[];
  withdrawal_fees?: Record<string, string>;
}

export interface UserFeeInfo {
  tier_level: number;
  tier_name: string;
  maker_fee: string;
  taker_fee: string;
  volume_30d: string;
}

export const getFeeSchedule = (): Promise<FeeSchedule> =>
  get<FeeSchedule>('/fees');

export const getUserFeeInfo = (): Promise<UserFeeInfo> =>
  get<UserFeeInfo>('/account/fee-rate');
