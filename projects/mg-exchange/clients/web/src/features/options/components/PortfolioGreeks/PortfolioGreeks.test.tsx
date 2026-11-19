import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { PortfolioGreeks } from './PortfolioGreeks';
import type { OptionGreeks } from '../../types';

const MOCK_GREEKS: OptionGreeks = {
  totalDelta: '1.23',
  totalGamma: '0.005',
  totalTheta: '-12.50',
  totalVega: '45.80',
};

describe('PortfolioGreeks', () => {
  it('renders the Portfolio Greeks title', () => {
    render(<PortfolioGreeks greeks={MOCK_GREEKS} loading={false} />);

    expect(screen.getByText('Portfolio Greeks')).toBeInTheDocument();
  });

  it('renders all four Greek values', () => {
    render(<PortfolioGreeks greeks={MOCK_GREEKS} loading={false} />);

    expect(screen.getByText('1.23')).toBeInTheDocument();
    expect(screen.getByText('0.005')).toBeInTheDocument();
    expect(screen.getByText('-12.50')).toBeInTheDocument();
    expect(screen.getByText('45.80')).toBeInTheDocument();
  });

  it('renders Greek labels', () => {
    render(<PortfolioGreeks greeks={MOCK_GREEKS} loading={false} />);

    expect(screen.getByText('Total Delta (Δ)')).toBeInTheDocument();
    expect(screen.getByText('Total Gamma (Γ)')).toBeInTheDocument();
    expect(screen.getByText('Total Theta (Θ)')).toBeInTheDocument();
    expect(screen.getByText('Total Vega (V)')).toBeInTheDocument();
  });

  it('shows loading message when loading is true', () => {
    render(<PortfolioGreeks greeks={null} loading={true} />);

    expect(screen.getByText('Loading portfolio Greeks...')).toBeInTheDocument();
  });

  it('shows empty message when greeks is null and not loading', () => {
    render(<PortfolioGreeks greeks={null} loading={false} />);

    expect(screen.getByText('No Greek data available.')).toBeInTheDocument();
  });
});
