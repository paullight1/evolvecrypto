import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getBalances: vi.fn(),
  getDepositAddress: vi.fn(),
  submitWithdrawal: vi.fn(),
  getDeposits: vi.fn(),
  getWithdrawals: vi.fn(),
  getTransfers: vi.fn(),
  internalTransfer: vi.fn(),
  getWithdrawalFee: vi.fn(),
}));

import WalletPage from './WalletPage';
import {
  getBalances,
  getDepositAddress,
  submitWithdrawal,
  getDeposits,
  getWithdrawals,
  getWithdrawalFee,
} from './api';
import type { Balance, DepositRecord, WithdrawalRecord, DepositAddress } from './types';

const mockGetBalances = vi.mocked(getBalances);
const mockGetDepositAddress = vi.mocked(getDepositAddress);
const mockSubmitWithdrawal = vi.mocked(submitWithdrawal);
const mockGetDeposits = vi.mocked(getDeposits);
const mockGetWithdrawals = vi.mocked(getWithdrawals);
const mockGetWithdrawalFee = vi.mocked(getWithdrawalFee);

const MOCK_BALANCES: Balance[] = [
  { asset: 'BTC', available: '1.5', locked: '0.1', total: '1.6' },
  { asset: 'USDT', available: '10000', locked: '0', total: '10000' },
];

const MOCK_ADDRESS: DepositAddress = {
  address: 'bc1qtest12345678',
  network: 'Bitcoin',
};

const MOCK_DEPOSITS: DepositRecord[] = [
  {
    id: 'dep-1',
    asset: 'BTC',
    network: 'Bitcoin',
    amount: '0.5',
    address: 'bc1qsource',
    tx_hash: 'abc123',
    status: 'completed',
    created_at: 1700000000,
  },
];

const MOCK_WITHDRAWALS: WithdrawalRecord[] = [
  {
    id: 'wd-1',
    asset: 'USDT',
    network: 'Ethereum',
    address: '0xdest',
    amount: '500',
    fee: '1',
    status: 'pending',
    created_at: 1700001000,
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetBalances.mockResolvedValue(MOCK_BALANCES);
  mockGetDepositAddress.mockResolvedValue(MOCK_ADDRESS);
  mockGetDeposits.mockResolvedValue(MOCK_DEPOSITS);
  mockGetWithdrawals.mockResolvedValue(MOCK_WITHDRAWALS);
  mockGetWithdrawalFee.mockResolvedValue({ fee: '0.0005', min_amount: '0.001', max_amount: '10' });
  mockSubmitWithdrawal.mockResolvedValue({ id: 'wd-new' });
});

describe('WalletPage', () => {
  it('renders title and tabs', async () => {
    await act(async () => { render(<WalletPage />); });
    expect(screen.getByText('Wallet')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Overview' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Deposit' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Withdraw' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'History' })).toBeInTheDocument();
  });

  it('renders balance table with asset data on overview tab', async () => {
    await act(async () => { render(<WalletPage />); });
    expect(await screen.findByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('USDT')).toBeInTheDocument();
    expect(screen.getByText('1.6')).toBeInTheDocument();
    // 10000 appears in both "Total" and "Available" columns for USDT
    expect(screen.getAllByText('10000').length).toBeGreaterThanOrEqual(1);
  });

  it('switching to Deposit tab shows deposit address', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'Deposit' }));
    expect(await screen.findByText('Deposit Address')).toBeInTheDocument();
    expect(screen.getByText('bc1qtest12345678')).toBeInTheDocument();
  });

  it('copying address calls clipboard API', async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    // jsdom does not allow Object.assign on navigator.clipboard — use defineProperty
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
      writable: true,
    });

    await act(async () => { render(<WalletPage />); });
    await user.click(screen.getByRole('tab', { name: 'Deposit' }));
    await screen.findByText('bc1qtest12345678');

    const copyBtn = screen.getByRole('button', { name: /copy to clipboard/i });
    await user.click(copyBtn);

    expect(writeText).toHaveBeenCalledWith('bc1qtest12345678');
  });

  it('withdrawal form shows error when amount is zero', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'Withdraw' }));
    await screen.findByText(/Withdrawal Address/i);

    // Provide address but no amount
    const addressInput = screen.getByPlaceholderText('Enter destination address');
    await user.type(addressInput, 'bc1qdestination');

    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
  });

  it('withdrawal form validates missing address', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'Withdraw' }));
    await screen.findByText(/Withdrawal Address/i);

    const amountInput = screen.getByPlaceholderText('0.00');
    await user.type(amountInput, '0.1');

    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    expect(screen.getByText('Address is required')).toBeInTheDocument();
  });

  it('Max button fills the amount field with available balance', async () => {
    mockGetBalances.mockResolvedValue([
      { asset: 'BTC', available: '1.5', locked: '0', total: '1.5' },
    ]);

    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'Withdraw' }));
    await screen.findByText(/Withdrawal Address/i);

    await user.click(screen.getByRole('button', { name: 'Max' }));

    const amountInput = screen.getByPlaceholderText('0.00') as HTMLInputElement;
    expect(amountInput.value).toBe('1.5');
  });

  it('confirming withdrawal calls submitWithdrawal with correct params', async () => {
    mockGetBalances.mockResolvedValue([
      { asset: 'BTC', available: '1.5', locked: '0', total: '1.5' },
    ]);

    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'Withdraw' }));
    await screen.findByText(/Withdrawal Address/i);

    await user.type(screen.getByPlaceholderText('Enter destination address'), 'bc1qdestination');
    await user.type(screen.getByPlaceholderText('0.00'), '0.5');
    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    // Confirm dialog appears; click confirm
    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Withdrawal' }));
    });

    expect(mockSubmitWithdrawal).toHaveBeenCalledWith(
      expect.objectContaining({
        asset: 'BTC',
        amount: '0.5',
        address: 'bc1qdestination',
      }),
    );
  });

  it('shows error when withdrawal API fails via toast store', async () => {
    mockGetBalances.mockResolvedValue([
      { asset: 'BTC', available: '1.5', locked: '0', total: '1.5' },
    ]);
    mockSubmitWithdrawal.mockRejectedValueOnce(new Error('Insufficient funds'));

    const { useToastStore } = await import('@shared/ui/Toast/store');

    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'Withdraw' }));
    await screen.findByText(/Withdrawal Address/i);

    await user.type(screen.getByPlaceholderText('Enter destination address'), 'bc1qdestination');
    await user.type(screen.getByPlaceholderText('0.00'), '0.5');
    await user.click(screen.getByRole('button', { name: /Withdraw BTC/i }));

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Withdrawal' }));
    });

    // Error goes to the toast store — verify it was pushed
    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.type === 'error' && t.message === 'Insufficient funds')).toBe(true);
  });

  it('history tab shows combined deposit and withdrawal records', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<WalletPage />); });

    await user.click(screen.getByRole('tab', { name: 'History' }));

    const depositTexts = await screen.findAllByText('Deposit');
    expect(depositTexts.length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('Withdrawal')).toBeInTheDocument();
    expect(screen.getByText('0.5')).toBeInTheDocument();
    expect(screen.getByText('500')).toBeInTheDocument();
  });
});
