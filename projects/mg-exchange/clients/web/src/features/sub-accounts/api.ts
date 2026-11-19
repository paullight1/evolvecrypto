import { get, post } from '@shared/api/client';
import type { SubAccount, SubAccountBalance } from './types';

export const createSubAccount = (label: string, email: string): Promise<SubAccount> =>
  post<SubAccount>('/sub-accounts', { label, email });

export const getSubAccounts = (): Promise<SubAccount[]> =>
  get<SubAccount[]>('/sub-accounts');

export const getSubAccountBalances = (id: string): Promise<SubAccountBalance[]> =>
  get<SubAccountBalance[]>(`/sub-accounts/${id}/balances`);
