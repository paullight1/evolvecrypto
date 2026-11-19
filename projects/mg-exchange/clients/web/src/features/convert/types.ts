export interface ConvertQuote {
  from_asset: string;
  to_asset: string;
  from_amount: string;
  to_amount: string;
  rate: string;
  expires_at: number;
  quote_id: string;
}

export interface ConvertRequest {
  quote_id: string;
}

export interface ConvertHistoryItem {
  id: string;
  from_asset: string;
  to_asset: string;
  from_amount: string;
  to_amount: string;
  rate: string;
  status: 'completed' | 'failed' | 'pending';
  created_at: string;
}
