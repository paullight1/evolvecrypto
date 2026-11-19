import { render, screen, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { CommissionTable } from './CommissionTable';
import type { ReferralCommission } from '../../types';

const MOCK_COMMISSIONS: ReferralCommission[] = [
  {
    id: 'com-1',
    referral_nickname: 'Bob',
    amount_usdt: '12.50',
    source: 'spot',
    created_at: 1700000000,
    status: 'paid',
  },
  {
    id: 'com-2',
    referral_nickname: 'Carol',
    amount_usdt: '5.00',
    source: 'futures',
    created_at: 1700100000,
    status: 'pending',
  },
];

describe('CommissionTable', () => {
  it('renders referral nicknames', async () => {
    await act(async () => {
      render(<CommissionTable commissions={MOCK_COMMISSIONS} loading={false} />);
    });

    expect(screen.getByText('Bob')).toBeInTheDocument();
    expect(screen.getByText('Carol')).toBeInTheDocument();
  });

  it('renders commission amounts', async () => {
    await act(async () => {
      render(<CommissionTable commissions={MOCK_COMMISSIONS} loading={false} />);
    });

    expect(screen.getByText('12.50')).toBeInTheDocument();
    expect(screen.getByText('5.00')).toBeInTheDocument();
  });

  it('renders source badges', async () => {
    await act(async () => {
      render(<CommissionTable commissions={MOCK_COMMISSIONS} loading={false} />);
    });

    expect(screen.getByText('Spot')).toBeInTheDocument();
    expect(screen.getByText('Futures')).toBeInTheDocument();
  });

  it('renders status badges', async () => {
    await act(async () => {
      render(<CommissionTable commissions={MOCK_COMMISSIONS} loading={false} />);
    });

    expect(screen.getByText('paid')).toBeInTheDocument();
    expect(screen.getByText('pending')).toBeInTheDocument();
  });

  it('shows empty state when no commissions', async () => {
    await act(async () => {
      render(<CommissionTable commissions={[]} loading={false} />);
    });

    expect(screen.getByText('No commissions yet')).toBeInTheDocument();
  });
});
