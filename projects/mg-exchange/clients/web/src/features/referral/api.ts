import { get, post } from '@shared/api/client';
import type { ReferralStats, ReferralCommission, ReferralNode } from './types';

// Backend may return a different shape; normalise to the UI's ReferralStats contract.
export const getReferralStats = (): Promise<ReferralStats> =>
  get<Record<string, unknown>>('/referral').then((raw) => ({
    referral_code: (raw['referral_code'] ?? raw['code'] ?? '—') as string,
    referral_url: (raw['referral_url'] ?? '') as string,
    total_referrals: (raw['total_referrals'] ?? raw['total_referred'] ?? 0) as number,
    active_referrals: (raw['active_referrals'] ?? 0) as number,
    total_commissions_usdt: (raw['total_commissions_usdt'] ?? '0') as string,
    pending_commissions_usdt: (raw['pending_commissions_usdt'] ?? '0') as string,
    commission_rate: (raw['commission_rate'] ?? '0') as string,
  }));

export const getCommissions = (): Promise<ReferralCommission[]> =>
  get<{ commissions: ReferralCommission[] } | ReferralCommission[]>('/referral/commissions').then((res) =>
    Array.isArray(res) ? res : res.commissions,
  );

export const getReferralTree = (): Promise<ReferralNode[]> =>
  get<ReferralNode[] | unknown>('/referral/tree').then((res) =>
    Array.isArray(res) ? res : [],
  );

export const sendInvite = (email: string): Promise<void> =>
  post<void>('/referral/invite', { email });
