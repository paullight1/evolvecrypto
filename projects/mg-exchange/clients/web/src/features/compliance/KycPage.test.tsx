import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getKycStatus: vi.fn(),
  startKyc: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import KycPage from './KycPage';
import { getKycStatus, startKyc } from './api';
import type { KycStatus } from './types';

const mockGetKycStatus = vi.mocked(getKycStatus);
const mockStartKyc = vi.mocked(startKyc);

const STATUS_NONE: KycStatus = { level: 0, status: 'none' };
const STATUS_PENDING: KycStatus = { level: 1, status: 'pending', submitted_at: 1700000000 };
const STATUS_APPROVED: KycStatus = { level: 2, status: 'approved', submitted_at: 1700000000, reviewed_at: 1700100000 };
const STATUS_REJECTED: KycStatus = { level: 1, status: 'rejected', submitted_at: 1700000000, rejection_reason: 'ID document expired' };

beforeEach(() => {
  vi.clearAllMocks();
  mockGetKycStatus.mockResolvedValue(STATUS_NONE);
  mockStartKyc.mockResolvedValue({ message: 'ok' });
});

describe('KycPage', () => {
  it('shows "Not Verified" badge when status is none', async () => {
    mockGetKycStatus.mockResolvedValue(STATUS_NONE);
    render(<KycPage />);

    await waitFor(() => {
      expect(screen.getByText('NONE')).toBeInTheDocument();
    });
  });

  it('start KYC button calls API', async () => {
    // After startKyc resolves, getKycStatus is called again — return pending
    mockGetKycStatus
      .mockResolvedValueOnce(STATUS_NONE)
      .mockResolvedValueOnce(STATUS_PENDING);
    mockStartKyc.mockResolvedValue({ message: 'submitted' });
    render(<KycPage />);

    const startBtn = await screen.findByRole('button', { name: 'Start KYC Level 1' });
    await act(async () => {
      await userEvent.click(startBtn);
    });

    expect(mockStartKyc).toHaveBeenCalledWith(1);
  });

  it('shows "Pending" badge when submitted', async () => {
    mockGetKycStatus.mockResolvedValue(STATUS_PENDING);
    render(<KycPage />);

    await waitFor(() => {
      expect(screen.getByText('PENDING')).toBeInTheDocument();
    });
    // Should not show start buttons when pending
    expect(screen.queryByRole('button', { name: 'Start KYC Level 1' })).not.toBeInTheDocument();
  });

  it('shows "Approved" badge when approved', async () => {
    mockGetKycStatus.mockResolvedValue(STATUS_APPROVED);
    render(<KycPage />);

    await waitFor(() => {
      expect(screen.getByText('APPROVED')).toBeInTheDocument();
    });
    // Steps component shows "Full access" for all states — badge is the real check
  });

  it('shows rejection reason when rejected', async () => {
    mockGetKycStatus.mockResolvedValue(STATUS_REJECTED);
    render(<KycPage />);

    await waitFor(() => {
      expect(screen.getByText('REJECTED')).toBeInTheDocument();
    });
    expect(screen.getByText(/ID document expired/)).toBeInTheDocument();
  });
});
