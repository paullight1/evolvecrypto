import { get, post } from '@shared/api/client';
import type {
  OptionInstrument,
  OptionTicker,
  OptionMarkPrice,
  OptionGreeks,
  OptionPosition,
  PlaceOptionOrderRequest,
  PlaceOptionOrderResponse,
} from './types';

export const getOptionInstruments = async (underlying: string): Promise<OptionInstrument[]> => {
  const res = await get<{ instruments: OptionInstrument[] }>(`/options/instruments?underlying=${underlying}`);
  return res.instruments ?? [];
};

export const getOptionTickers = async (underlying: string): Promise<OptionTicker[]> => {
  const res = await get<{ tickers: OptionTicker[] }>(`/options/ticker?underlying=${underlying}`);
  return res.tickers ?? [];
};

export const getOptionMarkPrices = (underlying: string): Promise<OptionMarkPrice[]> =>
  get<OptionMarkPrice[]>(`/options/mark-price?underlying=${underlying}`);

export const getOptionPositions = async (): Promise<OptionPosition[]> => {
  const res = await get<{ positions: OptionPosition[] }>('/options/positions');
  return res.positions ?? [];
};

export const getOptionGreeks = (): Promise<OptionGreeks> =>
  get<OptionGreeks>('/options/greeks');

export const placeOptionOrder = (req: PlaceOptionOrderRequest): Promise<PlaceOptionOrderResponse> =>
  post<PlaceOptionOrderResponse>('/options/order', req);
