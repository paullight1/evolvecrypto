import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { BorrowTable } from './BorrowTable';
import type { MarginBorrow } from '../../types';

const MOCK_BORROWS: MarginBorrow[] = [
  { asset: 'USDT', amount: '2000', interest_rate: '0.01', interest_accrued: '4.50', created_at: 1700000000 },
  { asset: 'BTC', amount: '0.05', interest_rate: '0.005', interest_accrued: '0.001', created_at: 1700003600 },
];

describe('BorrowTable', () => {
  it('renders asset names', () => {
    render(<BorrowTable borrows={MOCK_BORROWS} loading={false} />);

    expect(screen.getByText('USDT')).toBeInTheDocument();
    expect(screen.getByText('BTC')).toBeInTheDocument();
  });

  it('renders borrowed amounts', () => {
    render(<BorrowTable borrows={MOCK_BORROWS} loading={false} />);

    expect(screen.getByText('2000')).toBeInTheDocument();
    expect(screen.getByText('0.05')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<BorrowTable borrows={MOCK_BORROWS} loading={false} />);

    expect(screen.getByText('Asset')).toBeInTheDocument();
    expect(screen.getByText('Borrowed')).toBeInTheDocument();
    expect(screen.getByText('Rate (hr)')).toBeInTheDocument();
    expect(screen.getByText('Interest')).toBeInTheDocument();
  });

  it('renders interest accrued values', () => {
    render(<BorrowTable borrows={MOCK_BORROWS} loading={false} />);

    expect(screen.getByText('4.50')).toBeInTheDocument();
    expect(screen.getByText('0.001')).toBeInTheDocument();
  });

  it('shows empty state when no borrows', () => {
    render(<BorrowTable borrows={[]} loading={false} />);

    expect(screen.getByText('No active borrows')).toBeInTheDocument();
  });
});
