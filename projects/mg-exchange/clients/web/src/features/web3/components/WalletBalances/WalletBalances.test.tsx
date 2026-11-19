import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { WalletBalances } from './WalletBalances';
import type { TokenBalance } from '../../types';

const MOCK_BALANCES: TokenBalance[] = [
  { token: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48', symbol: 'USDC', balance: '1000', usd_value: '1000', chain: 'Ethereum' },
  { token: '0xdAC17F958D2ee523a2206206994597C13D831ec7', symbol: 'USDT', balance: '500', usd_value: '500', chain: 'Polygon' },
  { token: 'native', symbol: 'ETH', balance: '0.5', usd_value: '1600', chain: 'Ethereum' },
];

describe('WalletBalances', () => {
  it('renders token symbols', () => {
    render(<WalletBalances balances={MOCK_BALANCES} loading={false} />);

    expect(screen.getByText('USDC')).toBeInTheDocument();
    expect(screen.getByText('USDT')).toBeInTheDocument();
    expect(screen.getByText('ETH')).toBeInTheDocument();
  });

  it('renders chain names', () => {
    render(<WalletBalances balances={MOCK_BALANCES} loading={false} />);

    expect(screen.getAllByText('Ethereum').length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('Polygon')).toBeInTheDocument();
  });

  it('renders balance values', () => {
    render(<WalletBalances balances={MOCK_BALANCES} loading={false} />);

    expect(screen.getByText('1000')).toBeInTheDocument();
    expect(screen.getByText('500')).toBeInTheDocument();
    expect(screen.getByText('0.5')).toBeInTheDocument();
  });

  it('renders USD values with dollar sign', () => {
    render(<WalletBalances balances={MOCK_BALANCES} loading={false} />);

    expect(screen.getByText('$1000')).toBeInTheDocument();
    expect(screen.getByText('$1600')).toBeInTheDocument();
  });

  it('shows empty state when no balances', () => {
    render(<WalletBalances balances={[]} loading={false} />);

    expect(screen.getByText(/No balances found/i)).toBeInTheDocument();
  });
});
