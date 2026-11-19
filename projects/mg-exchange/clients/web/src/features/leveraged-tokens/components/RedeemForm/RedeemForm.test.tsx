import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  purchaseLt: vi.fn(),
  redeemLt: vi.fn(),
}));

import { RedeemForm } from './RedeemForm';
import { redeemLt } from '../../api';
import type { LeveragedToken } from '../../types';

const mockRedeemLt = vi.mocked(redeemLt);

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
  mockRedeemLt.mockResolvedValue({ usdt_received: '500', tokens_burned: '5' } as never);
});

describe('RedeemForm', () => {
  it('renders nothing when token is null', async () => {
    await act(async () => {
      render(<RedeemForm token={null} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders modal with NAV info when token is provided', async () => {
    await act(async () => {
      render(<RedeemForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByRole('dialog', { name: /Redeem BTC3L/ })).toBeInTheDocument();
    expect(screen.getByText('$100.00')).toBeInTheDocument();
  });

  it('renders amount input for the token', async () => {
    await act(async () => {
      render(<RedeemForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByLabelText('Amount (BTC3L)')).toBeInTheDocument();
  });

  it('shows estimated USDT when amount is entered', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<RedeemForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    await user.type(screen.getByLabelText('Amount (BTC3L)'), '5');

    await waitFor(() => {
      // 5 * 100 = 500 USDT
      expect(screen.getByText(/500\.00 USDT/)).toBeInTheDocument();
    });
  });

  it('calls redeemLt and onSuccess on valid submit', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    await act(async () => {
      render(<RedeemForm token={MOCK_TOKEN} onClose={vi.fn()} onSuccess={onSuccess} />);
    });

    await user.type(screen.getByLabelText('Amount (BTC3L)'), '5');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Redeem' }));
    });

    await waitFor(() => {
      expect(mockRedeemLt).toHaveBeenCalledWith({ token_id: 'lt-1', token_amount: '5' });
      expect(onSuccess).toHaveBeenCalled();
    });
  });
});
