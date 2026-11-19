import { get, post } from '@shared/api/client';
import type {
  Balance,
  DepositAddress,
  WithdrawalRequest,
  DepositRecord,
  WithdrawalRecord,
  TransferRequest,
  TransferRecord,
} from './types';

export const getBalances = (): Promise<Balance[]> =>
  get<Balance[]>('/account/balances');

export const getDepositAddress = (asset: string, network: string): Promise<DepositAddress> =>
  post<DepositAddress>('/account/deposit-address', { asset, network });

export const submitWithdrawal = (req: WithdrawalRequest): Promise<{ id: string }> =>
  post<{ id: string }>('/account/withdraw', req);

export const getDeposits = (limit = 20): Promise<DepositRecord[]> =>
  get<DepositRecord[]>(`/account/deposits?limit=${limit}`);

export const getWithdrawals = (limit = 20): Promise<WithdrawalRecord[]> =>
  get<WithdrawalRecord[]>(`/account/withdrawals?limit=${limit}`);

export const getTransfers = (limit = 20): Promise<TransferRecord[]> =>
  get<TransferRecord[]>(`/transfers?limit=${limit}`);

export const internalTransfer = (req: TransferRequest): Promise<{ id: string }> =>
  post<{ id: string }>('/transfer', req);

export const getWalletOverview = (): Promise<{ total_equity: string; available: string }> =>
  get('/account/wallet');

export const getWithdrawalFee = (asset: string, network: string): Promise<{ fee: string; min_withdrawal: string }> =>
  get<{ fee: string; min_withdrawal: string }>(`/fees?asset=${asset}&network=${network}`);
