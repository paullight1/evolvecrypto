import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getSubAccounts: vi.fn(),
  createSubAccount: vi.fn(),
  getSubAccountBalances: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import SubAccountsPage from './SubAccountsPage';
import { getSubAccounts, createSubAccount, getSubAccountBalances } from './api';
import type { SubAccount, SubAccountBalance } from './types';

const mockGetSubAccounts = vi.mocked(getSubAccounts);
const mockCreateSubAccount = vi.mocked(createSubAccount);
const mockGetSubAccountBalances = vi.mocked(getSubAccountBalances);

const MOCK_SUB_ACCOUNTS: SubAccount[] = [
  { id: 'sub-1', label: 'Trading Bot 1', email: 'bot1@example.com', status: 'active', created_at: 1700000000 },
  { id: 'sub-2', label: 'Hedging Account', email: 'hedge@example.com', status: 'active', created_at: 1700100000 },
];

const MOCK_BALANCES: SubAccountBalance[] = [
  { asset: 'USDT', available: '5000.00', locked: '200.00', total: '5200.00' },
  { asset: 'BTC', available: '0.5', locked: '0.1', total: '0.6' },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetSubAccounts.mockResolvedValue([]);
  mockCreateSubAccount.mockResolvedValue({
    id: 'new-sub', label: 'New Bot', email: 'new@example.com', status: 'active', created_at: Date.now() / 1000,
  });
  mockGetSubAccountBalances.mockResolvedValue([]);
});

describe('SubAccountsPage', () => {
  it('renders create sub-account form', () => {
    render(<SubAccountsPage />);
    expect(screen.getByText('Create Sub-Account')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Trading Bot 1')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('sub@example.com')).toBeInTheDocument();
  });

  it('Create button calls createSubAccount API', async () => {
    mockGetSubAccounts.mockResolvedValue([]);
    render(<SubAccountsPage />);

    await userEvent.type(screen.getByPlaceholderText('Trading Bot 1'), 'New Bot');
    await userEvent.type(screen.getByPlaceholderText('sub@example.com'), 'new@example.com');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Create' }));
    });

    expect(mockCreateSubAccount).toHaveBeenCalledWith('New Bot', 'new@example.com');
  });

  it('Create button does nothing when fields are empty', async () => {
    render(<SubAccountsPage />);

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Create' }));
    });

    expect(mockCreateSubAccount).not.toHaveBeenCalled();
  });

  it('shows sub-account list after load', async () => {
    mockGetSubAccounts.mockResolvedValue(MOCK_SUB_ACCOUNTS);
    render(<SubAccountsPage />);

    expect(await screen.findByText('Trading Bot 1')).toBeInTheDocument();
    expect(screen.getByText('Hedging Account')).toBeInTheDocument();
  });

  it('clicking sub-account row loads its balances', async () => {
    mockGetSubAccounts.mockResolvedValue(MOCK_SUB_ACCOUNTS);
    mockGetSubAccountBalances.mockResolvedValue(MOCK_BALANCES);
    render(<SubAccountsPage />);

    await screen.findByText('Trading Bot 1');
    await userEvent.click(screen.getByText('Trading Bot 1'));

    await waitFor(() => {
      expect(mockGetSubAccountBalances).toHaveBeenCalledWith('sub-1');
    });
    expect(await screen.findByText('5000.00')).toBeInTheDocument();
  });
});
