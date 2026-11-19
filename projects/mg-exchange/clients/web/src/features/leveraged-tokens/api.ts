import { get, post } from '@shared/api/client';
import type { LeveragedToken, LtPurchaseRequest, LtRedeemRequest } from './types';

export const getLeveragedTokens = (): Promise<LeveragedToken[]> =>
  get<LeveragedToken[]>('/leveraged-tokens');

export const purchaseLt = (req: LtPurchaseRequest): Promise<{ tokens_received: string }> =>
  post<{ tokens_received: string }>('/leveraged-tokens/purchase', req);

export const redeemLt = (req: LtRedeemRequest): Promise<{ usdt_received: string }> =>
  post<{ usdt_received: string }>('/leveraged-tokens/redeem', req);
