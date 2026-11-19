import { get } from '@shared/api/client';
import type { WalletOverview } from './types';
import type { OpenOrder, Position } from '@features/trading/types';

export const getWalletOverview = (): Promise<WalletOverview> =>
  get<WalletOverview>('/account/wallet');

export const getRecentOrders = (limit = 5): Promise<OpenOrder[]> =>
  get<OpenOrder[]>(`/orders/history?limit=${limit}`);

export const getOpenPositions = (): Promise<Position[]> =>
  get<Position[]>('/positions');
