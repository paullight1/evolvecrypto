export interface MasterTrader {
  user_id: string;
  display_name: string;
  total_pnl: string;
  win_rate: string;
  followers_count: number;
  profit_share_rate: string;
  is_active: boolean;
}

export interface FollowRelation {
  relation_id: string;
  master_id: string;
  allocation: string;
  used_allocation: string;
  proportional: boolean;
  stop_loss_pct: string;
  total_pnl: string;
  is_active: boolean;
  created_at: string;
}

export interface FollowRequest {
  master_id: string;
  allocation: string;
  stop_loss_pct?: string;
}

export interface CopyTradingSettings {
  masterUserId?: string;
  fixedAmount?: string;
  maxPositionSize?: string;
  stopLossPct?: string;
  copyRatio?: string;
}
