import { get, post } from '@shared/api/client';
import type { ConvertQuote, ConvertHistoryItem } from './types';

export const getConvertQuote = (from_asset: string, to_asset: string, amount: string): Promise<ConvertQuote> =>
  post<ConvertQuote>('/convert/quote', { from_asset, to_asset, amount });

export const executeConvert = (quote_id: string): Promise<{ id: string; status: string }> =>
  post<{ id: string; status: string }>('/convert/execute', { quote_id });

export const getConvertHistory = (limit = 20): Promise<ConvertHistoryItem[]> =>
  get<ConvertHistoryItem[]>(`/convert/history?limit=${limit}`);
