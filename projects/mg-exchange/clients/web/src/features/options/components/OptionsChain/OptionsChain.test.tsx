import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi } from 'vitest';
import { OptionsChain } from './OptionsChain';
import type { OptionInstrument, OptionTicker } from '../../types';

const INSTRUMENTS: OptionInstrument[] = [
  { symbol: 'BTC-28MAR25-60000-C', underlying: 'BTC', strike: '60000', expiry: '2025-03-28', optionType: 'Call', contractSize: '1', settleCoin: 'USDT', isActive: true },
  { symbol: 'BTC-28MAR25-60000-P', underlying: 'BTC', strike: '60000', expiry: '2025-03-28', optionType: 'Put', contractSize: '1', settleCoin: 'USDT', isActive: true },
  { symbol: 'BTC-28MAR25-70000-C', underlying: 'BTC', strike: '70000', expiry: '2025-03-28', optionType: 'Call', contractSize: '1', settleCoin: 'USDT', isActive: true },
  { symbol: 'BTC-28MAR25-70000-P', underlying: 'BTC', strike: '70000', expiry: '2025-03-28', optionType: 'Put', contractSize: '1', settleCoin: 'USDT', isActive: true },
];

const TICKERS: OptionTicker[] = [
  { symbol: 'BTC-28MAR25-60000-C', markPrice: '1225', iv: '0.65', openInterest: '500', volume24h: '100', turnover24h: '10000', expiryTimestamp: 1743120000 },
  { symbol: 'BTC-28MAR25-60000-P', markPrice: '825', iv: '0.60', openInterest: '400', volume24h: '80', turnover24h: '8000', expiryTimestamp: 1743120000 },
];

describe('OptionsChain', () => {
  it('renders strike values in the chain', () => {
    render(
      <OptionsChain
        instruments={INSTRUMENTS}
        tickers={TICKERS}
        loading={false}
        expiry="2025-03-28"
        onSelectSymbol={vi.fn()}
      />,
    );

    expect(screen.getByText('60000')).toBeInTheDocument();
    expect(screen.getByText('70000')).toBeInTheDocument();
  });

  it('renders call and put column headers', () => {
    render(
      <OptionsChain
        instruments={INSTRUMENTS}
        tickers={TICKERS}
        loading={false}
        expiry="2025-03-28"
        onSelectSymbol={vi.fn()}
      />,
    );

    expect(screen.getByText('Calls')).toBeInTheDocument();
    expect(screen.getByText('Puts')).toBeInTheDocument();
  });

  it('shows empty state when no instruments for expiry', () => {
    render(
      <OptionsChain
        instruments={[]}
        tickers={[]}
        loading={false}
        expiry="2025-03-28"
        onSelectSymbol={vi.fn()}
      />,
    );

    expect(screen.getByText('No instruments for this expiry')).toBeInTheDocument();
  });

  it('clicking a row fires onSelectSymbol with the call symbol', async () => {
    const user = userEvent.setup();
    const onSelectSymbol = vi.fn();

    render(
      <OptionsChain
        instruments={INSTRUMENTS}
        tickers={TICKERS}
        loading={false}
        expiry="2025-03-28"
        onSelectSymbol={onSelectSymbol}
      />,
    );

    // Click on the strike badge row for 60000
    const strike = screen.getByText('60000');
    await user.click(strike);

    expect(onSelectSymbol).toHaveBeenCalledWith('BTC-28MAR25-60000-C', 'buy');
  });

  it('filters by expiry — instruments from other expiries are not rendered', () => {
    const otherInstruments: OptionInstrument[] = [
      ...INSTRUMENTS,
      { symbol: 'BTC-30APR25-50000-C', underlying: 'BTC', strike: '50000', expiry: '2025-04-30', optionType: 'Call', contractSize: '1', settleCoin: 'USDT', isActive: true },
    ];
    render(
      <OptionsChain
        instruments={otherInstruments}
        tickers={TICKERS}
        loading={false}
        expiry="2025-03-28"
        onSelectSymbol={vi.fn()}
      />,
    );

    // strike 50000 belongs to different expiry, should not appear
    expect(screen.queryByText('50000')).not.toBeInTheDocument();
  });
});
