import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getBalances: vi.fn(),
  getWithdrawalFee: vi.fn(),
  submitWithdrawal: vi.fn(),
  getDepositAddress: vi.fn(),
  getDeposits: vi.fn(),
  getWithdrawals: vi.fn(),
  getTransfers: vi.fn(),
  internalTransfer: vi.fn(),
}));

import { WithdrawForm } from './WithdrawForm';
import { getBalances, getWithdrawalFee, submitWithdrawal } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { Balance, WithdrawalFee } from '../../types';

const mockGetBalances = vi.mocked(getBalances);
const mockGetWithdrawalFee = vi.mocked(getWithdrawalFee);
const mockSubmitWithdrawal = vi.mocked(submitWithdrawal);

const MOCK_BALANCES: Balance[] = [
  { asset: 'BTC', available: '2.5', locked: '0', total: '2.5' },
  { asset: 'ETH', available: '10.0', locked: '0', total: '10.0' },
];

const MOCK_FEE: WithdrawalFee = {
  fee: '0.0005',
  min_amount: '0.001',
  max_amount: '10',
};

// Helper to fill and submit the form so confirm dialog appears
async function fillAndSubmit(user: ReturnType<typeof userEvent.setup>) {
  await waitFor(() => { expect(mockGetBalances).toHaveBeenCalled(); });

  const addressInput = screen.getByLabelText('Withdrawal Address');
  await user.type(addressInput, 'bc1qvalidaddress');

  const amountInput = screen.getByLabelText('Amount');
  await user.type(amountInput, '1.0');

  await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));
}

beforeEach(() => {
  vi.clearAllMocks();
  mockGetBalances.mockResolvedValue(MOCK_BALANCES);
  mockGetWithdrawalFee.mockResolvedValue(MOCK_FEE);
  mockSubmitWithdrawal.mockResolvedValue({ id: 'wd-1' });
  useToastStore.setState({ toasts: [] });
});

