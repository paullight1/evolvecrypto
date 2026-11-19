import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { DeliveryPositions } from './DeliveryPositions';
import type { DeliveryPosition } from '../../types';

const MOCK_POSITIONS: DeliveryPosition[] = [
  {
    symbol: 'BTCUSDT-0328',
    side: 'buy',
    size: '0.5',
    entry_price: '64000',
    mark_price: '65000',
    unrealized_pnl: '500',
    margin: '6400',
    delivery_date: '2025-03-28T00:00:00Z',
  },
  {
    symbol: 'ETHUSDT-0328',
    side: 'sell',
    size: '2',
    entry_price: '3300',
    mark_price: '3200',
    unrealized_pnl: '200',
    margin: '660',
    delivery_date: '2025-03-28T00:00:00Z',
  },
];

describe('DeliveryPositions', () => {
  it('renders position symbols', () => {
    render(<DeliveryPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('BTCUSDT-0328')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT-0328')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<DeliveryPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('Symbol')).toBeInTheDocument();
    expect(screen.getByText('Side')).toBeInTheDocument();
    expect(screen.getByText('Size')).toBeInTheDocument();
    expect(screen.getByText('Entry Price')).toBeInTheDocument();
    expect(screen.getByText('Unr. PnL')).toBeInTheDocument();
  });

  it('renders side badges', () => {
    render(<DeliveryPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('buy')).toBeInTheDocument();
    expect(screen.getByText('sell')).toBeInTheDocument();
  });

  it('shows empty state when no positions', () => {
    render(<DeliveryPositions positions={[]} loading={false} />);

    expect(screen.getByText('No open delivery positions')).toBeInTheDocument();
  });

  it('renders margin values', () => {
    render(<DeliveryPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('6400')).toBeInTheDocument();
    expect(screen.getByText('660')).toBeInTheDocument();
  });
});
