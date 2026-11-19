export interface TradeRecord {
  trade_id: string;
  symbol: string;
  side: string;
  price: string;
  qty: string;
  fee: string;
  role: string;
  created_at: number;
}

export interface FundingRecord {
  id: string;
  symbol: string;
  funding_rate: string;
  amount: string;
  created_at: number;
}

export interface TaxReport {
  year: number;
  total_gains: string;
  total_fees: string;
  trade_count: number;
  download_url?: string;
}
