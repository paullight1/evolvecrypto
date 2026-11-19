import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { InstrumentList } from './InstrumentList';
import type { DeliveryInstrument } from '../../types';

const MOCK_INSTRUMENTS: DeliveryInstrument[] = [
  {
    symbol: 'BTCUSDT-0328',
    underlying: 'BTC',
    delivery_date: '2025-03-28T00:00:00Z',
    days_remaining: 5,
    last_price: '65000',
    mark_price: '65100',
    basis: '0.15',
    oi: '12000',
  },
  {
    symbol: 'ETHUSDT-0328',
    underlying: 'ETH',
    delivery_date: '2025-03-28T00:00:00Z',
    days_remaining: 45,
    last_price: '3200',
    mark_price: '3210',
    basis: '0.31',
    oi: '5000',
  },
];

describe('InstrumentList', () => {
  it('renders instrument symbols', () => {
    render(<InstrumentList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={vi.fn()} />);

    expect(screen.getByText('BTCUSDT-0328')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT-0328')).toBeInTheDocument();
  });

  it('renders column headers', () => {
    render(<InstrumentList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={vi.fn()} />);

    expect(screen.getByText('Symbol')).toBeInTheDocument();
    expect(screen.getByText('Underlying')).toBeInTheDocument();
    expect(screen.getByText('Days Left')).toBeInTheDocument();
  });

  it('clicking a row fires onSelect with the symbol', async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(<InstrumentList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={onSelect} />);

    await user.click(screen.getByText('BTCUSDT-0328'));

    expect(onSelect).toHaveBeenCalledWith('BTCUSDT-0328');
  });

  it('shows empty state when no instruments', () => {
    render(<InstrumentList instruments={[]} loading={false} onSelect={vi.fn()} />);

    expect(screen.getByText('No delivery instruments available')).toBeInTheDocument();
  });

  it('renders last price values', () => {
    render(<InstrumentList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={vi.fn()} />);

    expect(screen.getByText('65000')).toBeInTheDocument();
    expect(screen.getByText('3200')).toBeInTheDocument();
  });
});
