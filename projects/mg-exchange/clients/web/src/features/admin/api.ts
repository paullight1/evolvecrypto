import { get, post } from '@shared/api/client';
import type { AdminStats, KycEntry, AmlAlert, PendingWithdrawal, SystemHealth } from './types';

export const getAdminStats = (): Promise<AdminStats> =>
  get<AdminStats>('/admin/stats');

export const getKycPending = (): Promise<KycEntry[]> =>
  get<KycEntry[]>('/admin/kyc/pending');

export const getAmlAlerts = (): Promise<AmlAlert[]> =>
  get<AmlAlert[]>('/admin/aml/alerts');

export const getPendingWithdrawals = (): Promise<PendingWithdrawal[]> =>
  get<PendingWithdrawal[]>('/admin/withdrawals/pending');

export const approveKyc = (userId: string, level: number): Promise<void> =>
  post<void>(`/admin/kyc/${userId}/approve`, { level });

export const rejectKyc = (userId: string, reason: string): Promise<void> =>
  post<void>(`/admin/kyc/${userId}/reject`, { reason });

export const approveWithdrawal = (id: string): Promise<void> =>
  post<void>(`/admin/withdrawals/${id}/approve`, undefined);

export const rejectWithdrawal = (id: string, reason: string): Promise<void> =>
  post<void>(`/admin/withdrawals/${id}/reject`, { reason });

export const getSystemHealth = (): Promise<SystemHealth> =>
  get<SystemHealth>('/health/detailed');

// ── System Controls ─────────────────────────────────────────────────────────

export const haltTrading = (): Promise<void> =>
  post<void>('/admin/halt');

export const resumeTrading = (): Promise<void> =>
  post<void>('/admin/resume');

export const getTreasury = (): Promise<TreasuryData> =>
  get<TreasuryData>('/admin/treasury');

export const getReconciliation = (): Promise<ReconciliationReport> =>
  get<ReconciliationReport>('/admin/reconciliation');

// ── User Management ─────────────────────────────────────────────────────────

export const listUsers = (params?: { limit?: number; offset?: number }): Promise<AdminUser[]> => {
  const qs = new URLSearchParams();
  if (params?.limit) qs.set('limit', String(params.limit));
  if (params?.offset) qs.set('offset', String(params.offset));
  const q = qs.toString();
  return get<AdminUser[]>(`/admin/users${q ? `?${q}` : ''}`);
};

export const getUserBalances = (userId: string): Promise<UserBalance[]> =>
  get<UserBalance[]>(`/admin/users/${userId}/balances`);

export const getUserOrders = (userId: string): Promise<unknown[]> =>
  get<unknown[]>(`/admin/users/${userId}/orders`);

export const getUserPositions = (userId: string): Promise<unknown[]> =>
  get<unknown[]>(`/admin/users/${userId}/positions`);

// ── Audit & Compliance ──────────────────────────────────────────────────────

export const getAuditLog = (limit = 100): Promise<AuditEntry[]> =>
  get<AuditEntry[]>(`/admin/audit-log?limit=${limit}`);

export const getTravelRulePending = (): Promise<TravelRuleEntry[]> =>
  get<TravelRuleEntry[]>('/admin/travel-rule/pending');

export const performSurveillanceAction = (body: { alert_id: string; action: string; reason: string }): Promise<void> =>
  post<void>('/admin/surveillance/action', body);

// ── Feature Flags ───────────────────────────────────────────────────────────

export const getFeatureFlags = (): Promise<Record<string, boolean>> =>
  get<Record<string, boolean>>('/admin/feature-flags');

export const setFeatureFlag = (flag: string, enabled: boolean): Promise<void> =>
  post<void>('/admin/feature-flags', { flag, enabled });

// ── Market Management ───────────────────────────────────────────────────────

export const listPairs = (): Promise<AdminPair[]> =>
  get<AdminPair[]>('/admin/pairs');

export const createPair = (body: {
  symbol: string;
  base_asset: string;
  quote_asset: string;
  product_type: string;
  tick_size: string;
  lot_size: string;
}): Promise<void> =>
  post<void>('/admin/pairs', body);

// ── Types ───────────────────────────────────────────────────────────────────

export interface TreasuryData {
  total_balance_usd: string;
  assets: { asset: string; balance: string; usd_value: string }[];
}

export interface ReconciliationReport {
  status: string;
  discrepancies: { asset: string; expected: string; actual: string; diff: string }[];
}

export interface AdminUser {
  user_id: string;
  email: string;
  kyc_level: number;
  created_at: number;
  status: string;
}

export interface UserBalance {
  asset: string;
  total: string;
  available: string;
  locked: string;
}

export interface AuditEntry {
  id: string;
  action: string;
  actor: string;
  target: string;
  timestamp: number;
  details: string;
}

export interface TravelRuleEntry {
  id: string;
  sender: string;
  receiver: string;
  amount: string;
  asset: string;
  status: string;
}

export interface AdminPair {
  symbol: string;
  base_asset: string;
  quote_asset: string;
  product_type: string;
  status: string;
}
