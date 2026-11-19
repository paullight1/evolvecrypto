import { get, post } from '@shared/api/client';
import type { TradeRecord, FundingRecord, TaxReport } from './types';
import type { OpenOrder } from '@features/trading/types';

export const getOrderHistory = (from?: number, to?: number, limit = 50): Promise<OpenOrder[]> =>
  get<OpenOrder[]>(`/orders/history?limit=${limit}${from ? `&from=${from}` : ''}${to ? `&to=${to}` : ''}`);

export const getTradeHistory = (from?: number, to?: number, limit = 50): Promise<TradeRecord[]> =>
  get<TradeRecord[]>(`/trades/history?limit=${limit}${from ? `&from=${from}` : ''}${to ? `&to=${to}` : ''}`);

export const getFundingHistory = (limit = 50): Promise<FundingRecord[]> =>
  get<FundingRecord[]>(`/funding/history?limit=${limit}`);

export const getTaxReport = (year: number): Promise<TaxReport> =>
  post<TaxReport>('/reports/tax', { year });
