import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { ReferralTree } from './ReferralTree';
import type { ReferralNode } from '../../types';

const MOCK_NODES: ReferralNode[] = [
  {
    user_id: 'u-1',
    nickname: 'Alice',
    level: 1,
    joined_at: 1700000000,
    total_commission: '100.00',
    children: [
      {
        user_id: 'u-2',
        nickname: 'Bob',
        level: 2,
        joined_at: 1700100000,
        total_commission: '20.00',
        children: [],
      },
      {
        user_id: 'u-3',
        nickname: 'Carol',
        level: 2,
        joined_at: 1700200000,
        total_commission: '15.00',
        children: [],
      },
    ],
  },
];

describe('ReferralTree', () => {
  it('shows loading state', async () => {
    await act(async () => {
      render(<ReferralTree nodes={[]} loading={true} />);
    });

    expect(screen.getByText('Loading...')).toBeInTheDocument();
  });

  it('shows empty state when no nodes', async () => {
    await act(async () => {
      render(<ReferralTree nodes={[]} loading={false} />);
    });

    expect(screen.getByText('No referrals yet')).toBeInTheDocument();
  });

  it('renders root node nickname', async () => {
    await act(async () => {
      render(<ReferralTree nodes={MOCK_NODES} loading={false} />);
    });

    expect(screen.getByText('Alice')).toBeInTheDocument();
  });

  it('root node starts expanded and shows children', async () => {
    await act(async () => {
      render(<ReferralTree nodes={MOCK_NODES} loading={false} />);
    });

    // Root node at depth=0 starts expanded
    expect(screen.getByText('Bob')).toBeInTheDocument();
    expect(screen.getByText('Carol')).toBeInTheDocument();
  });

  it('collapses children when toggle is clicked', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<ReferralTree nodes={MOCK_NODES} loading={false} />);
    });

    // Alice's toggle button is the first one (depth=0, has children)
    const toggleBtns = screen.getAllByRole('button');
    // Alice toggle — click to collapse
    await user.click(toggleBtns[0]);

    expect(screen.queryByText('Bob')).not.toBeInTheDocument();
    expect(screen.queryByText('Carol')).not.toBeInTheDocument();
  });

  it('expands children when toggle is clicked again', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(<ReferralTree nodes={MOCK_NODES} loading={false} />);
    });

    const toggleBtns = screen.getAllByRole('button');
    // Collapse
    await user.click(toggleBtns[0]);
    // Expand again
    await user.click(screen.getAllByRole('button')[0]);

    expect(screen.getByText('Bob')).toBeInTheDocument();
    expect(screen.getByText('Carol')).toBeInTheDocument();
  });
});
