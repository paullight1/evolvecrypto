import { get, post, del } from '@shared/api/client';
import type { AlgoOrder, CreateTwapRequest, CreateVwapRequest } from './types';

export const createTwap = (req: CreateTwapRequest): Promise<AlgoOrder> =>
  post<AlgoOrder>('/algo/twap', req);

export const createVwap = (req: CreateVwapRequest): Promise<AlgoOrder> =>
  post<AlgoOrder>('/algo/vwap', req);

export const getAlgoOrders = (status?: string): Promise<AlgoOrder[]> =>
  get<AlgoOrder[]>(`/algo/orders${status ? `?status=${status}` : ''}`);

export const cancelAlgo = (id: string): Promise<void> =>
  del<void>(`/algo/orders/${id}`);
