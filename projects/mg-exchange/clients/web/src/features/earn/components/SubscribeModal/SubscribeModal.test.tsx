import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../../api', () => ({
  earnSubscribe: vi.fn(),
  getEarnProducts: vi.fn(),
  earnRedeem: vi.fn(),
  getEarnAllocations: vi.fn(),
  stakingStake: vi.fn(),
  stakingUnstake: vi.fn(),
  stakingClaimRewards: vi.fn(),
  getStakingBalance: vi.fn(),
}));

import { SubscribeModal } from './SubscribeModal';
import { earnSubscribe } from '../../api';
import { useToastStore } from '@shared/ui/Toast/store';
import type { EarnProduct } from '../../types';

const mockEarnSubscribe = vi.mocked(earnSubscribe);

const MOCK_PRODUCT: EarnProduct = {
  id: 'prod-1',
  coin: 'USDT',
  name: 'USDT Flexible',
  apy: '8.5%',
  productType: 'flexible',
  minAmount: '10',
  maxAmount: '1000000',
  lockDays: 0,
  supportsAutoCompound: true,
};

const FIXED_PRODUCT: EarnProduct = {
  id: 'prod-2',
  coin: 'BTC',
  name: 'BTC Fixed 30d',
  apy: '5.0%',
  productType: 'fixed_30d',
  minAmount: '0.001',
  maxAmount: '100',
  lockDays: 30,
  supportsAutoCompound: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockEarnSubscribe.mockResolvedValue({ allocation_id: 'alloc-1' });
  useToastStore.setState({ toasts: [] });
});

describe('SubscribeModal', () => {
  it('does not render when product is null', async () => {
    await act(async () => {
      render(
        <SubscribeModal product={null} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    expect(screen.queryByText('Subscribe to Earn')).not.toBeInTheDocument();
  });

  it('renders modal title when product is provided', async () => {
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    expect(screen.getByText('Subscribe to Earn')).toBeInTheDocument();
  });

  it('renders amount input', async () => {
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    expect(screen.getByLabelText(/Amount/i)).toBeInTheDocument();
  });

  it('shows APY value', async () => {
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    expect(screen.getByText('8.5%')).toBeInTheDocument();
  });

  it('shows lock period for fixed products', async () => {
    await act(async () => {
      render(
        <SubscribeModal product={FIXED_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    expect(screen.getByText('Lock Period')).toBeInTheDocument();
    expect(screen.getByText('30 days')).toBeInTheDocument();
  });

  it('shows estimated earnings section', async () => {
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    expect(screen.getByText('Est. earnings (30 days)')).toBeInTheDocument();
  });

  it('estimated earnings updates when amount is entered', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    const amountInput = screen.getByLabelText(/Amount/i);
    await user.type(amountInput, '1000');

    // 1000 * 8.5 / 100 / 365 * 30 = 6.986...
    // The estimateValue span shows "{estimated} {product.coin}"
    // Look for non-zero estimated value next to asset
    await waitFor(() => {
      const estimateSection = screen.getByText('Est. earnings (30 days)');
      expect(estimateSection).toBeInTheDocument();
      // The estimate value should show a non-zero decimal
      const estimateValue = screen.getByText(/\d+\.\d+ USDT/);
      expect(estimateValue).toBeInTheDocument();
    });
  });

  it('shows validation error when amount is empty on submit', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    await user.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));

    await waitFor(() => {
      expect(screen.getByText('Enter a valid amount')).toBeInTheDocument();
    });

    expect(mockEarnSubscribe).not.toHaveBeenCalled();
  });

  it('shows validation error when amount is below minimum', async () => {
    const user = userEvent.setup();
    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    const amountInput = screen.getByLabelText(/Amount/i);
    await user.type(amountInput, '5'); // min is 10

    await user.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));

    await waitFor(() => {
      expect(screen.getByText(/Minimum amount is 10 USDT/i)).toBeInTheDocument();
    });

    expect(mockEarnSubscribe).not.toHaveBeenCalled();
  });

  it('calls earnSubscribe API with productId and amount on valid submit', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();

    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={onSuccess} />,
      );
    });

    const amountInput = screen.getByLabelText(/Amount/i);
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));
    });

    await waitFor(() => {
      expect(mockEarnSubscribe).toHaveBeenCalledWith({
        productId: 'prod-1',
        amount: '100',
      });
    });
  });

  it('calls onSuccess after successful subscription', async () => {
    const user = userEvent.setup();
    const onSuccess = vi.fn();

    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={onSuccess} />,
      );
    });

    const amountInput = screen.getByLabelText(/Amount/i);
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));
    });

    await waitFor(() => {
      expect(onSuccess).toHaveBeenCalled();
    });
  });

  it('shows success toast after successful subscription', async () => {
    const user = userEvent.setup();

    await act(async () => {
      render(
        <SubscribeModal product={MOCK_PRODUCT} onClose={vi.fn()} onSuccess={vi.fn()} />,
      );
    });

    const amountInput = screen.getByLabelText(/Amount/i);
    await user.type(amountInput, '100');

    await act(async () => {
      await user.click(screen.getByRole('button', { name: 'Confirm Subscribe' }));
    });

    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.type === 'success')).toBe(true);
    });
  });
});