describe('WithdrawForm', () => {
  it('renders amount input', async () => {
    await act(async () => { render(<WithdrawForm />); });

    expect(screen.getByLabelText('Amount')).toBeInTheDocument();
  });

  it('renders withdrawal address input', async () => {
    await act(async () => { render(<WithdrawForm />); });

    expect(screen.getByLabelText('Withdrawal Address')).toBeInTheDocument();
  });

  it('renders asset and network selectors', async () => {
    await act(async () => { render(<WithdrawForm />); });

    expect(screen.getByLabelText('Asset')).toBeInTheDocument();
    expect(screen.getByLabelText('Network')).toBeInTheDocument();
  });

  it('Max button fills amount with available balance', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    const maxBtn = screen.getByRole('button', { name: 'Max' });
    await user.click(maxBtn);

    const amountInput = screen.getByLabelText('Amount') as HTMLInputElement;
    expect(amountInput.value).toBe('2.5');
  });

  it('shows available balance in the form', async () => {
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      expect(screen.getByText('Available')).toBeInTheDocument();
    });

    expect(screen.getByText('2.5 BTC')).toBeInTheDocument();
  });

  it('validates that amount must be greater than 0', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    // Fill address, leave amount empty
    const addressInput = screen.getByLabelText('Withdrawal Address');
    await user.type(addressInput, 'bc1qvalidaddress');

    const withdrawBtn = screen.getByRole('button', { name: /Withdraw BTC/i });
    await user.click(withdrawBtn);

    // Validation error should appear — no confirm dialog title in DOM as button
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows validation error when amount is empty', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    const addressInput = screen.getByLabelText('Withdrawal Address');
    await user.type(addressInput, 'bc1qvalidaddress');

    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    await waitFor(() => {
      expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
    });
  });

  it('opens confirm dialog on valid submit', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await fillAndSubmit(user);

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });
  });

  it('calls submitWithdrawal API after confirming', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await fillAndSubmit(user);

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });

    // Click confirm button - it's inside the dialog and its name matches the confirmText prop
    const confirmBtn = screen.getByRole('button', { name: 'Confirm Withdrawal' });
    await act(async () => {
      await user.click(confirmBtn);
    });

    await waitFor(() => {
      expect(mockSubmitWithdrawal).toHaveBeenCalledWith(
        expect.objectContaining({
          asset: 'BTC',
          address: 'bc1qvalidaddress',
          amount: '1.0',
        }),
      );
    });
  });

  it('shows success toast after successful withdrawal', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await fillAndSubmit(user);
    await waitFor(() => { expect(screen.getByRole('dialog')).toBeInTheDocument(); });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Withdrawal' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'success')).toBe(true);
    });
  });

  it('cancelling confirm dialog does not submit', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await fillAndSubmit(user);
    await waitFor(() => { expect(screen.getByRole('dialog')).toBeInTheDocument(); });

    // Click Cancel in the dialog — the ghost button inside ConfirmDialog
    await user.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(mockSubmitWithdrawal).not.toHaveBeenCalled();
  });

  // === Fee Display ===

  it('shows withdrawal fee for selected network', async () => {
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      // Default asset=BTC, network=Bitcoin → fee = '0.0005' from mock
      expect(screen.getByText('0.0005 BTC')).toBeInTheDocument();
    });
  });

  it('changing network triggers a new fee request', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    // Change asset to ETH so we have multiple networks
    const assetSelect = screen.getByLabelText('Asset') as HTMLSelectElement;
    await user.selectOptions(assetSelect, 'ETH');

    const networkSelect = screen.getByLabelText('Network') as HTMLSelectElement;
    // ETH networks: ERC20, Arbitrum, Optimism — switch to Arbitrum
    await user.selectOptions(networkSelect, 'Arbitrum');

    // getWithdrawalFee should be called again for the new combination
    await waitFor(() => {
      expect(mockGetWithdrawalFee).toHaveBeenCalledWith('ETH', 'Arbitrum');
    });
  });

  it('fee display updates after network change — API called for new combination', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    // Wait for initial fee load
    await waitFor(() => {
      expect(mockGetWithdrawalFee).toHaveBeenCalledWith('BTC', 'Bitcoin');
    });

    // Change asset to USDT — should trigger fee re-fetch for USDT/ERC20
    const assetSelect = screen.getByLabelText('Asset') as HTMLSelectElement;
    await user.selectOptions(assetSelect, 'USDT');

    await waitFor(() => {
      expect(mockGetWithdrawalFee).toHaveBeenCalledWith('USDT', 'ERC20');
    });

    // The fee display should show the value from the most recent call
    // (mock returns fee='0.0005' for all calls in beforeEach)
    await waitFor(() => {
      expect(screen.getByText('0.0005 USDT')).toBeInTheDocument();
    });
  });

  // === Amount Precision ===

  it('handles decimal amounts correctly without rounding', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    const amountInput = screen.getByLabelText('Amount') as HTMLInputElement;
    await user.type(amountInput, '0.12345678');

    expect(amountInput.value).toBe('0.12345678');
  });

  it('Max button fills exactly the available balance, not more', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    await user.click(screen.getByRole('button', { name: 'Max' }));

    const amountInput = screen.getByLabelText('Amount') as HTMLInputElement;
    // Available BTC balance is '2.5' — Max should set exactly that string
    expect(amountInput.value).toBe('2.5');
  });

  it('shows validation error when amount exceeds available balance', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    const addressInput = screen.getByLabelText('Withdrawal Address');
    await user.type(addressInput, 'bc1qvalidaddress');

    const amountInput = screen.getByLabelText('Amount');
    // maxBalance for BTC is 2.5 — enter more
    await user.type(amountInput, '3.0');

    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    await waitFor(() => {
      expect(screen.getByText('Exceeds available balance')).toBeInTheDocument();
    });
    expect(mockSubmitWithdrawal).not.toHaveBeenCalled();
  });

  // === Address Validation ===

  it('shows error for empty address on submit', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    // Fill amount but no address
    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '1.0');

    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    await waitFor(() => {
      expect(screen.getByText('Address is required')).toBeInTheDocument();
    });
    expect(mockSubmitWithdrawal).not.toHaveBeenCalled();
  });

  // === Confirm Dialog Details ===

  it('confirm dialog shows the correct amount and destination address', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    await waitFor(() => {
      expect(mockGetBalances).toHaveBeenCalled();
    });

    const addressInput = screen.getByLabelText('Withdrawal Address');
    await user.type(addressInput, 'bc1qtargetaddress');

    const amountInput = screen.getByLabelText('Amount');
    await user.type(amountInput, '0.5');

    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeInTheDocument();
    });

    // The ConfirmDialog message prop is: `Send ${amount} ${asset} to ${address}?`
    expect(screen.getByText(/Send 0\.5 BTC to bc1qtargetaddress/i)).toBeInTheDocument();
  });

  // === Network Switching ===

  it('switching asset resets network to first network for new asset', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WithdrawForm />); });

    // Start with BTC (network: Bitcoin), switch to USDT (networks: ERC20, TRC20, BEP20)
    const assetSelect = screen.getByLabelText('Asset') as HTMLSelectElement;
    await user.selectOptions(assetSelect, 'USDT');

    const networkSelect = screen.getByLabelText('Network') as HTMLSelectElement;
    // After asset change, network should be ERC20 (first in USDT networks)
    await waitFor(() => {
      expect(networkSelect.value).toBe('ERC20');
    });
  });
});
