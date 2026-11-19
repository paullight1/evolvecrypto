import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { ProductCard } from './ProductCard';
import type { EarnProduct } from '../../types';

const FLEXIBLE_PRODUCT: EarnProduct = {
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

const STAKING_PRODUCT: EarnProduct = {
  id: 'prod-3',
  coin: 'ETH',
  name: 'ETH Staking',
  apy: '4.2%',
  productType: 'staking',
  minAmount: '0.01',
  maxAmount: '1000',
  lockDays: 0,
  supportsAutoCompound: false,
};

describe('ProductCard', () => {
  it('renders product name (asset)', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('USDT')).toBeInTheDocument();
  });

  it('renders APY value', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('8.5%')).toBeInTheDocument();
  });

  it('renders APY label', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('APY')).toBeInTheDocument();
  });

  it('renders "Flexible" type badge for flexible products', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('Flexible')).toBeInTheDocument();
  });

  it('renders "Fixed 30d" type badge for fixed products', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FIXED_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('Fixed 30d')).toBeInTheDocument();
  });

  it('shows lock period for fixed products with lockDays > 0', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FIXED_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('Lock Period')).toBeInTheDocument();
    expect(screen.getByText('30 days')).toBeInTheDocument();
  });

  it('shows "None" lock period for flexible products', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('None')).toBeInTheDocument();
  });

  it('shows "None" lock period when lockDays is 0', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={STAKING_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('None')).toBeInTheDocument();
  });

  it('renders Subscribe button', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByRole('button', { name: 'Subscribe' })).toBeInTheDocument();
  });

  it('Subscribe button calls onSubscribe with the product', async () => {
    const user = userEvent.setup();
    const onSubscribe = vi.fn();

    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    await user.click(screen.getByRole('button', { name: 'Subscribe' }));

    expect(onSubscribe).toHaveBeenCalledWith(FLEXIBLE_PRODUCT);
  });

  it('shows min amount', async () => {
    const onSubscribe = vi.fn();
    await act(async () => {
      render(<ProductCard product={FLEXIBLE_PRODUCT} onSubscribe={onSubscribe} />);
    });

    expect(screen.getByText('10 USDT')).toBeInTheDocument();
  });
});
