export interface Balance {
  asset: string;
  available: string;
  locked: string;
  total: string;
}

export interface DepositAddress {
  address: string;
  memo?: string | null;
  network: string;
}

export interface WithdrawalRequest {
  asset: string;
  network: string;
  address: string;
  amount: string;
  memo?: string;
}

export interface DepositRecord {
  id: string;
  asset: string;
  network: string;
  amount: string;
  address: string;
  tx_hash?: string;
  status: string;
  created_at: number;
  credited_at?: number | null;
}

export interface WithdrawalRecord {
  id: string;
  asset: string;
  network: string;
  address: string;
  amount: string;
  fee: string;
  status: string;
  created_at: number;
  completed_at?: number | null;
}

export interface TransferRequest {
  from_sub?: string; // sub-account UUID (not 'from_account')
  to_sub?: string;   // sub-account UUID (not 'to_account')
  asset: string;
  amount: string;
}

export interface TransferRecord {
  id: string;
  from: string;
  to: string;
  asset: string;
  amount: string;
  type: string;
  timestamp: number;
}
