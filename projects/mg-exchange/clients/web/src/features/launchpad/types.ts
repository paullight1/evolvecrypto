export type LaunchpadProjectStatus = 'upcoming' | 'active' | 'completed' | 'sold_out';

export interface LaunchpadProject {
  id: string;
  token_name: string;
  token_symbol: string;
  description: string;
  total_raise_usdt: string;
  committed_usdt: string;
  token_price_usdt: string;
  min_commitment_usdt: string;
  max_commitment_usdt: string;
  status: LaunchpadProjectStatus;
  start_time: string;
  end_time: string;
  claim_date: string | null;
  token_distribution: string;
  eligibility_note: string;
  website_url: string | null;
}

export interface LaunchpadCommitment {
  commitment_id: string;
  project_id: string;
  project_name: string;
  token_symbol: string;
  committed_usdt: string;
  estimated_tokens: string;
  allocation_received: string | null;
  claim_date: string | null;
  status: 'pending' | 'confirmed' | 'distributed' | 'refunded';
}

export interface CommitRequest {
  project_id: string;
  amount_usdt: string;
  currency: string;
}
