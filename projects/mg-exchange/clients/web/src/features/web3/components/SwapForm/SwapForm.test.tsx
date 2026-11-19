import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getDexQuote: vi.fn(),
  executeDexSwap: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { SwapForm } from './SwapForm';
import { getDexQuote, executeDexSwap } from '../../api';
import type { Chain } from '../../types';

const mockGetDexQuote = vi.mocked(getDexQuote);
const mockExecuteDexSwap = vi.mocked(executeDexSwap);

const MOCK_CHAINS: Chain[] = [
  { id: 'ethereum', name: 'Ethereum', native_token: 'ETH', rpc_url: 'https://eth.rpc', explorer_url: 'https://etherscan.io' },
  { id: 'polygon', name: 'Polygon', native_token: 'MATIC', rpc_url: 'https://poly.rpc', explorer_url: 'https://polygonscan.com' },
];

const MOCK_QUOTE = {
  from_token: 'ETH',
  to_token: 'USDC',
  from_amount: '1',
  to_amount: '3200',
  price_impact: '0.1',
  fee: '0.003 ETH',
  route: [],
  expires_at: Math.floor(Date.now() / 1000) + 60,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetDexQuote.mockResolvedValue(MOCK_QUOTE);
  mockExecuteDexSwap.mockResolvedValue({ tx_hash: '0xabc123def456789' });
});

describe('SwapForm', () => {
  it('renders chain selector', async () => {
    await act(async () => { render(<SwapForm chains={MOCK_CHAINS} />); });

    expect(screen.getByLabelText('Chain')).toBeInTheDocument();
  });

  it('renders From and To token selects', async () => {
    await act(async () => { render(<SwapForm chains={MOCK_CHAINS} />); });

    expect(screen.getByLabelText('From')).toBeInTheDocument();
    expect(screen.getByLabelText('To')).toBeInTheDocument();
  });

  it('Get Quote button calls getDexQuote when amount is entered', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SwapForm chains={MOCK_CHAINS} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      expect(mockGetDexQuote).toHaveBeenCalledWith(
        expect.objectContaining({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC', amount: '1' }),
      );
    });
  });

  it('shows quote details after getting quote', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SwapForm chains={MOCK_CHAINS} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => {
      expect(screen.getByText('3200 USDC')).toBeInTheDocument();
    });
  });

  it('Execute Swap button calls executeDexSwap', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<SwapForm chains={MOCK_CHAINS} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '1');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await waitFor(() => screen.getByText('3200 USDC'));

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Execute Swap' }));
    });

    await waitFor(() => {
      expect(mockExecuteDexSwap).toHaveBeenCalledWith(
        expect.objectContaining({ chain: 'ethereum', from_token: 'ETH', to_token: 'USDC' }),
      );
    });
  });
});
