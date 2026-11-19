export interface KycStatus {
  level: number;
  status: 'none' | 'pending' | 'approved' | 'rejected';
  submitted_at?: number;
  reviewed_at?: number;
  rejection_reason?: string;
}
