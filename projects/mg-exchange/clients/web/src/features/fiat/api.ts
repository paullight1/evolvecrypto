import { get, post } from '@shared/api/client';
import type { FiatMethod, FiatRate, FiatOrder, PlaceFiatOrderRequest } from './types';

export const getFiatMethods = (): Promise<FiatMethod[]> =>
  get<{ methods: FiatMethod[] } | FiatMethod[]>('/fiat/methods').then((res) =>
    Array.isArray(res) ? res : res.methods,
  );

export const getFiatRate = (fiat_currency: string, crypto_asset: string): Promise<FiatRate> =>
  get<FiatRate>(`/fiat/rates?fiat=${fiat_currency}&crypto=${crypto_asset}`);

export const placeFiatOrder = (req: PlaceFiatOrderRequest): Promise<FiatOrder> =>
  post<FiatOrder>('/fiat/deposit', req);

export const getFiatHistory = (limit = 20): Promise<FiatOrder[]> =>
  get<FiatOrder[]>(`/fiat/history?limit=${limit}`);
