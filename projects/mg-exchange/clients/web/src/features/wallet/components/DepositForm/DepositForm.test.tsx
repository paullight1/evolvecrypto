import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getDepositAddress: vi.fn(),
  getDeposits: vi.fn(),
  getBalances: vi.fn(),
  getWithdrawalFee: vi.fn(),
  submitWithdrawal: vi.fn(),
  getWithdrawals: vi.fn(),
  getTransfers: vi.fn(),
  internalTransfer: vi.fn(),
}));

import { DepositForm } from './DepositForm';
import { getDepositAddress, getDeposits } from '../../api';
import type { DepositAddress, Transaction } from '../../types';

const mockGetDepositAddress = vi.mocked(getDepositAddress);
const mockGetDeposits = vi.mocked(getDeposits);

const MOCK_ADDRESS: DepositAddress = {
  address: 'bc1qexampleaddress123456789',
  network: 'Bitcoin',
  min_deposit: '0.001',
};

const MOCK_DEPOSITS: Transaction[] = [
  {
    id: 'tx-1',
    type: 'deposit',
    asset: 'BTC',
    amount: '0.5',
    status: 'completed',
    created_at: 1700000000,
    tx_hash: 'abcdef1234567890',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetDepositAddress.mockResolvedValue(MOCK_ADDRESS);
  mockGetDeposits.mockResolvedValue(MOCK_DEPOSITS);
});

describe('DepositForm', () => {
  it('renders asset selector', async () => {
    await act(async () => { render(<DepositForm />); });

    expect(screen.getByLabelText('Asset')).toBeInTheDocument();
  });

  it('renders network selector', async () => {
    await act(async () => { render(<DepositForm />); });

    expect(screen.getByLabelText('Network')).toBeInTheDocument();
  });

  it('loads deposit address on mount', async () => {
    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(mockGetDepositAddress).toHaveBeenCalled();
    });
  });

  it('shows deposit address after loading', async () => {
    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(screen.getByText(MOCK_ADDRESS.address)).toBeInTheDocument();
    });
  });

  it('shows "Deposit Address" label when address loaded', async () => {
    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(screen.getByText('Deposit Address')).toBeInTheDocument();
    });
  });

  it('shows copy button alongside the address', async () => {
    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(screen.getByText(MOCK_ADDRESS.address)).toBeInTheDocument();
    });

    // CopyableText renders a copy-to-clipboard button
    expect(screen.getByRole('button', { name: /Copy to clipboard/i })).toBeInTheDocument();
  });

  it('copy button triggers clipboard API', async () => {
    const user = userEvent.setup();
    const writeTextMock = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText: writeTextMock },
      writable: true,
      configurable: true,
    });

    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(screen.getByText(MOCK_ADDRESS.address)).toBeInTheDocument();
    });

    const copyBtn = screen.getByRole('button', { name: /Copy to clipboard/i });
    await user.click(copyBtn);

    expect(writeTextMock).toHaveBeenCalledWith(MOCK_ADDRESS.address);
  });

  it('loading a different asset calls getDepositAddress with new asset', async () => {
    const user = userEvent.setup();
    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(mockGetDepositAddress).toHaveBeenCalled();
    });

    // Change asset from BTC to ETH
    const assetSelect = screen.getByLabelText('Asset');
    await user.selectOptions(assetSelect, 'ETH');

    await waitFor(() => {
      expect(mockGetDepositAddress).toHaveBeenCalledWith('ETH', expect.any(String));
    });
  });

  it('shows error message when deposit address cannot be loaded', async () => {
    mockGetDepositAddress.mockRejectedValue(new Error('Network error'));

    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(screen.getByText('Could not load deposit address.')).toBeInTheDocument();
    });
  });

  it('shows "Recent Deposits" section', async () => {
    await act(async () => { render(<DepositForm />); });

    expect(screen.getByText('Recent Deposits')).toBeInTheDocument();
  });

  it('shows "No deposits yet" when history is empty', async () => {
    mockGetDeposits.mockResolvedValue([]);

    await act(async () => { render(<DepositForm />); });

    await waitFor(() => {
      expect(screen.getByText('No deposits yet')).toBeInTheDocument();
    });
  });
});
