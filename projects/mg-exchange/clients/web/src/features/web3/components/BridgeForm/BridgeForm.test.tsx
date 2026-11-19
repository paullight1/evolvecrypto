import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getBridgeRoutes: vi.fn(),
  getBridgeQuote: vi.fn(),
  executeBridge: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { BridgeForm } from './BridgeForm';
import { getBridgeRoutes, getBridgeQuote, executeBridge } from '../../api';
import type { Chain } from '../../types';

const mockGetBridgeRoutes = vi.mocked(getBridgeRoutes);
const mockGetBridgeQuote = vi.mocked(getBridgeQuote);
const mockExecuteBridge = vi.mocked(executeBridge);

const MOCK_CHAINS: Chain[] = [
  { id: 'ethereum', name: 'Ethereum', native_token: 'ETH', rpc_url: 'https://eth.rpc', explorer_url: 'https://etherscan.io' },
  { id: 'polygon', name: 'Polygon', native_token: 'MATIC', rpc_url: 'https://poly.rpc', explorer_url: 'https://polygonscan.com' },
];

const MOCK_ROUTE = {
  bridge_name: 'Across',
  from_chain: 'ethereum',
  to_chain: 'polygon',
  token: 'USDC',
  estimated_time_seconds: 120,
  fee: '2.50',
};

const MOCK_QUOTE = {
  route: MOCK_ROUTE,
  from_amount: '100',
  to_amount: '97.50',
  fee_usd: '2.50',
  expires_at: Math.floor(Date.now() / 1000) + 60,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetBridgeRoutes.mockResolvedValue([MOCK_ROUTE]);
  mockGetBridgeQuote.mockResolvedValue(MOCK_QUOTE);
  mockExecuteBridge.mockResolvedValue({ tx_hash: '0xbridge123456789' });
});

describe('BridgeForm', () => {
  it('renders From Chain and To Chain selectors', async () => {
    await act(async () => { render(<BridgeForm chains={MOCK_CHAINS} />); });

    expect(screen.getByLabelText('From Chain')).toBeInTheDocument();
    expect(screen.getByLabelText('To Chain')).toBeInTheDocument();
  });

  it('renders token selector with default USDC', async () => {
    await act(async () => { render(<BridgeForm chains={MOCK_CHAINS} />); });

    const tokenSelect = screen.getByLabelText('Token') as HTMLSelectElement;
    expect(tokenSelect.value).toBe('USDC');
  });

  it('Get Bridge Quote calls getBridgeRoutes and getBridgeQuote', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<BridgeForm chains={MOCK_CHAINS} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Bridge Quote' }));
    });

    await waitFor(() => {
      expect(mockGetBridgeRoutes).toHaveBeenCalledWith(
        expect.objectContaining({ from_chain: 'ethereum', to_chain: 'polygon', token: 'USDC' }),
      );
      expect(mockGetBridgeQuote).toHaveBeenCalled();
    });
  });

  it('shows quote details after getting quote', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<BridgeForm chains={MOCK_CHAINS} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Bridge Quote' }));
    });

    await waitFor(() => {
      expect(screen.getByText('97.50 USDC')).toBeInTheDocument();
    });
  });

  it('Bridge button calls executeBridge', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<BridgeForm chains={MOCK_CHAINS} />); });

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Get Bridge Quote' }));
    });

    await waitFor(() => screen.getByText('97.50 USDC'));

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Bridge USDC' }));
    });

    await waitFor(() => {
      expect(mockExecuteBridge).toHaveBeenCalledWith(
        expect.objectContaining({ from_chain: 'ethereum', to_chain: 'polygon', token: 'USDC' }),
      );
    });
  });
});
