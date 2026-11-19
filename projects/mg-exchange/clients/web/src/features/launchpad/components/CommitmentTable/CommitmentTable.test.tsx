import { render, screen, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { CommitmentTable } from './CommitmentTable';
import type { LaunchpadCommitment } from '../../types';

const MOCK_COMMITMENTS: LaunchpadCommitment[] = [
  {
    commitment_id: 'c-1',
    project_id: 'proj-1',
    project_name: 'ExampleToken',
    token_symbol: 'EXT',
    committed_usdt: '500',
    estimated_tokens: '5000',
    allocation_received: null,
    claim_date: null,
    status: 'pending',
  },
  {
    commitment_id: 'c-2',
    project_id: 'proj-2',
    project_name: 'AnotherToken',
    token_symbol: 'ANT',
    committed_usdt: '1000',
    estimated_tokens: '2000',
    allocation_received: '2000',
    claim_date: '2024-03-01',
    status: 'distributed',
  },
];

describe('CommitmentTable', () => {
  it('renders project names with symbols', async () => {
    await act(async () => {
      render(<CommitmentTable commitments={MOCK_COMMITMENTS} loading={false} />);
    });

    expect(screen.getByText('ExampleToken (EXT)')).toBeInTheDocument();
    expect(screen.getByText('AnotherToken (ANT)')).toBeInTheDocument();
  });

  it('renders committed USDT amounts', async () => {
    await act(async () => {
      render(<CommitmentTable commitments={MOCK_COMMITMENTS} loading={false} />);
    });

    expect(screen.getByText('500')).toBeInTheDocument();
    expect(screen.getByText('1000')).toBeInTheDocument();
  });

  it('renders status badges', async () => {
    await act(async () => {
      render(<CommitmentTable commitments={MOCK_COMMITMENTS} loading={false} />);
    });

    expect(screen.getByText('Pending')).toBeInTheDocument();
    expect(screen.getByText('Distributed')).toBeInTheDocument();
  });

  it('shows empty state when no commitments', async () => {
    await act(async () => {
      render(<CommitmentTable commitments={[]} loading={false} />);
    });

    expect(screen.getByText('No commitments yet')).toBeInTheDocument();
  });

  it('shows TBA for null claim date', async () => {
    await act(async () => {
      render(<CommitmentTable commitments={MOCK_COMMITMENTS} loading={false} />);
    });

    expect(screen.getByText('TBA')).toBeInTheDocument();
  });
});
