import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  earnRedeem: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { AllocationTable } from './AllocationTable';
import { earnRedeem } from '../../api';
import type { EarnAllocation } from '../../types';

const mockEarnRedeem = vi.mocked(earnRedeem);

const MOCK_ALLOCATIONS: EarnAllocation[] = [
  {
    allocationId: 'alloc-1',
    productId: 'prod-1',
    coin: 'USDT',
    amount: '5000',
    earnedInterest: '12.50',
    status: 'active',
    startedAt: '2023-11-14T22:13:20.000Z',
    autoCompound: false,
  },
  {
    allocationId: 'alloc-2',
    productId: 'prod-2',
    coin: 'BTC',
    amount: '0.1',
    earnedInterest: '0.0005',
    status: 'redeemed',
    startedAt: '2023-11-15T22:13:20.000Z',
    autoCompound: false,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockEarnRedeem.mockResolvedValue(undefined);
});

describe('AllocationTable', () => {
  it('renders asset and amount rows', () => {
    render(<AllocationTable allocations={MOCK_ALLOCATIONS} onRedeemed={vi.fn()} />);

    expect(screen.getByText('USDT')).toBeInTheDocument();
    expect(screen.getByText('5000')).toBeInTheDocument();
    expect(screen.getByText('BTC')).toBeInTheDocument();
  });

  it('shows Redeem button only for active allocations', () => {
    render(<AllocationTable allocations={MOCK_ALLOCATIONS} onRedeemed={vi.fn()} />);

    const redeemButtons = screen.getAllByRole('button', { name: 'Redeem' });
    // Only the active allocation has a Redeem button
    expect(redeemButtons).toHaveLength(1);
  });

  it('clicking Redeem opens confirm dialog', async () => {
    const user = userEvent.setup();
    render(<AllocationTable allocations={MOCK_ALLOCATIONS} onRedeemed={vi.fn()} />);

    await user.click(screen.getByRole('button', { name: 'Redeem' }));

    expect(screen.getByText('Redeem Allocation')).toBeInTheDocument();
  });

  it('confirming redeem calls earnRedeem and fires onRedeemed', async () => {
    const onRedeemed = vi.fn();
    const user = userEvent.setup();
    render(<AllocationTable allocations={MOCK_ALLOCATIONS} onRedeemed={onRedeemed} />);

    // Open the confirm dialog
    await user.click(screen.getByRole('button', { name: 'Redeem' }));

    // Both the table button and the confirm button are now in DOM
    const redeemBtns = screen.getAllByRole('button', { name: 'Redeem' });
    await act(async () => {
      await user.click(redeemBtns[redeemBtns.length - 1]);
    });

    await waitFor(() => {
      expect(mockEarnRedeem).toHaveBeenCalledWith({ allocationId: 'alloc-1' });
      expect(onRedeemed).toHaveBeenCalled();
    });
  });

  it('shows empty state when no allocations', () => {
    render(<AllocationTable allocations={[]} onRedeemed={vi.fn()} />);

    expect(screen.getByText('No active allocations')).toBeInTheDocument();
  });
});
