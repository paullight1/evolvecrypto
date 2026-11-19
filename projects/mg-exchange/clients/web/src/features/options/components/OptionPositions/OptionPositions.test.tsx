import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { OptionPositions } from './OptionPositions';
import type { OptionPosition } from '../../types';

const MOCK_POSITIONS: OptionPosition[] = [
  {
    symbol: 'BTC-28MAR25-60000-C',
    underlying: 'BTC',
    qty: '2',
    avgEntryPremium: '1200',
    markPrice: '1350',
    unrealisedPnl: '300',
    updatedAt: '2025-03-28T10:00:00.000Z',
  },
  {
    symbol: 'BTC-28MAR25-70000-P',
    underlying: 'BTC',
    qty: '1',
    avgEntryPremium: '800',
    markPrice: '700',
    unrealisedPnl: '100',
    updatedAt: '2025-03-28T10:00:00.000Z',
  },
];

describe('OptionPositions', () => {
  it('renders symbol column', () => {
    render(<OptionPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('BTC-28MAR25-60000-C')).toBeInTheDocument();
    expect(screen.getByText('BTC-28MAR25-70000-P')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<OptionPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('Symbol')).toBeInTheDocument();
    expect(screen.getByText('Qty')).toBeInTheDocument();
    expect(screen.getByText('Entry Premium')).toBeInTheDocument();
    expect(screen.getByText('Mark')).toBeInTheDocument();
    expect(screen.getByText('Unr. PnL')).toBeInTheDocument();
  });

  it('renders position values', () => {
    render(<OptionPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('1200')).toBeInTheDocument();
    expect(screen.getByText('1350')).toBeInTheDocument();
    expect(screen.getByText('300')).toBeInTheDocument();
  });

  it('shows empty state when no positions', () => {
    render(<OptionPositions positions={[]} loading={false} />);

    expect(screen.getByText('No open option positions')).toBeInTheDocument();
  });

  it('renders qty values for both positions', () => {
    render(<OptionPositions positions={MOCK_POSITIONS} loading={false} />);

    expect(screen.getByText('2')).toBeInTheDocument();
    expect(screen.getByText('1')).toBeInTheDocument();
  });
});
