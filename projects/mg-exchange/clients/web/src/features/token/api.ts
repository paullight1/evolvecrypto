import { get, post } from '@shared/api/client';
import type { BurnRecord, InsuranceStakeInfo, TokenSupplyInfo } from './types';

export const getBurnHistory = (): Promise<BurnRecord[]> =>
  get<BurnRecord[]>('/token/burns');

export const getInsurancePool = (): Promise<InsuranceStakeInfo> =>
  get<InsuranceStakeInfo>('/token/insurance-pool');

export const getTokenSupply = (): Promise<TokenSupplyInfo> =>
  get<TokenSupplyInfo>('/token/supply');

export const stakeInsurance = (amount: string): Promise<void> =>
  post<void>('/token/insurance-pool/stake', { amount });

export const unstakeInsurance = (amount: string): Promise<void> =>
  post<void>('/token/insurance-pool/unstake', { amount });
