import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { SpreadList } from './SpreadList';
import type { SpreadInstrument } from '../../types';

const MOCK_INSTRUMENTS: SpreadInstrument[] = [
  {
    id: 'sp-1',
    name: 'BTC-JAN25-FEB25',
    near_leg: 'BTCUSDT-JAN25',
    far_leg: 'BTCUSDT-FEB25',
    spread_value: '-150.00',
    bid: '-155.00',
    ask: '-145.00',
    last: '-148.00',
    volume_24h: '500000',
  },
  {
    id: 'sp-2',
    name: 'ETH-JAN25-MAR25',
    near_leg: 'ETHUSDT-JAN25',
    far_leg: 'ETHUSDT-MAR25',
    spread_value: '-25.00',
    bid: '-26.00',
    ask: '-24.00',
    last: '-25.00',
    volume_24h: '200000',
  },
];

describe('SpreadList', () => {
  it('renders instrument names', async () => {
    await act(async () => {
      render(<SpreadList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={vi.fn()} />);
    });

    expect(screen.getByText('BTC-JAN25-FEB25')).toBeInTheDocument();
    expect(screen.getByText('ETH-JAN25-MAR25')).toBeInTheDocument();
  });

  it('renders near and far legs', async () => {
    await act(async () => {
      render(<SpreadList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={vi.fn()} />);
    });

    expect(screen.getByText('BTCUSDT-JAN25')).toBeInTheDocument();
    expect(screen.getByText('BTCUSDT-FEB25')).toBeInTheDocument();
  });

  it('calls onSelect when a row is clicked', async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    await act(async () => {
      render(<SpreadList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={onSelect} />);
    });

    await user.click(screen.getByText('BTC-JAN25-FEB25'));

    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'sp-1' }));
  });

  it('shows empty state when no instruments', async () => {
    await act(async () => {
      render(<SpreadList instruments={[]} loading={false} onSelect={vi.fn()} />);
    });

    expect(screen.getByText('No spread instruments available')).toBeInTheDocument();
  });

  it('renders table headers', async () => {
    await act(async () => {
      render(<SpreadList instruments={MOCK_INSTRUMENTS} loading={false} onSelect={vi.fn()} />);
    });

    expect(screen.getByText('Spread')).toBeInTheDocument();
    expect(screen.getByText('Near Leg')).toBeInTheDocument();
    expect(screen.getByText('Far Leg')).toBeInTheDocument();
  });
});
