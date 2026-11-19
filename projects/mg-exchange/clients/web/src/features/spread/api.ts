import { get, post } from '@shared/api/client';
import type { SpreadInstrument, PlaceSpreadOrderRequest, SpreadOrderResponse } from './types';

export const getSpreadInstruments = (): Promise<SpreadInstrument[]> =>
  get<{ instruments: SpreadInstrument[] } | SpreadInstrument[]>('/spread/instruments').then((res) =>
    Array.isArray(res) ? res : res.instruments,
  );

export const placeSpreadOrder = (req: PlaceSpreadOrderRequest): Promise<SpreadOrderResponse> =>
  post<SpreadOrderResponse>('/spread/order', req);
