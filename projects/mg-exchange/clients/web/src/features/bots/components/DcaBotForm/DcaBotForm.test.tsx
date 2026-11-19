import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  createDcaBot: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { DcaBotForm } from './DcaBotForm';
import { createDcaBot } from '../../api';

const mockCreateDcaBot = vi.mocked(createDcaBot);

beforeEach(() => {
  vi.clearAllMocks();
  mockCreateDcaBot.mockResolvedValue({ bot_id: 'bot-1', type: 'dca', symbol: 'BTCUSDT', status: 'active', realized_pnl: '0', created_at: '2023-01-01T00:00:00.000Z' });
});

describe('DcaBotForm', () => {
  it('renders all form fields', async () => {
    await act(async () => { render(<DcaBotForm onCreated={vi.fn()} />); });

    expect(screen.getByLabelText('Symbol')).toBeInTheDocument();
    expect(screen.getByLabelText('Amount per order (USDT)')).toBeInTheDocument();
    expect(screen.getByLabelText('Interval')).toBeInTheDocument();
    expect(screen.getByLabelText('Total Orders')).toBeInTheDocument();
  });

  it('shows validation error when amount is empty', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DcaBotForm onCreated={vi.fn()} />); });

    await user.click(screen.getByRole('button', { name: 'Create DCA Bot' }));

    expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
    expect(mockCreateDcaBot).not.toHaveBeenCalled();
  });

  it('shows validation error when total orders is 0', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DcaBotForm onCreated={vi.fn()} />); });

    const amountInput = screen.getByLabelText('Amount per order (USDT)');
    await user.type(amountInput, '100');

    const ordersInput = screen.getByLabelText('Total Orders');
    await user.clear(ordersInput);
    await user.type(ordersInput, '0');

    await user.click(screen.getByRole('button', { name: 'Create DCA Bot' }));

    expect(screen.getByText('Total orders must be between 1 and 1000')).toBeInTheDocument();
  });

  it('calls createDcaBot with correct config on valid submit', async () => {
    const onCreated = vi.fn();
    const user = userEvent.setup();
    await act(async () => { render(<DcaBotForm onCreated={onCreated} />); });

    const amountInput = screen.getByLabelText('Amount per order (USDT)');
    await user.type(amountInput, '50');

    const ordersInput = screen.getByLabelText('Total Orders');
    await user.clear(ordersInput);
    await user.type(ordersInput, '20');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create DCA Bot' }));
    });

    await waitFor(() => {
      expect(mockCreateDcaBot).toHaveBeenCalledWith(
        expect.objectContaining({
          symbol: 'BTCUSDT',
          amount_per_order: '50',
          total_orders: 20,
        }),
      );
      expect(onCreated).toHaveBeenCalled();
    });
  });

  it('shows total investment calculation', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DcaBotForm onCreated={vi.fn()} />); });

    const amountInput = screen.getByLabelText('Amount per order (USDT)');
    await user.type(amountInput, '100');

    // Default total orders is 10, so 100 * 10 = 1000
    await waitFor(() => {
      expect(screen.getByText('1000.00 USDT')).toBeInTheDocument();
    });
  });
});
