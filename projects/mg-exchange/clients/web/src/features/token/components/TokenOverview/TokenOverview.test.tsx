import { render, screen, act } from '@testing-library/react';
import { describe, it, expect } from 'vitest';

import { TokenOverview } from './TokenOverview';
import type { TokenSupplyInfo } from '../../types';

const MOCK_SUPPLY: TokenSupplyInfo = {
  total_supply: '1000000000',
  circulating_supply: '450000000',
  burned_total: '25000000',
  price_usdt: '2.50',
  market_cap_usdt: '1125000000',
};

describe('TokenOverview', () => {
  it('displays price when supply is provided', async () => {
    await act(async () => {
      render(<TokenOverview supply={MOCK_SUPPLY} />);
    });

    expect(screen.getByText('$2.50')).toBeInTheDocument();
  });

  it('displays circulating supply', async () => {
    await act(async () => {
      render(<TokenOverview supply={MOCK_SUPPLY} />);
    });

    // 450000000 formatted with toLocaleString
    expect(screen.getByText(/450,000,000 EXC/)).toBeInTheDocument();
  });

  it('displays total burned', async () => {
    await act(async () => {
      render(<TokenOverview supply={MOCK_SUPPLY} />);
    });

    expect(screen.getByText(/25,000,000 EXC/)).toBeInTheDocument();
  });

  it('shows placeholder dashes when supply is null', async () => {
    await act(async () => {
      render(<TokenOverview supply={null} />);
    });

    // Multiple stats show $— or — EXC when null
    const dashes = screen.getAllByText('$—');
    expect(dashes.length).toBeGreaterThan(0);
  });

  it('renders token utilities list', async () => {
    await act(async () => {
      render(<TokenOverview supply={MOCK_SUPPLY} />);
    });

    expect(screen.getByText(/Trading fee discounts/)).toBeInTheDocument();
    expect(screen.getByText(/Governance voting rights/)).toBeInTheDocument();
  });
});
