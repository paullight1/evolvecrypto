import { get, post } from '@shared/api/client';
import type { KycStatus } from './types';

export const getKycStatus = (): Promise<KycStatus> =>
  get<KycStatus>('/kyc/status');

export const startKyc = (level: number): Promise<{ message: string }> =>
  post<{ message: string }>('/kyc/start', { level });
