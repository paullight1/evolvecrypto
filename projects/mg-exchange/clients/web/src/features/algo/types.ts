export type AlgoStatus = 'running' | 'paused' | 'completed' | 'cancelled' | 'failed';
export type AlgoType = 'twap' | 'vwap';

export interface AlgoOrder {
  id: string;
  type: AlgoType;
  symbol: string;
  side: 'buy' | 'sell';
  total_qty: string;
  executed_qty: string;
  duration_seconds: number;
  slices?: number;
  status: AlgoStatus;
  created_at: string;
  completed_at?: string;
}

export interface CreateTwapRequest {
  symbol: string;
  side: 'buy' | 'sell';
  qty: string;
  duration_seconds: number;
  slices: number;
}

export interface CreateVwapRequest {
  symbol: string;
  side: 'buy' | 'sell';
  qty: string;
  duration_seconds: number;
}
