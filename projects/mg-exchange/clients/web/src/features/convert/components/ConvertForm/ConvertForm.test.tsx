import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getConvertQuote: vi.fn(),
  executeConvert: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { ConvertForm } from './ConvertForm';
import { getConvertQuote, executeConvert } from '../../api';

const mockGetConvertQuote = vi.mocked(getConvertQuote);
const mockExecuteConvert = vi.mocked(executeConvert);

const MOCK_QUOTE = {
  quote_id: 'q-1',
  from_asset: 'BTC',
  to_asset: 'USDT',
  from_amount: '0.1',
  to_amount: '6500',
  rate: '65000',
  expires_at: Math.floor(Date.now() / 1000) + 30,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetConvertQuote.mockResolvedValue(MOCK_QUOTE);
  mockExecuteConvert.mockResolvedValue(undefined);
});

describe('ConvertForm', () => {
  it('renders From and To selects with initial values', async () => {
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    // Default from=BTC, to=USDT
    const selects = screen.getAllByRole('combobox');
    expect(selects.length).toBeGreaterThanOrEqual(2);
  });

  it('swap button swaps from/to assets', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    const swapBtn = screen.getByRole('button', { name: /⇅/i });
    await user.click(swapBtn);

    // After swap: from=USDT, to=BTC — the From select value changes
    const fromSelect = screen.getByLabelText('From') as HTMLSelectElement;
    expect(fromSelect.value).toBe('USDT');
  });

  it('Get Quote button calls getConvertQuote API', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      expect(mockGetConvertQuote).toHaveBeenCalledWith('BTC', 'USDT', '0.1');
    });
  });

  it('shows quote details after getting quote', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      expect(screen.getByText('6500 USDT')).toBeInTheDocument();
    });
  });

  it('execute button calls executeConvert with quote id', async () => {
    const onConverted = vi.fn();
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={onConverted} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => screen.getByText('6500 USDT'));

    await act(async () => {
      await user.click(screen.getByRole('button', { name: /Convert BTC → USDT/i }));
    });

    await waitFor(() => {
      expect(mockExecuteConvert).toHaveBeenCalledWith('q-1');
      expect(onConverted).toHaveBeenCalled();
    });
  });

  // === Swap Button ===

  it('swap button reverses from and to assets', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    const fromSelect = screen.getByLabelText('From') as HTMLSelectElement;
    const toSelect = screen.getByLabelText('To') as HTMLSelectElement;

    // Initial state: From=BTC, To=USDT
    expect(fromSelect.value).toBe('BTC');
    expect(toSelect.value).toBe('USDT');

    await user.click(screen.getByRole('button', { name: /⇅/i }));

    // After swap: From=USDT, To=BTC
    expect(fromSelect.value).toBe('USDT');
    expect(toSelect.value).toBe('BTC');
  });

  it('swap button clears the active quote', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    // Get a quote first
    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => screen.getByText('6500 USDT'));

    // Swap assets — should clear the quote
    await user.click(screen.getByRole('button', { name: /⇅/i }));

    // Quote card should be gone
    expect(screen.queryByText('6500 USDT')).not.toBeInTheDocument();
  });

  // === Amount Validation ===

  it('Get Quote button does nothing when amount is empty', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    // Leave amount empty
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    // API should not be called because handleGetQuote returns early when !amount
    expect(mockGetConvertQuote).not.toHaveBeenCalled();
  });

  it('shows estimated output amount in quote card', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      // Mock returns to_amount = '6500' and toAsset = 'USDT'
      expect(screen.getByText('6500 USDT')).toBeInTheDocument();
    });
  });

  it('shows rate in quote card', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    await user.type(screen.getByLabelText('Amount'), '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      // Mock rate = '65000': "1 BTC = 65000 USDT"
      expect(screen.getByText('1 BTC = 65000 USDT')).toBeInTheDocument();
    });
  });

  // === Quote Countdown ===

  it('shows countdown timer after getting quote', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    await user.type(screen.getByLabelText('Amount'), '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      // Countdown component renders MM:SS format (e.g. "00:30")
      expect(screen.getByText(/\d{2}:\d{2}/)).toBeInTheDocument();
    });
  });

  it('execute button is disabled when quote has expired', async () => {
    // Use an already-expired quote
    const expiredQuote = {
      ...MOCK_QUOTE,
      expires_at: Math.floor(Date.now() / 1000) - 10, // 10 seconds in the past
    };
    mockGetConvertQuote.mockResolvedValueOnce(expiredQuote);

    const user = userEvent.setup();
    await act(async () => { render(<ConvertForm onConverted={vi.fn()} />); });

    await user.type(screen.getByLabelText('Amount'), '0.1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      // quoteExpired = countdown === 0 && quote !== null → button should be disabled
      const executeBtn = screen.getByRole('button', { name: /Convert BTC → USDT/i });
      expect(executeBtn).toBeDisabled();
    });
  });
});
