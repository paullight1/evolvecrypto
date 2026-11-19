import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getDexChains: vi.fn(),
  getDexQuote: vi.fn(),
  executeDexSwap: vi.fn(),
  getBridgeRoutes: vi.fn(),
  getBridgeQuote: vi.fn(),
  executeBridge: vi.fn(),
  getSmartRoute: vi.fn(),
  createWeb3Wallet: vi.fn(),
  getWeb3Balances: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import Web3Page from './Web3Page';
import { getDexChains, getDexQuote, executeDexSwap, getBridgeRoutes, getBridgeQuote, getWeb3Balances } from './api';
import type { Chain, DexQuote, BridgeRoute, BridgeQuote, TokenBalance } from './types';

const mockGetDexChains = vi.mocked(getDexChains);
const mockGetDexQuote = vi.mocked(getDexQuote);
const mockExecuteDexSwap = vi.mocked(executeDexSwap);
const mockGetBridgeRoutes = vi.mocked(getBridgeRoutes);
const mockGetBridgeQuote = vi.mocked(getBridgeQuote);
const mockGetWeb3Balances = vi.mocked(getWeb3Balances);

const MOCK_CHAINS: Chain[] = [
  { id: 'eth', name: 'Ethereum', native_token: 'ETH', rpc_url: 'https://rpc.eth', explorer_url: 'https://etherscan.io' },
  { id: 'bsc', name: 'BNB Chain', native_token: 'BNB', rpc_url: 'https://rpc.bsc', explorer_url: 'https://bscscan.com' },
];

const MOCK_QUOTE: DexQuote = {
  from_token: 'ETH',
  to_token: 'USDC',
  from_amount: '1',
  to_amount: '3480.00',
  price_impact: '0.05',
  fee: '0.003 ETH',
  route: [],
  expires_at: Date.now() / 1000 + 60,
};

const MOCK_BRIDGE_ROUTES: BridgeRoute[] = [
  { bridge_name: 'Stargate', from_chain: 'eth', to_chain: 'bsc', token: 'USDC', estimated_time_seconds: 300, fee: '1.00' },
];

const MOCK_BRIDGE_QUOTE: BridgeQuote = {
  route: MOCK_BRIDGE_ROUTES[0],
  from_amount: '100',
  to_amount: '98.5',
  fee_usd: '1.50',
  expires_at: Date.now() / 1000 + 120,
};

const MOCK_BALANCES: TokenBalance[] = [
  { token: '0xabc', symbol: 'ETH', balance: '1.5', usd_value: '5220.00', chain: 'eth' },
  { token: '0xdef', symbol: 'USDC', balance: '1000', usd_value: '1000.00', chain: 'bsc' },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetDexChains.mockResolvedValue(MOCK_CHAINS);
  mockGetDexQuote.mockResolvedValue(MOCK_QUOTE);
  mockExecuteDexSwap.mockResolvedValue({ tx_hash: '0xabc123def456' });
  mockGetBridgeRoutes.mockResolvedValue(MOCK_BRIDGE_ROUTES);
  mockGetBridgeQuote.mockResolvedValue(MOCK_BRIDGE_QUOTE);
  mockGetWeb3Balances.mockResolvedValue(MOCK_BALANCES);
});

describe('Web3Page', () => {
  it('swap form validates amount > 0', async () => {
    render(<Web3Page />);

    // Get Quote button should be visible immediately on Swap tab
    const getQuoteBtn = await screen.findByRole('button', { name: 'Get Quote' });

    // Click without entering amount — no API call expected
    await act(async () => {
      await userEvent.click(getQuoteBtn);
    });

    expect(mockGetDexQuote).not.toHaveBeenCalled();
  });

  it('get quote calls getDexQuote API', async () => {
    render(<Web3Page />);

    // Wait for chains to load (the select will have options once chains resolve)
    await waitFor(() => {
      // Chain select should have options once getDexChains resolves
      const chainSelect = screen.getByLabelText('Chain');
      expect(chainSelect).not.toBeDisabled();
    });

    // Manually select the first chain to ensure chain state is set
    const chainSelect = screen.getByLabelText('Chain');
    await userEvent.selectOptions(chainSelect, 'eth');

    const amountInput = screen.getByLabelText('Amount');
    await userEvent.type(amountInput, '1');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    expect(mockGetDexQuote).toHaveBeenCalledWith({
      chain: 'eth',
      from_token: 'ETH',
      to_token: 'USDC',
      amount: '1',
    });
    expect(await screen.findByText(/3480\.00 USDC/)).toBeInTheDocument();
  });

  it('bridge form shows route options', async () => {
    render(<Web3Page />);

    await userEvent.click(screen.getByRole('tab', { name: 'Bridge' }));

    const amountInput = await screen.findByLabelText('Amount');
    await userEvent.type(amountInput, '100');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Get Bridge Quote' }));
    });

    expect(mockGetBridgeRoutes).toHaveBeenCalled();
    // Bridge route option appears
    expect(await screen.findByText(/Stargate/)).toBeInTheDocument();
  });

  it('wallet tab shows multi-chain balances', async () => {
    render(<Web3Page />);

    await act(async () => {
      await userEvent.click(screen.getByRole('tab', { name: 'Wallet' }));
    });

    expect(mockGetWeb3Balances).toHaveBeenCalledTimes(1);
    expect(await screen.findByText('ETH')).toBeInTheDocument();
    expect(screen.getByText('USDC')).toBeInTheDocument();
    expect(screen.getByText('eth')).toBeInTheDocument();
    expect(screen.getByText('bsc')).toBeInTheDocument();
  });

  it('execute swap calls API', async () => {
    render(<Web3Page />);

    // Wait for chains, then select chain
    await waitFor(() => {
      expect(screen.getByLabelText('Chain')).not.toBeDisabled();
    });
    const chainSelect = screen.getByLabelText('Chain');
    await userEvent.selectOptions(chainSelect, 'eth');

    const amountInput = screen.getByLabelText('Amount');
    await userEvent.type(amountInput, '1');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Get Quote' }));
    });

    await screen.findByText(/3480\.00 USDC/);

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Execute Swap' }));
    });

    expect(mockExecuteDexSwap).toHaveBeenCalledWith({
      chain: 'eth',
      from_token: 'ETH',
      to_token: 'USDC',
      amount: '1',
      slippage: '0.5',
    });
  });
});
