export type CompetitionStatus = 'upcoming' | 'active' | 'ended';
export type CompetitionMetric = 'pnl' | 'roi' | 'volume' | 'trades';

export interface Competition {
  id: string;
  name: string;
  description: string;
  metric: CompetitionMetric;
  prize_pool: string;
  prize_currency: string;
  status: CompetitionStatus;
  participants: number;
  start_time: string;
  end_time: string;
  is_joined: boolean;
}

export interface CompetitionEntry {
  rank: number;
  user_id: string;
  nickname: string;
  score: string;
  prize: string;
}

export interface JoinRequest {
  competition_id: string;
}
