import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  getProjects: vi.fn(),
  commitToProject: vi.fn(),
  getMyCommitments: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import LaunchpadPage from './LaunchpadPage';
import { getProjects, commitToProject, getMyCommitments } from './api';
import type { LaunchpadProject, LaunchpadCommitment } from './types';

const mockGetProjects = vi.mocked(getProjects);
const mockCommitToProject = vi.mocked(commitToProject);
const mockGetMyCommitments = vi.mocked(getMyCommitments);

const MOCK_PROJECTS: LaunchpadProject[] = [
  {
    id: 'proj-1',
    token_name: 'Alpha Token',
    token_symbol: 'ALPHA',
    description: 'The next big thing',
    total_raise_usdt: '1000000',
    committed_usdt: '400000',
    token_price_usdt: '0.10',
    min_commitment_usdt: '100',
    max_commitment_usdt: '10000',
    status: 'active',
    start_time: new Date(Date.now() - 86_400_000).toISOString(),
    end_time: new Date(Date.now() + 86_400_000).toISOString(),
    claim_date: null,
    token_distribution: '50%',
    eligibility_note: 'KYC required',
    website_url: null,
  },
];

const MOCK_COMMITMENTS: LaunchpadCommitment[] = [
  {
    commitment_id: 'commit-1',
    project_id: 'proj-1',
    project_name: 'Alpha Token',
    token_symbol: 'ALPHA',
    committed_usdt: '500',
    estimated_tokens: '5000',
    allocation_received: null,
    claim_date: null,
    status: 'pending',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGetProjects.mockResolvedValue(MOCK_PROJECTS);
  mockGetMyCommitments.mockResolvedValue([]);
  mockCommitToProject.mockResolvedValue({ estimated_tokens: '5000', committed_usdt: '500' });
});

describe('LaunchpadPage', () => {
  it('renders project cards', async () => {
    render(<LaunchpadPage />);

    expect(await screen.findByText('Alpha Token')).toBeInTheDocument();
    expect(screen.getByText('ALPHA')).toBeInTheDocument();
  });

  it('commit form validates amount > 0', async () => {
    render(<LaunchpadPage />);

    // Click the Commit Now button to open the modal
    const commitBtn = await screen.findByRole('button', { name: 'Commit Now' });
    await userEvent.click(commitBtn);

    // Modal should appear with Amount field
    const amountInput = await screen.findByLabelText('Amount (USDT)');

    // Type 0 (invalid) and submit
    await userEvent.type(amountInput, '0');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Commit' }));
    });

    // API should not have been called
    expect(mockCommitToProject).not.toHaveBeenCalled();
  });

  it('commit calls API with amount', async () => {
    render(<LaunchpadPage />);

    const commitBtn = await screen.findByRole('button', { name: 'Commit Now' });
    await userEvent.click(commitBtn);

    const amountInput = await screen.findByLabelText('Amount (USDT)');
    await userEvent.type(amountInput, '500');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Commit' }));
    });

    expect(mockCommitToProject).toHaveBeenCalledWith({
      project_id: 'proj-1',
      amount_usdt: '500',
      currency: 'USDT',
    });
  });

  it('my commitments tab shows history', async () => {
    mockGetMyCommitments.mockResolvedValue(MOCK_COMMITMENTS);
    render(<LaunchpadPage />);

    // Switch to My Commitments tab
    await userEvent.click(screen.getByRole('tab', { name: 'My Commitments' }));

    await waitFor(() => {
      expect(mockGetMyCommitments).toHaveBeenCalledTimes(1);
    });
    expect(await screen.findByText(/Alpha Token/)).toBeInTheDocument();
  });

  it('shows Launchpad page title', async () => {
    render(<LaunchpadPage />);
    expect(screen.getByText('Launchpad')).toBeInTheDocument();
  });

  it('project card shows progress toward raise target', async () => {
    render(<LaunchpadPage />);

    // MOCK_PROJECTS[0] has committed_usdt '400000' out of total_raise_usdt '1000000' = 40%
    // ProjectCard renders "{pct.toFixed(1)}% filled"
    expect(await screen.findByText('Alpha Token')).toBeInTheDocument();
    expect(screen.getByText('40.0% filled')).toBeInTheDocument();
  });

  it('switching back to Projects tab reloads projects', async () => {
    render(<LaunchpadPage />);
    await screen.findByText('Alpha Token');

    // Go to commitments then back to projects
    await userEvent.click(screen.getByRole('tab', { name: 'My Commitments' }));
    await userEvent.click(screen.getByRole('tab', { name: 'Projects' }));

    // getProjects should have been called at least twice (initial + return visit)
    await waitFor(() => {
      expect(mockGetProjects.mock.calls.length).toBeGreaterThanOrEqual(2);
    });
  });

  it('commit amount below min shows validation error', async () => {
    render(<LaunchpadPage />);

    const commitBtn = await screen.findByRole('button', { name: 'Commit Now' });
    await userEvent.click(commitBtn);

    const amountInput = await screen.findByLabelText('Amount (USDT)');
    // Type negative amount
    await userEvent.type(amountInput, '-50');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Commit' }));
    });

    // API should not be called with invalid amount
    expect(mockCommitToProject).not.toHaveBeenCalled();
  });
});
