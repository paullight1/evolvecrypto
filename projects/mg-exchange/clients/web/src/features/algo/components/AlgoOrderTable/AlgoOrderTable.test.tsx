import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  cancelAlgo: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { AlgoOrderTable } from './AlgoOrderTable';
import { cancelAlgo } from '../../api';
import type { AlgoOrder } from '../../types';

const mockCancelAlgo = vi.mocked(cancelAlgo);

const MOCK_ORDERS: AlgoOrder[] = [
  {
    id: 'algo-1',
    type: 'twap',
    symbol: 'BTCUSDT',
    side: 'buy',
    total_qty: '1',
    executed_qty: '0.3',
    duration_seconds: 3600,
    slices: 10,
    status: 'running',
    created_at: '2025-03-01T10:00:00Z',
  },
  {
    id: 'algo-2',
    type: 'vwap',
    symbol: 'ETHUSDT',
    side: 'sell',
    total_qty: '5',
    executed_qty: '5',
    duration_seconds: 7200,
    status: 'completed',
    created_at: '2025-03-02T12:00:00Z',
  },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockCancelAlgo.mockResolvedValue(undefined);
});

describe('AlgoOrderTable', () => {
  it('renders symbol and type columns', () => {
    render(<AlgoOrderTable orders={MOCK_ORDERS} loading={false} onCancelled={vi.fn()} />);

    expect(screen.getByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
    expect(screen.getByText('TWAP')).toBeInTheDocument();
    expect(screen.getByText('VWAP')).toBeInTheDocument();
  });

  it('renders status badges', () => {
    render(<AlgoOrderTable orders={MOCK_ORDERS} loading={false} onCancelled={vi.fn()} />);

    expect(screen.getByText('running')).toBeInTheDocument();
    expect(screen.getByText('completed')).toBeInTheDocument();
  });

  it('does not show Cancel buttons when showActions is false', () => {
    render(<AlgoOrderTable orders={MOCK_ORDERS} loading={false} onCancelled={vi.fn()} showActions={false} />);

    expect(screen.queryByRole('button', { name: 'Cancel' })).not.toBeInTheDocument();
  });

  it('shows Cancel button for running orders when showActions is true', () => {
    render(<AlgoOrderTable orders={MOCK_ORDERS} loading={false} onCancelled={vi.fn()} showActions={true} />);

    // Only running order has Cancel button (completed does not)
    const cancelBtns = screen.getAllByRole('button', { name: 'Cancel' });
    expect(cancelBtns).toHaveLength(1);
  });

  it('Cancel button calls cancelAlgo and fires onCancelled', async () => {
    const onCancelled = vi.fn();
    const user = userEvent.setup();
    render(<AlgoOrderTable orders={MOCK_ORDERS} loading={false} onCancelled={onCancelled} showActions={true} />);

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
    });

    await waitFor(() => {
      expect(mockCancelAlgo).toHaveBeenCalledWith('algo-1');
      expect(onCancelled).toHaveBeenCalled();
    });
  });

  it('shows empty state when no orders', () => {
    render(<AlgoOrderTable orders={[]} loading={false} onCancelled={vi.fn()} />);

    expect(screen.getByText('No algo orders')).toBeInTheDocument();
  });
});
