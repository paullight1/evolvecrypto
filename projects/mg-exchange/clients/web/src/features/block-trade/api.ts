import { get, post } from '@shared/api/client';
import type { BlockTradeRfq, RfqQuote, BlockTrade, RfqRequest } from './types';

export const createRfq = (req: RfqRequest): Promise<BlockTradeRfq> =>
  post<BlockTradeRfq>('/block-trade/rfq', req);

export const getRfqQuotes = (rfqId: string): Promise<RfqQuote[]> =>
  get<RfqQuote[]>(`/block-trade/rfq/${rfqId}/quote`);

export const executeRfq = (rfqId: string, quoteId: string): Promise<BlockTrade> =>
  post<BlockTrade>(`/block-trade/rfq/${rfqId}/execute`, { quote_id: quoteId });

export const getBlockTradeHistory = (limit = 20): Promise<BlockTrade[]> =>
  get<BlockTrade[]>(`/block-trade/history?limit=${limit}`);
