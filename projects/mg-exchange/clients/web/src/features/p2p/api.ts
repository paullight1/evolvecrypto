import { get, post } from '@shared/api/client';
import type { P2PAd, P2POrder, CreateP2POrderRequest, CreateP2PAdRequest } from './types';

export const getP2PAds = (side: 'buy' | 'sell', asset?: string, fiat?: string): Promise<P2PAd[]> => {
  const params = new URLSearchParams({ side });
  if (asset) params.set('asset', asset);
  if (fiat) params.set('fiat', fiat);
  return get<P2PAd[]>(`/p2p/ads?${params.toString()}`);
};

export const createP2POrder = (req: CreateP2POrderRequest): Promise<P2POrder> =>
  post<P2POrder>('/p2p/orders', req);

export const markPaid = (id: string): Promise<void> =>
  post<void>(`/p2p/orders/${id}/mark-paid`, {});

export const releaseOrder = (id: string): Promise<void> =>
  post<void>(`/p2p/orders/${id}/release`, {});

export const disputeOrder = (id: string): Promise<void> =>
  post<void>(`/p2p/orders/${id}/dispute`, {});

export const cancelOrder = (id: string): Promise<void> =>
  post<void>(`/p2p/orders/${id}/cancel`, {});

export const getMyOrders = (): Promise<P2POrder[]> =>
  get<P2POrder[]>('/p2p/orders');

export const createAd = (req: CreateP2PAdRequest): Promise<P2PAd> =>
  post<P2PAd>('/p2p/ads', req);

export const getMyAds = (): Promise<P2PAd[]> =>
  get<P2PAd[]>('/p2p/ads/my');
