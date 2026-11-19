import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getLeveragedTokens: vi.fn(),
  purchaseLt: vi.fn(),
  redeemLt: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import LeveragedTokensPage from './LeveragedTokensPage';
import { getLeveragedTokens, purchaseLt, redeemLt } from './api';
import type { LeveragedToken } from './types';

const mockGetLeveragedTokens = vi.mocked(getLeveragedTokens);
const mockPurchaseLt = vi.mocked(purchaseLt);
const mockRedeemLt = vi.mocked(redeemLt);

const MOCK_TOKENS: LeveragedToken[] = [
  {
    id: 'lt-1',
    symbol: 'BTC3L',
    underlying: 'BTC',
    leverage: '3',
    nav: '12.50',
    change_24h: '5.2',
    total_supply: '1000000',
    management_fee: '0.1',
  },
  {
    id: 'lt-2',
    symbol: 'ETH2S',
    underlying: 'ETH',
    leverage: '2',
    nav: '5.80',
    change_24h: '-2.1',
    total_supply: '500000',
    management_fee: '0.1',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetLeveragedTokens.mockResolvedValue(MOCK_TOKENS);
  mockPurchaseLt.mockResolvedValue({ tokens_received: '40' });
  mockRedeemLt.mockResolvedValue({ usdt_received: '500' });
});

describe('LeveragedTokensPage', () => {
  it('renders token list with leverage info', async () => {
    render(<LeveragedTokensPage />);

    expect(await screen.findByText('BTC3L')).toBeInTheDocument();
    expect(screen.getByText('ETH2S')).toBeInTheDocument();
    // Leverage badges
    expect(screen.getByText('3x')).toBeInTheDocument();
    expect(screen.getByText('2x')).toBeInTheDocument();
  });

  it('purchase form validates amount > 0', async () => {
    render(<LeveragedTokensPage />);

    // Click Buy on first token
    const buyBtns = await screen.findAllByRole('button', { name: 'Buy' });
    await userEvent.click(buyBtns[0]);

    // Modal opens with Amount (USDT) input
    const amountInput = await screen.findByLabelText('Amount (USDT)');
    // Leave amount empty and submit
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Purchase' }));
    });

    // HTML5 required validation prevents API call
    expect(mockPurchaseLt).not.toHaveBeenCalled();
  });

  it('purchase calls API', async () => {
    render(<LeveragedTokensPage />);

    const buyBtns = await screen.findAllByRole('button', { name: 'Buy' });
    await userEvent.click(buyBtns[0]);

    const amountInput = await screen.findByLabelText('Amount (USDT)');
    await userEvent.type(amountInput, '500');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Purchase' }));
    });

    expect(mockPurchaseLt).toHaveBeenCalledWith({ token_id: 'lt-1', usdt_amount: '500' });
  });

  it('token list displays NAV column header', async () => {
    render(<LeveragedTokensPage />);

    // Wait for tokens to load
    await screen.findByText('BTC3L');
    expect(screen.getByText('NAV (USDT)')).toBeInTheDocument();
  });

  it('token NAV values are visible in the table', async () => {
    render(<LeveragedTokensPage />);

    await screen.findByText('BTC3L');
    expect(screen.getByText('12.50')).toBeInTheDocument();
    expect(screen.getByText('5.80')).toBeInTheDocument();
  });

  it('renders Leveraged Tokens page title', async () => {
    render(<LeveragedTokensPage />);
    expect(screen.getByText('Leveraged Tokens')).toBeInTheDocument();
  });

  it('redeem calls API', async () => {
    render(<LeveragedTokensPage />);

    // Click Redeem on first token to open modal
    const redeemBtns = await screen.findAllByRole('button', { name: 'Redeem' });
    await userEvent.click(redeemBtns[0]);

    const amountInput = await screen.findByLabelText('Amount (BTC3L)');
    await userEvent.type(amountInput, '40');

    // After modal opens there are multiple "Redeem" buttons (list + modal submit).
    // The modal submit is the last one in the DOM.
    await act(async () => {
      const allRedeemBtns = screen.getAllByRole('button', { name: 'Redeem' });
      await userEvent.click(allRedeemBtns[allRedeemBtns.length - 1]);
    });

    expect(mockRedeemLt).toHaveBeenCalledWith({ token_id: 'lt-1', token_amount: '40' });
  });
});
