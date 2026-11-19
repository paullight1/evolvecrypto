export interface AdminStats {
  total_users: number;
  active_users_24h: number;
  total_volume_24h: string;
  open_orders: number;
  open_positions: number;
  insurance_fund: string;
}

export interface KycEntry {
  user_id: string;
  email: string;
  level: number;
  submitted_at: number;
  country: string;
}

export interface AmlAlert {
  id: string;
  user_id: string;
  alert_type: string;
  severity: string;
  created_at: number;
  status: string;
}

export interface PendingWithdrawal {
  id: string;
  user_id: string;
  asset: string;
  amount: string;
  address: string;
  network: string;
  created_at: number;
}

export interface SystemHealth {
  api: string;
  websocket: string;
  database: string;
  matching_engine: string;
  latency_ms: number;
}
