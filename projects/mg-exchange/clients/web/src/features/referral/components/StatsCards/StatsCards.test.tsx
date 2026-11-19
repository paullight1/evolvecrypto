import { render, screen, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { StatsCards } from './StatsCards';
import type { ReferralStats } from '../../types';

const MOCK_STATS: ReferralStats = {
  referral_code: 'ALICE2024',
  referral_url: 'https://exchange.io/register?ref=ALICE2024',
  total_referrals: 42,
  active_referrals: 28,
  total_commissions_usdt: '1234.56',
  pending_commissions_usdt: '100.00',
  commission_rate: '20',
};

describe('StatsCards', () => {
  it('displays the referral code', async () => {
    await act(async () => {
      render(<StatsCards stats={MOCK_STATS} />);
    });

    expect(screen.getByText('ALICE2024')).toBeInTheDocument();
  });

  it('displays total referrals count', async () => {
    await act(async () => {
      render(<StatsCards stats={MOCK_STATS} />);
    });

    expect(screen.getByText('42')).toBeInTheDocument();
  });

  it('displays active referrals count', async () => {
    await act(async () => {
      render(<StatsCards stats={MOCK_STATS} />);
    });

    expect(screen.getByText('28')).toBeInTheDocument();
  });

  it('displays total commissions formatted to 2 decimals', async () => {
    await act(async () => {
      render(<StatsCards stats={MOCK_STATS} />);
    });

    expect(screen.getByText('1234.56')).toBeInTheDocument();
    expect(screen.getByText('$')).toBeInTheDocument();
  });

  it('displays commission rate', async () => {
    await act(async () => {
      render(<StatsCards stats={MOCK_STATS} />);
    });

    expect(screen.getByText('20')).toBeInTheDocument();
    expect(screen.getByText('%')).toBeInTheDocument();
  });
});
