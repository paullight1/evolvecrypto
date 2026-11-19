import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { FiatHistory } from './FiatHistory';
import type { FiatOrder } from '../../types';

const MOCK_ORDERS: FiatOrder[] = [
  {
    id: 'f-1',
    type: 'buy',
    fiat_currency: 'USD',
    crypto_asset: 'BTC',
    fiat_amount: '500',
    crypto_amount: '0.0077',
    rate: '65000',
    fee: '7.50',
    payment_method: 'Credit Card',
    status: 'completed',
    created_at: '2025-03-01T10:00:00Z',
  },
  {
    id: 'f-2',
    type: 'sell',
    fiat_currency: 'EUR',
    crypto_asset: 'ETH',
    fiat_amount: '300',
    crypto_amount: '0.09',
    rate: '3200',
    fee: '1.50',
    payment_method: 'Bank Transfer',
    status: 'pending',
    created_at: '2025-03-02T12:00:00Z',
  },
];

describe('FiatHistory', () => {
  it('renders order type column', () => {
    render(<FiatHistory orders={MOCK_ORDERS} loading={false} />);

    expect(screen.getByText('BUY')).toBeInTheDocument();
    expect(screen.getByText('SELL')).toBeInTheDocument();
  });

  it('renders fiat and crypto columns', () => {
    render(<FiatHistory orders={MOCK_ORDERS} loading={false} />);

    expect(screen.getByText('USD')).toBeInTheDocument();
    expect(screen.getByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('EUR')).toBeInTheDocument();
  });

  it('renders status badges', () => {
    render(<FiatHistory orders={MOCK_ORDERS} loading={false} />);

    expect(screen.getByText('completed')).toBeInTheDocument();
    expect(screen.getByText('pending')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<FiatHistory orders={MOCK_ORDERS} loading={false} />);

    expect(screen.getByText('Type')).toBeInTheDocument();
    expect(screen.getByText('Fiat')).toBeInTheDocument();
    expect(screen.getByText('Crypto')).toBeInTheDocument();
    expect(screen.getByText('Status')).toBeInTheDocument();
  });

  it('shows empty state when no orders', () => {
    render(<FiatHistory orders={[]} loading={false} />);

    expect(screen.getByText('No fiat orders')).toBeInTheDocument();
  });
});
