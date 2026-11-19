export interface SubAccount {
  id: string;
  label: string;
  email: string;
  status: string;
  created_at: number;
}

export interface SubAccountBalance {
  asset: string;
  available: string;
  locked: string;
  total: string;
}
