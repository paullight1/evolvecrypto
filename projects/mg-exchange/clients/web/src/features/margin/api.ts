import { get, post } from '@shared/api/client';
import type {
  MarginStatus,
  MarginBorrow,
  BorrowableAsset,
  InterestRecord,
  BorrowRequest,
  RepayRequest,
} from './types';

export const toggleSpotMargin = (enable: boolean): Promise<MarginStatus> =>
  post<MarginStatus>('/margin/toggle', { enable });

export const getMarginStatus = (): Promise<MarginStatus> =>
  get<MarginStatus>('/margin/status');

export const borrowMargin = (req: BorrowRequest): Promise<void> =>
  post<void>('/margin/borrow', req);

export const repayMargin = (req: RepayRequest): Promise<void> =>
  post<void>('/margin/repay', req);

export const getActiveBorrows = (): Promise<MarginBorrow[]> =>
  get<MarginBorrow[]>('/margin/borrows');

export const getInterestHistory = (limit = 50): Promise<InterestRecord[]> =>
  get<InterestRecord[]>(`/margin/interest-history?limit=${limit}`);

export const getBorrowableAssets = (): Promise<BorrowableAsset[]> =>
  get<BorrowableAsset[]>('/margin/borrowable');
