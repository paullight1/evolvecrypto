export interface MarginBorrow {
  asset: string;
  amount: string;
  interest_rate: string;
  interest_accrued: string;
  created_at: number;
}

export interface BorrowableAsset {
  asset: string;
  max_borrow: string;
  hourly_rate: string;
  daily_rate: string;
}

export interface MarginStatus {
  enabled: boolean;
  margin_level: string;
  available_margin: string;
  used_margin: string;
  total_assets: string;
  total_liabilities: string;
}

export interface InterestRecord {
  asset: string;
  amount: string;
  rate: string;
  timestamp: number;
}

export interface BorrowRequest {
  asset: string;
  amount: string;
}

export interface RepayRequest {
  asset: string;
  amount: string;
}
