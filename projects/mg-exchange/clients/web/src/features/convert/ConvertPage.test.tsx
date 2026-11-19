import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getConvertHistory: vi.fn(),
  getConvertQuote: vi.fn(),
  executeConvert: vi.fn(),
}));

import ConvertPage from './ConvertPage';
import { getConvertHistory, getConvertQuote, executeConvert } from './api';
import type { ConvertHistoryItem, ConvertQuote } from './types';

const mockGetConvertHistory = vi.mocked(getConvertHistory);
const mockGetConvertQuote = vi.mocked(getConvertQuote);
const mockExecuteConvert = vi.mocked(executeConvert);

const MOCK_QUOTE: ConvertQuote = {
  from_asset: 'BTC',
  to_asset: 'USDT',
  from_amount: '0.1',
  to_amount: '6500',
  rate: '65000',
  expires_at: Math.floor(Date.now() / 1000) + 30,
  quote_id: 'quote-abc',
};

const MOCK_HISTORY: ConvertHistoryItem[] = [
  {
    id: 'conv-1',
    from_asset: 'BTC',
    to_asset: 'USDT',
    from_amount: '0.1',
    to_amount: '6500',
    rate: '65000',
    status: 'completed',
    created_at: new Date().toISOString(),
  },
  {
    id: 'conv-2',
    from_asset: 'ETH',
    to_asset: 'USDT',
    from_amount: '1',
    to_amount: '3200',
    rate: '3200',
    status: 'pending',
    created_at: new Date().toISOString(),
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetConvertHistory.mockResolvedValue(MOCK_HISTORY);
  mockGetConvertQuote.mockResolvedValue(MOCK_QUOTE);
  mockExecuteConvert.mockResolvedValue({ id: 'exec-1', status: 'completed' });
});

describe('ConvertPage', () => {
  it('renders title and tabs', () => {
    render(<ConvertPage />);
    expect(screen.getByRole('heading', { name: 'Convert' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Convert' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'History' })).toBeInTheDocument();
  });

  it('shows convert form with asset selectors on default tab', () => {
    render(<ConvertPage />);
    expect(screen.getByLabelText('From')).toBeInTheDocument();
    expect(screen.getByLabelText('To')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Get Quote' })).toBeInTheDocument();
  });

  it('swap button swaps the from and to asset values', async () => {
    const user = userEvent.setup();
    render(<ConvertPage />);

    const fromSelect = screen.getByLabelText('From') as HTMLSelectElement;
    const toSelect = screen.getByLabelText('To') as HTMLSelectElement;

    expect(fromSelect.value).toBe('BTC');
    expect(toSelect.value).toBe('USDT');

    await user.click(screen.getByTitle('Swap assets'));

    expect(fromSelect.value).toBe('USDT');
    expect(toSelect.value).toBe('BTC');
  });

  it('Get Quote calls getConvertQuote with correct params', async () => {
    const user = userEvent.setup();
    render(<ConvertPage />);

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    expect(mockGetConvertQuote).toHaveBeenCalledWith('BTC', 'USDT', '0.1');
  });

  it('after quote loads, convert button calls executeConvert with quote_id', async () => {
    const user = userEvent.setup();
    render(<ConvertPage />);

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    // Quote card should appear
    expect(await screen.findByText('You receive')).toBeInTheDocument();
    expect(screen.getByText('6500 USDT')).toBeInTheDocument();

    await act(async () => {
      await user.click(screen.getByRole('button', { name: /Convert BTC → USDT/i }));
    });

    expect(mockExecuteConvert).toHaveBeenCalledWith('quote-abc');
  });

  it('shows error toast when Get Quote fails', async () => {
    const { useToastStore } = await import('@shared/ui/Toast/store');
    mockGetConvertQuote.mockRejectedValueOnce(new Error('Quote service unavailable'));
    const user = userEvent.setup();
    render(<ConvertPage />);

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.message === 'Quote service unavailable')).toBe(true);
  });

  it('history tab loads and shows conversion records', async () => {
    const user = userEvent.setup();
    render(<ConvertPage />);

    await user.click(screen.getByRole('tab', { name: 'History' }));

    expect(await screen.findByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('ETH')).toBeInTheDocument();
    expect(mockGetConvertHistory).toHaveBeenCalledTimes(1);
  });
});
