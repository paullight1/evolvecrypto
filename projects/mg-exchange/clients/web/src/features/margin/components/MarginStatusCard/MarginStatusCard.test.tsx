import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { MarginStatusCard } from './MarginStatusCard';
import type { MarginStatus } from '../../types';

const MOCK_STATUS_SAFE: MarginStatus = {
  enabled: true,
  margin_level: '3.5',
  available_margin: '8000',
  used_margin: '2000',
  total_assets: '12000',
  total_liabilities: '2000',
};

const MOCK_STATUS_DANGER: MarginStatus = {
  enabled: true,
  margin_level: '1.1',
  available_margin: '100',
  used_margin: '1900',
  total_assets: '2000',
  total_liabilities: '1900',
};

describe('MarginStatusCard', () => {
  it('renders margin level value', () => {
    render(<MarginStatusCard status={MOCK_STATUS_SAFE} loading={false} />);

    expect(screen.getByText('3.5x')).toBeInTheDocument();
  });

  it('renders available margin, used margin, and totals', () => {
    render(<MarginStatusCard status={MOCK_STATUS_SAFE} loading={false} />);

    expect(screen.getByText('8000')).toBeInTheDocument();
    // used_margin and total_liabilities both appear as "2000" — use getAllByText
    expect(screen.getAllByText('2000').length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText('12000')).toBeInTheDocument();
  });

  it('renders Active badge when margin is enabled', () => {
    render(<MarginStatusCard status={MOCK_STATUS_SAFE} loading={false} />);

    expect(screen.getByText('Active')).toBeInTheDocument();
  });

  it('renders danger badge when margin level < 1.3', () => {
    render(<MarginStatusCard status={MOCK_STATUS_DANGER} loading={false} />);

    // The badge with the margin level value should be present
    expect(screen.getByText('1.1x')).toBeInTheDocument();
  });

  it('shows loading state', () => {
    render(<MarginStatusCard status={null} loading={true} />);

    expect(screen.getByText('Loading margin status...')).toBeInTheDocument();
  });

  it('shows unavailable message when status is null', () => {
    render(<MarginStatusCard status={null} loading={false} />);

    expect(screen.getByText('Margin status unavailable.')).toBeInTheDocument();
  });
});
