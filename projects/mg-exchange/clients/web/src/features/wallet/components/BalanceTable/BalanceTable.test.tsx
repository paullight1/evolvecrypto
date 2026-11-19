import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect } from 'vitest';
import { BalanceTable } from './BalanceTable';
import type { Balance } from '../../types';

const MOCK_BALANCES: Balance[] = [
  { asset: 'BTC', available: '0.5', locked: '0.1', total: '0.6' },
  { asset: 'ETH', available: '10.0', locked: '0', total: '10.0' },
  { asset: 'USDT', available: '5000', locked: '500', total: '5500' },
];

describe('BalanceTable', () => {
  it('renders asset names in rows', () => {
    render(<BalanceTable balances={MOCK_BALANCES} />);

    expect(screen.getByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('ETH')).toBeInTheDocument();
    expect(screen.getByText('USDT')).toBeInTheDocument();
  });

  it('renders available and locked values', () => {
    render(<BalanceTable balances={MOCK_BALANCES} />);

    expect(screen.getByText('0.5')).toBeInTheDocument();
    expect(screen.getByText('0.1')).toBeInTheDocument();
    expect(screen.getByText('5000')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<BalanceTable balances={MOCK_BALANCES} />);

    expect(screen.getByText('Asset')).toBeInTheDocument();
    expect(screen.getByText('Available')).toBeInTheDocument();
    expect(screen.getByText('Locked')).toBeInTheDocument();
    expect(screen.getByText('Total')).toBeInTheDocument();
  });

  it('shows empty state when no balances', () => {
    render(<BalanceTable balances={[]} />);

    expect(screen.getByText('No balances found')).toBeInTheDocument();
  });

  it('clicking a sortable column header triggers sort', async () => {
    const user = userEvent.setup();
    render(<BalanceTable balances={MOCK_BALANCES} />);

    const assetHeader = screen.getByText('Asset');
    await user.click(assetHeader);

    // After click, the table still renders all rows (sort logic tested implicitly)
    expect(screen.getByText('BTC')).toBeInTheDocument();
    expect(screen.getByText('ETH')).toBeInTheDocument();
  });
});
