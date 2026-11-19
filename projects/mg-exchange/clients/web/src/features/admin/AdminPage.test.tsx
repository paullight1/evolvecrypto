import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getAdminStats: vi.fn(),
  getKycPending: vi.fn(),
  getAmlAlerts: vi.fn(),
  getPendingWithdrawals: vi.fn(),
  approveKyc: vi.fn(),
  rejectKyc: vi.fn(),
  approveWithdrawal: vi.fn(),
  rejectWithdrawal: vi.fn(),
  getSystemHealth: vi.fn(),
}));

import AdminPage from './AdminPage';
import {
  getAdminStats,
  getKycPending,
  getPendingWithdrawals,
  approveKyc,
  rejectKyc,
} from './api';
import type { AdminStats, KycEntry, PendingWithdrawal } from './types';

const mockGetAdminStats = vi.mocked(getAdminStats);
const mockGetKycPending = vi.mocked(getKycPending);
const mockGetPendingWithdrawals = vi.mocked(getPendingWithdrawals);
const mockApproveKyc = vi.mocked(approveKyc);
const mockRejectKyc = vi.mocked(rejectKyc);

const MOCK_STATS: AdminStats = {
  total_users: 12500,
  active_users_24h: 340,
  total_volume_24h: '1500000',
  open_orders: 875,
  open_positions: 420,
  insurance_fund: '500000',
};

const MOCK_KYC: KycEntry[] = [
  { user_id: 'user-1', email: 'alice@example.com', level: 0, submitted_at: 1700000000, country: 'US' },
  { user_id: 'user-2', email: 'bob@example.com', level: 1, submitted_at: 1700001000, country: 'GB' },
];

const MOCK_WITHDRAWALS: PendingWithdrawal[] = [
  {
    id: 'wd-admin-1',
    user_id: 'user-42',
    asset: 'BTC',
    amount: '2.5',
    address: 'bc1qadmintest1234567890',
    network: 'Bitcoin',
    created_at: 1700002000,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetAdminStats.mockResolvedValue(MOCK_STATS);
  mockGetKycPending.mockResolvedValue(MOCK_KYC);
  mockGetPendingWithdrawals.mockResolvedValue(MOCK_WITHDRAWALS);
  vi.mocked(getAdminStats); // ensure typed
  mockApproveKyc.mockResolvedValue(undefined);
  mockRejectKyc.mockResolvedValue(undefined);
});

describe('AdminPage', () => {
  it('renders title and all tabs', async () => {
    await act(async () => { render(<AdminPage />); });
    expect(screen.getByText('Admin Panel')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Overview' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'KYC' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'AML' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Withdrawals' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'System' })).toBeInTheDocument();
  });

  it('overview tab shows stats loaded from API', async () => {
    await act(async () => { render(<AdminPage />); });

    expect(await screen.findByText('Total Users')).toBeInTheDocument();
    expect(screen.getByText('12,500')).toBeInTheDocument();
    expect(screen.getByText('24h Volume')).toBeInTheDocument();
    expect(screen.getByText('1500000')).toBeInTheDocument();
    expect(mockGetAdminStats).toHaveBeenCalledTimes(1);
  });

  it('KYC tab renders pending review entries', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdminPage />); });

    await user.click(screen.getByRole('tab', { name: 'KYC' }));

    expect(await screen.findByText('alice@example.com')).toBeInTheDocument();
    expect(screen.getByText('bob@example.com')).toBeInTheDocument();
    expect(mockGetKycPending).toHaveBeenCalledTimes(1);
  });

  it('Approve button on KYC entry calls approveKyc with correct user_id', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdminPage />); });

    await user.click(screen.getByRole('tab', { name: 'KYC' }));
    await screen.findByText('alice@example.com');

    const approveBtns = screen.getAllByRole('button', { name: 'Approve' });
    await act(async () => {
      await user.click(approveBtns[0]);
    });

    expect(mockApproveKyc).toHaveBeenCalledWith('user-1', 1);
  });

  it('Reject button on KYC entry calls rejectKyc with correct user_id', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdminPage />); });

    await user.click(screen.getByRole('tab', { name: 'KYC' }));
    await screen.findByText('alice@example.com');

    const rejectBtns = screen.getAllByRole('button', { name: 'Reject' });
    await act(async () => {
      await user.click(rejectBtns[0]);
    });

    expect(mockRejectKyc).toHaveBeenCalledWith('user-1', expect.any(String));
  });

  it('withdrawals tab shows pending withdrawal list', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<AdminPage />); });

    await user.click(screen.getByRole('tab', { name: 'Withdrawals' }));

    expect(await screen.findByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('2.5')).toBeInTheDocument();
    expect(mockGetPendingWithdrawals).toHaveBeenCalledTimes(1);
  });

  it('overview handles stats API failure gracefully', async () => {
    mockGetAdminStats.mockRejectedValueOnce(new Error('Unauthorized'));
    await act(async () => { render(<AdminPage />); });

    expect(screen.getByText('Admin Panel')).toBeInTheDocument();
    // Stats show dashes when null
    expect(screen.getAllByText('—').length).toBeGreaterThan(0);
  });
});
