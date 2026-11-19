import { get, post } from '@shared/api/client';
import type {
  DeliveryInstrument,
  DeliveryPosition,
  DeliveryPrice,
  SettlementRecord,
  PlaceDeliveryOrderRequest,
  PlaceDeliveryOrderResponse,
} from './types';

export const getDeliveryInstruments = (): Promise<DeliveryInstrument[]> =>
  get<DeliveryInstrument[]>('/futures/delivery/instruments');

export const placeDeliveryOrder = (req: PlaceDeliveryOrderRequest): Promise<PlaceDeliveryOrderResponse> =>
  post<PlaceDeliveryOrderResponse>('/futures/delivery/order', req);

export const getDeliveryPositions = (): Promise<DeliveryPosition[]> =>
  get<DeliveryPosition[]>('/futures/delivery/positions');

export const getDeliveryPrice = (symbol: string): Promise<DeliveryPrice> =>
  get<DeliveryPrice>(`/futures/delivery/price/${symbol}`);

export const getSettlementPrices = (limit = 20): Promise<SettlementRecord[]> =>
  get<SettlementRecord[]>(`/market/delivery-prices?limit=${limit}`);
