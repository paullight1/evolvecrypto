import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  repayMargin: vi.fn(),
}));

import { RepayForm } from './RepayForm';
import { repayMargin } from '../../api';
import type { MarginBorrow } from '../../types';

const mockRepayMargin = vi.mocked(repayMargin);

const MOCK_BORROWS: MarginBorrow[] = [
  { asset: 'USDT', amount: '2000', interest_rate: '0.01', interest_accrued: '4.5', created_at: 1700000000 },
  { asset: 'BTC', amount: '0.05', interest_rate: '0.005', interest_accrued: '0.001', created_at: 1700000100 },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockRepayMargin.mockResolvedValue(undefined);
});

describe('RepayForm', () => {
  it('renders the Repay heading', () => {
    render(<RepayForm borrows={MOCK_BORROWS} />);

    // "Repay" appears as both the heading and the submit button span
    expect(screen.getAllByText('Repay').length).toBeGreaterThanOrEqual(1);
  });

  it('Max button fills the amount field with borrow amount', async () => {
    const user = userEvent.setup();
    render(<RepayForm borrows={MOCK_BORROWS} />);

    await user.click(screen.getByRole('button', { name: 'Max' }));

    const amountInput = screen.getByLabelText('Amount') as HTMLInputElement;
    expect(amountInput.value).toBe('2000');
  });

  it('shows validation error when amount is zero on submit', async () => {
    const user = userEvent.setup();
    render(<RepayForm borrows={MOCK_BORROWS} />);

    await user.click(screen.getByRole('button', { name: 'Repay' }));

    expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
    expect(mockRepayMargin).not.toHaveBeenCalled();
  });

  it('submits repay with correct data', async () => {
    const user = userEvent.setup();
    render(<RepayForm borrows={MOCK_BORROWS} />);

    const amountInput = screen.getByLabelText('Amount');
    await user.clear(amountInput);
    await user.type(amountInput, '500');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Repay' }));
    });

    await waitFor(() => {
      expect(mockRepayMargin).toHaveBeenCalledWith(
        expect.objectContaining({ asset: 'USDT', amount: '500' }),
      );
    });
  });

  it('shows empty state when no borrows', () => {
    render(<RepayForm borrows={[]} />);

    expect(screen.getByText('No active borrows to repay.')).toBeInTheDocument();
  });
});
