import { get, post, put, del } from '@shared/api/client';
import type {
  MasterTrader,
  FollowRelation,
  FollowRequest,
  CopyTradingSettings,
} from './types';

export const getLeaderboard = (): Promise<MasterTrader[]> =>
  get<MasterTrader[]>('/copy-trading/leaderboard');

export const followTrader = (req: FollowRequest): Promise<{ relation_id: string }> =>
  post<{ relation_id: string }>('/copy-trading/follow', req);

export const unfollowTrader = (relationId: string): Promise<void> =>
  del<void>(`/copy-trading/follow/${relationId}`);

export const getMyFollowing = (): Promise<FollowRelation[]> =>
  get<FollowRelation[]>('/copy-trading/following');

export const getCopyTradingSettings = (): Promise<CopyTradingSettings> =>
  get<CopyTradingSettings>('/copy-trading/settings');

export const updateSettings = (settings: Partial<CopyTradingSettings>): Promise<CopyTradingSettings> =>
  put<CopyTradingSettings>('/copy-trading/settings', settings);
