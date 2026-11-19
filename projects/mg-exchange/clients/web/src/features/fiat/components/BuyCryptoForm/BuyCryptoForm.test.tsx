import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getFiatMethods: vi.fn(),
  getFiatRate: vi.fn(),
  placeFiatOrder: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { BuyCryptoForm } from './BuyCryptoForm';
import { getFiatMethods, getFiatRate, placeFiatOrder } from '../../api';

const mockGetFiatMethods = vi.mocked(getFiatMethods);
const mockGetFiatRate = vi.mocked(getFiatRate);
const mockPlaceFiatOrder = vi.mocked(placeFiatOrder);

const MOCK_METHODS = [
  { id: 'card-1', name: 'Credit Card', fee_pct: '1.5' },
  { id: 'bank-1', name: 'Bank Transfer', fee_pct: '0.5' },
];

const MOCK_RATE = { rate: '65000', updated_at: '2025-03-01T10:00:00Z' };

beforeEach(() => {
  vi.clearAllMocks();
  mockGetFiatMethods.mockResolvedValue(MOCK_METHODS);
  mockGetFiatRate.mockResolvedValue(MOCK_RATE);
  mockPlaceFiatOrder.mockResolvedValue(undefined);
});

describe('BuyCryptoForm', () => {
  it('renders buy button with crypto asset label', async () => {
    await act(async () => { render(<BuyCryptoForm mode="buy" onSuccess={vi.fn()} />); });

    await waitFor(() => expect(mockGetFiatMethods).toHaveBeenCalled());

    expect(screen.getByRole('button', { name: 'Buy BTC' })).toBeInTheDocument();
  });

  it('renders sell button when mode is sell', async () => {
    await act(async () => { render(<BuyCryptoForm mode="sell" onSuccess={vi.fn()} />); });

    await waitFor(() => expect(mockGetFiatMethods).toHaveBeenCalled());

    expect(screen.getByRole('button', { name: 'Sell BTC' })).toBeInTheDocument();
  });

  it('amount input accepts numeric value', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<BuyCryptoForm mode="buy" onSuccess={vi.fn()} />); });

    await waitFor(() => expect(mockGetFiatMethods).toHaveBeenCalled());

    const amountInput = screen.getByLabelText('Amount (USD)');
    await user.clear(amountInput);
    await user.type(amountInput, '500');

    expect((amountInput as HTMLInputElement).value).toBe('500');
  });

  it('payment method select is rendered after methods load', async () => {
    await act(async () => { render(<BuyCryptoForm mode="buy" onSuccess={vi.fn()} />); });

    await waitFor(() => {
      expect(screen.getByLabelText('Payment Method')).toBeInTheDocument();
    });
  });

  it('submit calls placeFiatOrder with correct payload', async () => {
    const onSuccess = vi.fn();
    const user = userEvent.setup();
    await act(async () => { render(<BuyCryptoForm mode="buy" onSuccess={onSuccess} />); });

    await waitFor(() => expect(mockGetFiatMethods).toHaveBeenCalled());

    const amountInput = screen.getByLabelText('Amount (USD)');
    await user.clear(amountInput);
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Buy BTC' }));
    });

    await waitFor(() => {
      expect(mockPlaceFiatOrder).toHaveBeenCalledWith(
        expect.objectContaining({
          type: 'buy',
          fiat_currency: 'USD',
          crypto_asset: 'BTC',
          fiat_amount: '100',
        }),
      );
      expect(onSuccess).toHaveBeenCalled();
    });
  });
});
