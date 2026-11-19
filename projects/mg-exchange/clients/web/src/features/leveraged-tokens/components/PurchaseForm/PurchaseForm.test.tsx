import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  purchaseLt: vi.fn(),
  redeemLt: vi.fn(),
}));

import { PurchaseForm } from './PurchaseForm';
import { purchaseLt } from '../../api';
import type { LeveragedToken } from '../../types';

const mockPurchaseLt = vi.mocked(purchaseLt);

const MOCK_TOKEN: LeveragedToken = {
  id: 'lt-1',
  symbol: 'BTC3L',
  underlying: 'BTC',
  leverage: '3',
  nav: '100.00',
  change_24h: '5.5',
  total_supply: '1000000',
  management_fee: '0.1',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockPurchaseLt.mockResolvedValue({ tokens_received: '5', usdt_spent: '500' } as never);
});

describe('PurchaseForm', () => {
  it('renders nothing when token is null', async () => {
    await act(async () => {
      render(<PurchaseForm token={null} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders modal with token info when token is provided', async () => {
    await act(async () => {
      render(<PurchaseForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByRole('dialog', { name: /Buy BTC3L/ })).toBeInTheDocument();
    expect(screen.getByText('$100.00')).toBeInTheDocument();
    expect(screen.getByText('3x')).toBeInTheDocument();
  });

  it('shows estimated tokens when amount is entered', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<PurchaseForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    await user.type(screen.getByLabelText('Amount (USDT)'), '500');

    await waitFor(() => {
      // 500 / 100 = 5 BTC3L
      expect(screen.getByText(/5\.000000 BTC3L/)).toBeInTheDocument();
    });
  });

  it('calls purchaseLt and onSuccess on valid submit', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    await act(async () => {
      render(<PurchaseForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={onSuccess} />);
    });

    await user.type(screen.getByLabelText('Amount (USDT)'), '500');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Purchase' }));
    });

    await waitFor(() => {
      expect(mockPurchaseLt).toHaveBeenCalledWith({ token_id: 'lt-1', usdt_amount: '500' });
      expect(onSuccess).toHaveBeenCalled();
    });
  });
});
