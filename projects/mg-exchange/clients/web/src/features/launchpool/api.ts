import { get, post } from '@shared/api/client';
import type { LaunchpoolPool, LaunchpoolStake, StakeRequest } from './types';

export const getPools = (): Promise<LaunchpoolPool[]> =>
  get<{ pools: LaunchpoolPool[] } | LaunchpoolPool[]>('/launchpool/pools').then((res) =>
    Array.isArray(res) ? res : res.pools,
  );

export const stakeInPool = (req: StakeRequest): Promise<void> =>
  post<void>('/launchpool/stake', req);

export const unstakeFromPool = (req: StakeRequest): Promise<void> =>
  post<void>('/launchpool/unstake', req);

export const claimRewards = (pool_id: string): Promise<void> =>
  post<void>('/launchpool/claim', { pool_id });

export const getMyStakes = (): Promise<LaunchpoolStake[]> =>
  get<LaunchpoolStake[]>('/launchpool/my-stakes');
