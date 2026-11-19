import { render, screen, act } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';

import { BlockTradeHistory } from './BlockTradeHistory';
import type { BlockTrade } from '../../types';

const MOCK_TRADES: BlockTrade[] = [
  {
    id: 'bt-1',
    symbol: 'BTCUSDT',
    side: 'buy',
    qty: '2',
    price: '65000.00',
    notional_usd: '130000',
    counterparty: 'MarketMaker1',
    executed_at: new Date('2024-01-15').toISOString(),
  },
  {
    id: 'bt-2',
    symbol: 'ETHUSDT',
    side: 'sell',
    qty: '10',
    price: '3500.00',
    notional_usd: '35000',
    counterparty: 'MarketMaker2',
    executed_at: new Date('2024-01-16').toISOString(),
  },
];

describe('BlockTradeHistory', () => {
  it('renders trade history table with symbol column', async () => {
    await act(async () => {
      render(<BlockTradeHistory trades={MOCK_TRADES} loading={false} />);
    });

    expect(screen.getByText('BTCUSDT')).toBeInTheDocument();
    expect(screen.getByText('ETHUSDT')).toBeInTheDocument();
  });

  it('renders counterparty names', async () => {
    await act(async () => {
      render(<BlockTradeHistory trades={MOCK_TRADES} loading={false} />);
    });

    expect(screen.getByText('MarketMaker1')).toBeInTheDocument();
    expect(screen.getByText('MarketMaker2')).toBeInTheDocument();
  });

  it('renders side column values', async () => {
    await act(async () => {
      render(<BlockTradeHistory trades={MOCK_TRADES} loading={false} />);
    });

    expect(screen.getByText('buy')).toBeInTheDocument();
    expect(screen.getByText('sell')).toBeInTheDocument();
  });

  it('shows empty state when no trades', async () => {
    await act(async () => {
      render(<BlockTradeHistory trades={[]} loading={false} />);
    });

    expect(screen.getByText('No block trades')).toBeInTheDocument();
  });

  it('renders table headers', async () => {
    await act(async () => {
      render(<BlockTradeHistory trades={MOCK_TRADES} loading={false} />);
    });

    expect(screen.getByText('Symbol')).toBeInTheDocument();
    expect(screen.getByText('Counterparty')).toBeInTheDocument();
    expect(screen.getByText('Notional')).toBeInTheDocument();
  });
});
