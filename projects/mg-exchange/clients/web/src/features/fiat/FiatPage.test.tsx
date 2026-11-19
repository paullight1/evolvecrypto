import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getFiatMethods: vi.fn(),
  getFiatRate: vi.fn(),
  placeFiatOrder: vi.fn(),
  getFiatHistory: vi.fn(),
}));

import FiatPage from './FiatPage';
import { getFiatMethods, getFiatRate, placeFiatOrder, getFiatHistory } from './api';
import type { FiatMethod, FiatRate, FiatOrder } from './types';

const mockGetFiatMethods = vi.mocked(getFiatMethods);
const mockGetFiatRate = vi.mocked(getFiatRate);
const mockPlaceFiatOrder = vi.mocked(placeFiatOrder);
const mockGetFiatHistory = vi.mocked(getFiatHistory);

const MOCK_METHODS: FiatMethod[] = [
  {
    id: 'card-visa',
    name: 'Visa Card',
    type: 'card',
    currencies: ['USD', 'EUR'],
    min_amount: '10',
    max_amount: '10000',
    fee_pct: '1.5',
  },
  {
    id: 'bank-wire',
    name: 'Bank Wire',
    type: 'bank_transfer',
    currencies: ['USD'],
    min_amount: '100',
    max_amount: '100000',
    fee_pct: '0.5',
  },
];

const MOCK_RATE: FiatRate = {
  fiat_currency: 'USD',
  crypto_asset: 'BTC',
  rate: '65000',
  last_updated: new Date().toISOString(),
};

const MOCK_ORDER: FiatOrder = {
  id: 'fiat-1',
  type: 'buy',
  fiat_currency: 'USD',
  crypto_asset: 'BTC',
  fiat_amount: '1000',
  crypto_amount: '0.01538',
  rate: '65000',
  fee: '15',
  payment_method: 'Visa Card',
  status: 'completed',
  created_at: new Date().toISOString(),
};

const MOCK_HISTORY: FiatOrder[] = [
  { ...MOCK_ORDER, id: 'fiat-hist-1' },
  {
    id: 'fiat-hist-2',
    type: 'sell',
    fiat_currency: 'EUR',
    crypto_asset: 'ETH',
    fiat_amount: '500',
    crypto_amount: '0.15',
    rate: '3200',
    fee: '5',
    payment_method: 'Bank Wire',
    status: 'pending',
    created_at: new Date().toISOString(),
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetFiatMethods.mockResolvedValue(MOCK_METHODS);
  mockGetFiatRate.mockResolvedValue(MOCK_RATE);
  mockPlaceFiatOrder.mockResolvedValue(MOCK_ORDER);
  mockGetFiatHistory.mockResolvedValue(MOCK_HISTORY);
});

describe('FiatPage', () => {
  it('renders title and tabs', async () => {
    await act(async () => { render(<FiatPage />); });
    expect(screen.getByText('Buy Crypto')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Buy' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Sell' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'History' })).toBeInTheDocument();
  });

  it('renders payment method options from API', async () => {
    await act(async () => { render(<FiatPage />); });

    // Wait for payment methods to load into the select
    expect(await screen.findByText(/Visa Card \(fee: 1.5%\)/)).toBeInTheDocument();
    expect(screen.getByText(/Bank Wire \(fee: 0.5%\)/)).toBeInTheDocument();
  });

  it('entering an amount triggers rate display', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<FiatPage />); });

    // Wait for rate to be fetched
    await screen.findByText(/Visa Card/);

    const amountInput = screen.getByLabelText(/Amount \(USD\)/i);
    await user.type(amountInput, '1000');

    // Rate card should show estimated crypto amount
    expect(await screen.findByText(/You receive/i)).toBeInTheDocument();
    expect(screen.getByText(/1 BTC = 65,000 USD/i)).toBeInTheDocument();
  });

  it('submit button calls placeFiatOrder with correct params', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<FiatPage />); });

    await screen.findByText(/Visa Card/);

    const amountInput = screen.getByLabelText(/Amount \(USD\)/i);
    await user.type(amountInput, '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: /Buy BTC/i }));
    });

    expect(mockPlaceFiatOrder).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'buy',
        fiat_currency: 'USD',
        crypto_asset: 'BTC',
        fiat_amount: '1000',
        payment_method_id: 'card-visa',
      }),
    );
  });

  it('switching to Sell tab shows sell form', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<FiatPage />); });

    await user.click(screen.getByRole('tab', { name: 'Sell' }));

    expect(await screen.findByRole('button', { name: /Sell BTC/i })).toBeInTheDocument();
  });

  it('history tab shows past fiat orders', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<FiatPage />); });

    await user.click(screen.getByRole('tab', { name: 'History' }));

    expect(await screen.findByText('completed')).toBeInTheDocument();
    expect(screen.getByText('pending')).toBeInTheDocument();
    expect(mockGetFiatHistory).toHaveBeenCalledTimes(1);
  });

  it('shows error toast when placeFiatOrder fails', async () => {
    const { useToastStore } = await import('@shared/ui/Toast/store');
    mockPlaceFiatOrder.mockRejectedValueOnce(new Error('Payment declined'));
    const user = userEvent.setup();
    await act(async () => { render(<FiatPage />); });

    await screen.findByText(/Visa Card/);

    await user.type(screen.getByLabelText(/Amount \(USD\)/i), '1000');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: /Buy BTC/i }));
    });

    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.message === 'Payment declined')).toBe(true);
  });
});
