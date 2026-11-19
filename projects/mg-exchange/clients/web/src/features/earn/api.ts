import { get, post } from '@shared/api/client';
import type {
  EarnProduct,
  EarnAllocation,
  StakingBalance,
  EarnSubscribeRequest,
  EarnRedeemRequest,
  StakingStakeRequest,
  StakingUnstakeRequest,
} from './types';

// ── Flexible & Fixed Savings ────────────────────────────────────────────────

export const getEarnProducts = (): Promise<EarnProduct[]> =>
  get<{ products: EarnProduct[] } | EarnProduct[]>('/earn/products').then((res) =>
    Array.isArray(res) ? res : res.products,
  );

export const earnSubscribe = (req: EarnSubscribeRequest): Promise<{ allocation_id: string }> =>
  post<{ allocation_id: string }>('/earn/subscribe', req);

export const earnRedeem = (req: EarnRedeemRequest): Promise<void> =>
  post<void>('/earn/redeem', req);

export const getEarnAllocations = (): Promise<EarnAllocation[]> =>
  get<{ allocations: EarnAllocation[] } | EarnAllocation[]>('/earn/allocations').then((res) =>
    Array.isArray(res) ? res : res.allocations,
  );

export const getEarnInterestHistory = (limit = 50): Promise<InterestRecord[]> =>
  get<InterestRecord[]>(`/earn/interest-history?limit=${limit}`);

// ── Staking ─────────────────────────────────────────────────────────────────

export const stakingStake = (req: StakingStakeRequest): Promise<void> =>
  post<void>('/earn/staking/stake', req);

export const stakingUnstake = (req: StakingUnstakeRequest): Promise<void> =>
  post<void>('/earn/staking/unstake', req);

export const stakingClaimRewards = (asset: string): Promise<void> =>
  post<void>('/earn/staking/claim-rewards', { asset });

export const getStakingBalance = (asset: string): Promise<StakingBalance> =>
  get<StakingBalance>(`/earn/staking/balance?asset=${asset}`);

export const getStakingApyHistory = (asset: string, limit = 30): Promise<ApyPoint[]> =>
  get<ApyPoint[]>(`/earn/staking/apy-history?asset=${asset}&limit=${limit}`);

// ── Dual Investment ─────────────────────────────────────────────────────────

export interface DualProduct {
  id: string;
  underlying: string;
  settlement_asset: string;
  direction: 'buy_low' | 'sell_high';
  strike_price: string;
  apy: string;
  duration_days: number;
  settlement_date: string;
}

export interface DualPosition {
  id: string;
  product_id: string;
  amount: string;
  apy: string;
  status: string;
  subscribed_at: string;
  settlement_at: string;
}

export const getDualProducts = (): Promise<DualProduct[]> =>
  get<DualProduct[]>('/earn/dual-investment/products');

export const dualInvestSubscribe = (body: { product_id: string; amount: string }): Promise<{ id: string }> =>
  post<{ id: string }>('/earn/dual-investment/subscribe', body);

export const getDualPositions = (): Promise<DualPosition[]> =>
  get<DualPosition[]>('/earn/dual-investment/positions');

// ── Shark Fin ───────────────────────────────────────────────────────────────

export interface SharkFinPosition {
  id: string;
  amount: string;
  apy_range: string;
  status: string;
  subscribed_at: string;
}

export const sharkFinSubscribe = (body: { product_id: string; amount: string }): Promise<{ id: string }> =>
  post<{ id: string }>('/earn/shark-fin/subscribe', body);

export const getSharkFinPositions = (): Promise<SharkFinPosition[]> =>
  get<SharkFinPosition[]>('/earn/shark-fin/positions');

// ── Liquidity Mining ────────────────────────────────────────────────────────

export interface LiquidityPosition {
  id: string;
  pool: string;
  amount: string;
  share_pct: string;
  earned: string;
  status: string;
}

export const liquidityProvide = (body: { pool_id: string; amount: string }): Promise<{ id: string }> =>
  post<{ id: string }>('/earn/liquidity/provide', body);

export const liquidityWithdraw = (body: { position_id: string }): Promise<void> =>
  post<void>('/earn/liquidity/withdraw', body);

export const getLiquidityPositions = (): Promise<LiquidityPosition[]> =>
  get<LiquidityPosition[]>('/earn/liquidity/my-positions');

// ── Lending ─────────────────────────────────────────────────────────────────

export interface LoanPosition {
  id: string;
  asset: string;
  amount: string;
  interest_rate: string;
  collateral_asset: string;
  collateral_amount: string;
  status: string;
}

export interface LendingRate {
  asset: string;
  hourly_rate: string;
  daily_rate: string;
  available: string;
}

export const getLendingRates = (): Promise<LendingRate[]> =>
  get<LendingRate[]>('/lending/interest-rates');

export const lendingBorrow = (body: { asset: string; amount: string; collateral_asset: string }): Promise<{ loan_id: string }> =>
  post<{ loan_id: string }>('/lending/borrow', body);

export const lendingRepay = (body: { loan_id: string; amount: string }): Promise<void> =>
  post<void>('/lending/repay', body);

export const getLendingLoans = (): Promise<LoanPosition[]> =>
  get<LoanPosition[]>('/lending/loans');

export const loanBorrow = (body: { asset: string; amount: string; collateral_asset: string }): Promise<{ loan_id: string }> =>
  post<{ loan_id: string }>('/loan/borrow', body);

export const loanRepay = (body: { loan_id: string; amount: string }): Promise<void> =>
  post<void>('/loan/repay', body);

export const getLoanPositions = (): Promise<LoanPosition[]> =>
  get<LoanPosition[]>('/loan/positions');

// ── Shared Types ────────────────────────────────────────────────────────────

export interface InterestRecord {
  asset: string;
  amount: string;
  product: string;
  time: number;
}

export interface ApyPoint {
  time: number;
  apy: string;
}
