export interface ReferralStats {
  referral_code: string;
  referral_url: string;
  total_referrals: number;
  active_referrals: number;
  total_commissions_usdt: string;
  pending_commissions_usdt: string;
  commission_rate: string;
}

export interface ReferralCommission {
  id: string;
  referral_nickname: string;
  amount_usdt: string;
  source: 'spot' | 'futures' | 'earn';
  created_at: number;
  status: 'paid' | 'pending';
}

export interface ReferralNode {
  user_id: string;
  nickname: string;
  level: number;
  joined_at: number;
  total_commission: string;
  children: ReferralNode[];
}
