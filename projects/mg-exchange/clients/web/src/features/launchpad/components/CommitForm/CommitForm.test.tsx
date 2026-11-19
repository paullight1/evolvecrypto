import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  commitToProject: vi.fn(),
}));

import { CommitForm } from './CommitForm';
import { commitToProject } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { LaunchpadProject } from '../../types';

const mockCommitToProject = vi.mocked(commitToProject);

const MOCK_PROJECT: LaunchpadProject = {
  id: 'proj-1',
  token_name: 'ExampleToken',
  token_symbol: 'EXT',
  description: 'A new DeFi protocol.',
  total_raise_usdt: '1000000',
  committed_usdt: '500000',
  token_price_usdt: '0.10',
  min_commitment_usdt: '100',
  max_commitment_usdt: '10000',
  status: 'active',
  start_time: new Date(Date.now() - 86_400_000).toISOString(),
  end_time: new Date(Date.now() + 7 * 86_400_000).toISOString(),
  claim_date: null,
  token_distribution: '50% public',
  eligibility_note: 'KYC required',
  website_url: 'https://example.com',
};

beforeEach(() => {
  vi.clearAllMocks();
  mockCommitToProject.mockResolvedValue(undefined as never);
  useToastStore.setState({ toasts: [] });
});

describe('CommitForm', () => {
  it('renders nothing when project is null', async () => {
    await act(async () => {
      render(<CommitForm project={null} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('renders modal with token price info when project is provided', async () => {
    await act(async () => {
      render(<CommitForm project={MOCK_PROJECT} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('$0.10')).toBeInTheDocument();
  });

  it('shows estimated tokens when amount is entered', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<CommitForm project={MOCK_PROJECT} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    await user.type(screen.getByLabelText('Amount (USDT)'), '100');

    await waitFor(() => {
      // 100 / 0.10 = 1000 EXT
      expect(screen.getByText(/1000\.0000 EXT/)).toBeInTheDocument();
    });
  });

  it('shows error toast for invalid amount (zero)', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<CommitForm project={MOCK_PROJECT} onClose={vi.fn()} onSuccess={vi.fn()} />);
    });

    // Type 0 — parseFloat('0') is 0 which is not > 0
    await user.type(screen.getByLabelText('Amount (USDT)'), '0');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Commit' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });

    expect(mockCommitToProject).not.toHaveBeenCalled();
  });

  it('calls commitToProject and onSuccess on valid submit', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();
    await act(async () => {
      render(<CommitForm project={MOCK_PROJECT} onClose={vi.fn()} onSuccess={onSuccess} />);
    });

    await user.type(screen.getByLabelText('Amount (USDT)'), '500');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Commit' }));
    });

    await waitFor(() => {
      expect(mockCommitToProject).toHaveBeenCalledWith(
        expect.objectContaining({ project_id: 'proj-1', amount_usdt: '500' }),
      );
      expect(onSuccess).toHaveBeenCalled();
    });
  });
});
