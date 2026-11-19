import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  getMyOrders: vi.fn(),
  markPaid: vi.fn(),
  releaseOrder: vi.fn(),
  disputeOrder: vi.fn(),
  cancelOrder: vi.fn(),
  getP2PAds: vi.fn(),
  createP2POrder: vi.fn(),
  createAd: vi.fn(),
  getMyAds: vi.fn(),
}));

import { OrderFlow } from './OrderFlow';
import { getMyOrders, markPaid, releaseOrder, disputeOrder, cancelOrder } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { P2POrder } from '../../types';

const mockGetMyOrders = vi.mocked(getMyOrders);
const mockMarkPaid = vi.mocked(markPaid);
const mockReleaseOrder = vi.mocked(releaseOrder);
const mockDisputeOrder = vi.mocked(disputeOrder);
const mockCancelOrder = vi.mocked(cancelOrder);

// The component uses order.buyerId === order.id for "Mark Paid"
// and order.sellerId === order.id for "Release"
function makeBuyOrder(overrides: Partial<P2POrder> = {}): P2POrder {
  return {
    id: 'order-1',
    adId: 'ad-1',
    buyerId: 'order-1', // matches id → Mark Paid shows
    sellerId: 'seller-1',
    asset: 'USDT',
    fiatCurrency: 'USD',
    cryptoAmount: '100',
    fiatAmount: '100',
    status: 'Created',
    createdAt: 1700000000,
    ...overrides,
  };
}

function makeSellOrder(overrides: Partial<P2POrder> = {}): P2POrder {
  return {
    id: 'order-1',
    adId: 'ad-1',
    buyerId: 'buyer-1',
    sellerId: 'order-1', // matches id → Release shows
    asset: 'USDT',
    fiatCurrency: 'USD',
    cryptoAmount: '100',
    fiatAmount: '100',
    status: 'Created',
    createdAt: 1700000000,
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mockGetMyOrders.mockResolvedValue([]);
  mockMarkPaid.mockResolvedValue(undefined);
  mockReleaseOrder.mockResolvedValue(undefined);
  mockDisputeOrder.mockResolvedValue(undefined);
  mockCancelOrder.mockResolvedValue(undefined);
  useToastStore.setState({ toasts: [] });
});

describe('OrderFlow', () => {
  it('renders "No orders yet" when there are no orders', async () => {
    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByText('No orders yet')).toBeInTheDocument();
    });
  });

  it('renders order table with column headers', async () => {
    await act(async () => { render(<OrderFlow />); });

    expect(screen.getByText('Order ID')).toBeInTheDocument();
    expect(screen.getByText('Asset')).toBeInTheDocument();
    expect(screen.getByText('Status')).toBeInTheDocument();
    expect(screen.getByText('Actions')).toBeInTheDocument();
  });

  it('shows "Mark Paid" button for buy orders with "Created" status', async () => {
    const order = makeBuyOrder({ status: 'Created' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Mark Paid' })).toBeInTheDocument();
    });
  });

  it('does NOT show "Mark Paid" for sell orders', async () => {
    const order = makeSellOrder({ status: 'Created' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Mark Paid' })).not.toBeInTheDocument();
    });
  });

  it('shows "Release" button for sell orders with "PaymentSent" status', async () => {
    const order = makeSellOrder({ status: 'PaymentSent' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Release' })).toBeInTheDocument();
    });
  });

  it('does NOT show "Release" for buy orders', async () => {
    const order = makeBuyOrder({ status: 'PaymentSent' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Release' })).not.toBeInTheDocument();
    });
  });

  it('shows "Dispute" button for Created orders', async () => {
    const order = makeBuyOrder({ status: 'Created' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Dispute' })).toBeInTheDocument();
    });
  });

  it('shows "Dispute" button for PaymentSent orders', async () => {
    const order = makeSellOrder({ status: 'PaymentSent' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Dispute' })).toBeInTheDocument();
    });
  });

  it('does NOT show Dispute for completed orders', async () => {
    const order = makeBuyOrder({ status: 'Released' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.queryByRole('button', { name: 'Dispute' })).not.toBeInTheDocument();
    });
  });

  it('shows "Cancel" button for Created orders', async () => {
    const order = makeBuyOrder({ status: 'Created' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    });
  });

  it('Mark Paid button calls markPaid API', async () => {
    const user = userEvent.setup();
    const order = makeBuyOrder({ id: 'order-42', status: 'Created', buyerId: 'order-42' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Mark Paid' })).toBeInTheDocument();
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Mark Paid' }));
    });

    await waitFor(() => {
      expect(mockMarkPaid).toHaveBeenCalledWith('order-42');
    });
  });

  it('Release button calls releaseOrder API', async () => {
    const user = userEvent.setup();
    const order = makeSellOrder({ id: 'order-99', status: 'PaymentSent', sellerId: 'order-99' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Release' })).toBeInTheDocument();
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Release' }));
    });

    await waitFor(() => {
      expect(mockReleaseOrder).toHaveBeenCalledWith('order-99');
    });
  });

  it('Dispute button calls disputeOrder API', async () => {
    const user = userEvent.setup();
    const order = makeBuyOrder({ id: 'order-55', status: 'Created', buyerId: 'order-55' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Dispute' })).toBeInTheDocument();
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Dispute' }));
    });

    await waitFor(() => {
      expect(mockDisputeOrder).toHaveBeenCalledWith('order-55');
    });
  });

  it('Cancel button calls cancelOrder API', async () => {
    const user = userEvent.setup();
    const order = makeBuyOrder({ id: 'order-77', status: 'Created', buyerId: 'order-77' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
    });

    await waitFor(() => {
      expect(mockCancelOrder).toHaveBeenCalledWith('order-77');
    });
  });

  it('shows success toast after action completes', async () => {
    const user = userEvent.setup();
    const order = makeBuyOrder({ status: 'Created', buyerId: 'order-1' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'success' && t.message === 'Action completed')).toBe(true);
    });
  });

  it('shows error toast when action fails', async () => {
    mockCancelOrder.mockRejectedValue(new Error('Order already cancelled'));
    const user = userEvent.setup();
    const order = makeBuyOrder({ status: 'Created', buyerId: 'order-1' });
    mockGetMyOrders.mockResolvedValue([order]);

    await act(async () => { render(<OrderFlow />); });

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Cancel' })).toBeInTheDocument();
    });

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Cancel' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'error')).toBe(true);
    });
  });
});
