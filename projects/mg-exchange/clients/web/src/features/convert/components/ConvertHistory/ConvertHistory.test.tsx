import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { ConvertHistory } from './ConvertHistory';
import type { ConvertHistoryItem } from '../../types';

const MOCK_ITEMS: ConvertHistoryItem[] = [
  {
    id: 'c-1',
    from_asset: 'BTC',
    to_asset: 'USDT',
    from_amount: '0.1',
    to_amount: '6500',
    rate: '65000',
    status: 'completed',
    created_at: '2025-03-01T10:00:00Z',
  },
  {
    id: 'c-2',
    from_asset: 'ETH',
    to_asset: 'USDC',
    from_amount: '2',
    to_amount: '6400',
    rate: '3200',
    status: 'failed',
    created_at: '2025-03-02T12:00:00Z',
  },
];

describe('ConvertHistory', () => {
  it('renders from and to asset columns', () => {
    render(<ConvertHistory items={MOCK_ITEMS} loading={false} />);

    expect(screen.getByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('USDT')).toBeInTheDocument();
    expect(screen.getByText('ETH')).toBeInTheDocument();
  });

  it('renders amount columns', () => {
    render(<ConvertHistory items={MOCK_ITEMS} loading={false} />);

    expect(screen.getByText('0.1')).toBeInTheDocument();
    expect(screen.getByText('6500')).toBeInTheDocument();
  });

  it('renders status badges', () => {
    render(<ConvertHistory items={MOCK_ITEMS} loading={false} />);

    expect(screen.getByText('completed')).toBeInTheDocument();
    expect(screen.getByText('failed')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<ConvertHistory items={MOCK_ITEMS} loading={false} />);

    expect(screen.getByText('From')).toBeInTheDocument();
    expect(screen.getByText('To')).toBeInTheDocument();
    expect(screen.getByText('Sold')).toBeInTheDocument();
    expect(screen.getByText('Received')).toBeInTheDocument();
    expect(screen.getByText('Status')).toBeInTheDocument();
  });

  it('shows empty state when no items', () => {
    render(<ConvertHistory items={[]} loading={false} />);

    expect(screen.getByText('No conversion history')).toBeInTheDocument();
  });
});
