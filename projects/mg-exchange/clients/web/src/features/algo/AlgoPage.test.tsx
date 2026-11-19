import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./api', () => ({
  createTwap: vi.fn(),
  createVwap: vi.fn(),
  getAlgoOrders: vi.fn(),
  cancelAlgo: vi.fn(),
}));

import AlgoPage from './AlgoPage';
import { createTwap, createVwap, getAlgoOrders, cancelAlgo } from './api';
import type { AlgoOrder } from './types';

const mockCreateTwap = vi.mocked(createTwap);
const mockCreateVwap = vi.mocked(createVwap);
const mockGetAlgoOrders = vi.mocked(getAlgoOrders);
const mockCancelAlgo = vi.mocked(cancelAlgo);

const MOCK_ACTIVE_ORDERS: AlgoOrder[] = [
  {
    id: 'algo-1',
    type: 'twap',
    symbol: 'BTCUSDT',
    side: 'buy',
    total_qty: '1.0',
    executed_qty: '0.3',
    duration_seconds: 3600,
    slices: 10,
    status: 'running',
    created_at: new Date().toISOString(),
  },
];

const MOCK_NEW_ORDER: AlgoOrder = {
  id: 'algo-new',
  type: 'twap',
  symbol: 'BTCUSDT',
  side: 'buy',
  total_qty: '0.5',
  executed_qty: '0',
  duration_seconds: 3600,
  slices: 10,
  status: 'running',
  created_at: new Date().toISOString(),
};

beforeEach(() => {
  vi.clearAllMocks();
  mockGetAlgoOrders.mockResolvedValue(MOCK_ACTIVE_ORDERS);
  mockCreateTwap.mockResolvedValue(MOCK_NEW_ORDER);
  mockCreateVwap.mockResolvedValue({ ...MOCK_NEW_ORDER, type: 'vwap' });
  mockCancelAlgo.mockResolvedValue(undefined);
});

describe('AlgoPage', () => {
  it('renders title and all tabs', () => {
    render(<AlgoPage />);
    expect(screen.getByText('Algo Execution')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'TWAP' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'VWAP' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Active Orders' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'History' })).toBeInTheDocument();
  });

  it('TWAP form validates zero or negative duration', async () => {
    const { useToastStore } = await import('@shared/ui/Toast/store');
    const user = userEvent.setup();
    render(<AlgoPage />);

    // Clear duration (default is "60") and set to 0
    const durationInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durationInput);
    await user.type(durationInput, '0');

    await user.type(screen.getByLabelText('Quantity'), '0.5');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create TWAP Order' }));
    });

    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.message === 'Enter a valid duration')).toBe(true);
    expect(mockCreateTwap).not.toHaveBeenCalled();
  });

  it('TWAP form validates zero quantity', async () => {
    const { useToastStore } = await import('@shared/ui/Toast/store');
    const user = userEvent.setup();
    render(<AlgoPage />);

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create TWAP Order' }));
    });

    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.message === 'Enter a valid quantity')).toBe(true);
    expect(mockCreateTwap).not.toHaveBeenCalled();
  });

  it('TWAP submit calls createTwap with correct params', async () => {
    const user = userEvent.setup();
    render(<AlgoPage />);

    await user.type(screen.getByLabelText('Quantity'), '0.5');

    const durationInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durationInput);
    await user.type(durationInput, '30');

    const slicesInput = screen.getByLabelText('Slices');
    await user.clear(slicesInput);
    await user.type(slicesInput, '6');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create TWAP Order' }));
    });

    expect(mockCreateTwap).toHaveBeenCalledWith({
      symbol: 'BTCUSDT',
      side: 'buy',
      qty: '0.5',
      duration_seconds: 1800, // 30 minutes * 60
      slices: 6,
    });
  });

  it('VWAP form validates zero quantity', async () => {
    const { useToastStore } = await import('@shared/ui/Toast/store');
    const user = userEvent.setup();
    render(<AlgoPage />);

    await user.click(screen.getByRole('tab', { name: 'VWAP' }));

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create VWAP Order' }));
    });

    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.message === 'Enter a valid quantity')).toBe(true);
    expect(mockCreateVwap).not.toHaveBeenCalled();
  });

  it('VWAP submit calls createVwap with correct params', async () => {
    const user = userEvent.setup();
    render(<AlgoPage />);

    await user.click(screen.getByRole('tab', { name: 'VWAP' }));

    await user.type(screen.getByLabelText('Quantity'), '1');

    const durationInput = screen.getByLabelText('Duration (minutes)');
    await user.clear(durationInput);
    await user.type(durationInput, '120');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Create VWAP Order' }));
    });

    expect(mockCreateVwap).toHaveBeenCalledWith({
      symbol: 'BTCUSDT',
      side: 'buy',
      qty: '1',
      duration_seconds: 7200, // 120 minutes * 60
    });
  });

  it('active orders tab shows running orders with Cancel button', async () => {
    const user = userEvent.setup();
    render(<AlgoPage />);

    await user.click(screen.getByRole('tab', { name: 'Active Orders' }));

    expect(await screen.findByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    expect(mockGetAlgoOrders).toHaveBeenCalledWith('running');
  });

  it('clicking Cancel on active order calls cancelAlgo with correct id', async () => {
    const user = userEvent.setup();
    render(<AlgoPage />);

    await user.click(screen.getByRole('tab', { name: 'Active Orders' }));
    await screen.findByText('BTCUSDT');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
    });

    expect(mockCancelAlgo).toHaveBeenCalledWith('algo-1');
  });
});
