import { render, screen, act } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';

import { TokenList } from './TokenList';
import type { LeveragedToken } from '../../types';

const MOCK_TOKENS: LeveragedToken[] = [
  {
    id: 'lt-1',
    symbol: 'BTC3L',
    underlying: 'BTC',
    leverage: '3',
    nav: '1.23',
    change_24h: '5.5',
    total_supply: '1000000',
    management_fee: '0.1',
  },
  {
    id: 'lt-2',
    symbol: 'ETH2S',
    underlying: 'ETH',
    leverage: '2',
    nav: '0.89',
    change_24h: '-3.2',
    total_supply: '500000',
    management_fee: '0.1',
  },
];

describe('TokenList', () => {
  it('renders token symbols', async () => {
    await act(async () => {
      render(<TokenList tokens={MOCK_TOKENS} loading={false} onPurchase={vi.fn()} onRedeem={vi.fn()} />);
    });

    expect(screen.getByText('BTC3L')).toBeInTheDocument();
    expect(screen.getByText('ETH2S')).toBeInTheDocument();
  });

  it('renders leverage badges', async () => {
    await act(async () => {
      render(<TokenList tokens={MOCK_TOKENS} loading={false} onPurchase={vi.fn()} onRedeem={vi.fn()} />);
    });

    expect(screen.getByText('3x')).toBeInTheDocument();
    expect(screen.getByText('2x')).toBeInTheDocument();
  });

  it('calls onPurchase when Buy button is clicked', async () => {
    const user = userEvent.setup();
    const onPurchase = vi.fn();
    await act(async () => {
      render(<TokenList tokens={MOCK_TOKENS} loading={false} onPurchase={onPurchase} onRedeem={vi.fn()} />);
    });

    await user.click(screen.getAllByRole('button', { name: 'Buy' })[0]);

    expect(onPurchase).toHaveBeenCalledWith(expect.objectContaining({ id: 'lt-1' }));
  });

  it('calls onRedeem when Redeem button is clicked', async () => {
    const user = userEvent.setup();
    const onRedeem = vi.fn();
    await act(async () => {
      render(<TokenList tokens={MOCK_TOKENS} loading={false} onPurchase={vi.fn()} onRedeem={onRedeem} />);
    });

    await user.click(screen.getAllByRole('button', { name: 'Redeem' })[0]);

    expect(onRedeem).toHaveBeenCalledWith(expect.objectContaining({ id: 'lt-1' }));
  });

  it('shows empty state when no tokens', async () => {
    await act(async () => {
      render(<TokenList tokens={[]} loading={false} onPurchase={vi.fn()} onRedeem={vi.fn()} />);
    });

    expect(screen.getByText('No leveraged tokens available')).toBeInTheDocument();
  });
});
