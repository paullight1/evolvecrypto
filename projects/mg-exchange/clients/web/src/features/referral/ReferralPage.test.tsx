import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getReferralStats: vi.fn(),
  getCommissions: vi.fn(),
  getReferralTree: vi.fn(),
  sendInvite: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import ReferralPage from './ReferralPage';
import { getReferralStats, getCommissions, sendInvite } from './api';
import type { ReferralStats, ReferralCommission } from './types';

const mockGetReferralStats = vi.mocked(getReferralStats);
const mockGetCommissions = vi.mocked(getCommissions);
const mockSendInvite = vi.mocked(sendInvite);

const MOCK_STATS: ReferralStats = {
  referral_code: 'MYCODE123',
  referral_url: 'https://exchange.com?ref=MYCODE123',
  total_referrals: 12,
  active_referrals: 8,
  total_commissions_usdt: '245.50',
  pending_commissions_usdt: '12.00',
  commission_rate: '20',
};

const MOCK_COMMISSIONS: ReferralCommission[] = [
  {
    id: 'com-1',
    referral_nickname: 'Trader99',
    amount_usdt: '15.00',
    source: 'spot',
    created_at: 1700000000,
    status: 'paid',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetReferralStats.mockResolvedValue(MOCK_STATS);
  mockGetCommissions.mockResolvedValue([]);
  vi.mocked(vi.fn()).mockResolvedValue([]);
  mockSendInvite.mockResolvedValue(undefined);

  // Mock clipboard API
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
});

describe('ReferralPage', () => {
  it('shows referral code with copy button', async () => {
    render(<ReferralPage />);

    expect(await screen.findByText('MYCODE123')).toBeInTheDocument();
    // CopyableText renders copy buttons (one for code, one for URL)
    const copyBtns = screen.getAllByRole('button', { name: /copy/i });
    expect(copyBtns.length).toBeGreaterThanOrEqual(1);
  });

  it('copy button triggers clipboard', async () => {
    render(<ReferralPage />);

    await screen.findByText('MYCODE123');
    // First copy button copies the referral code
    const copyBtns = screen.getAllByRole('button', { name: /copy/i });
    await userEvent.click(copyBtns[0]);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('MYCODE123');
  });

  it('commission table loads data', async () => {
    mockGetCommissions.mockResolvedValue(MOCK_COMMISSIONS);
    render(<ReferralPage />);

    await userEvent.click(screen.getByRole('tab', { name: 'Commissions' }));

    expect(await screen.findByText('Trader99')).toBeInTheDocument();
    expect(screen.getByText('15.00')).toBeInTheDocument();
  });

  it('Total Referrals stat is rendered', async () => {
    render(<ReferralPage />);

    await waitFor(() => {
      expect(screen.getByText('12')).toBeInTheDocument();
    });
    expect(screen.getByText('Total Referrals')).toBeInTheDocument();
  });

  it('commission rate is shown', async () => {
    render(<ReferralPage />);

    await waitFor(() => {
      expect(screen.getByText('Commission Rate')).toBeInTheDocument();
      // StatCard splits value and suffix into separate elements
      expect(screen.getByText('20')).toBeInTheDocument();
    });
  });
});
