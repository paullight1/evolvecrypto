import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { StatCard } from './StatCard';

describe('StatCard', () => {
  it('renders label and value', () => {
    render(<StatCard label="Total Equity" value="12,450.00" />);
    expect(screen.getByText('Total Equity')).toBeInTheDocument();
    expect(screen.getByText('12,450.00')).toBeInTheDocument();
  });

  it('renders numeric value', () => {
    render(<StatCard label="Open Positions" value={4} />);
    expect(screen.getByText('4')).toBeInTheDocument();
  });

  it('renders change text when provided', () => {
    render(<StatCard label="PnL" value="340.50" change="+2.81%" trend="up" />);
    expect(screen.getByText('+2.81%')).toBeInTheDocument();
  });

  it('applies up trend class to change', () => {
    const { container } = render(
      <StatCard label="PnL" value="340.50" change="+2.81%" trend="up" />,
    );
    const change = container.querySelector('[class*="change"]');
    expect(change?.className).toMatch(/up/);
  });

  it('applies down trend class to change', () => {
    const { container } = render(
      <StatCard label="PnL" value="-120.00" change="-0.96%" trend="down" />,
    );
    const change = container.querySelector('[class*="change"]');
    expect(change?.className).toMatch(/down/);
  });

  it('does not render change element when change is not provided', () => {
    const { container } = render(<StatCard label="Equity" value="1000" />);
    expect(container.querySelector('[class*="change"]')).toBeNull();
  });

  it('renders loading skeletons when loading is true', () => {
    const { container } = render(
      <StatCard label="Equity" value="1000" loading />,
    );
    // Skeleton renders aria-hidden spans; label and value text should not be visible
    expect(screen.queryByText('Equity')).not.toBeInTheDocument();
    expect(screen.queryByText('1000')).not.toBeInTheDocument();
    // Two skeleton elements rendered
    const skeletons = container.querySelectorAll('[aria-hidden="true"]');
    expect(skeletons.length).toBe(2);
  });

  it('renders prefix when provided', () => {
    render(<StatCard label="Balance" value="8,200.00" prefix="$" />);
    expect(screen.getByText('$')).toBeInTheDocument();
  });

  it('renders suffix when provided', () => {
    render(<StatCard label="Balance" value="8,200.00" suffix="USDT" />);
    expect(screen.getByText('USDT')).toBeInTheDocument();
  });

  it('renders both prefix and suffix', () => {
    render(<StatCard label="Balance" value="8,200.00" prefix="$" suffix="USDT" />);
    expect(screen.getByText('$')).toBeInTheDocument();
    expect(screen.getByText('USDT')).toBeInTheDocument();
  });

  it('renders icon when provided', () => {
    render(
      <StatCard
        label="Positions"
        value="4"
        icon={<span data-testid="stat-icon">icon</span>}
      />,
    );
    expect(screen.getByTestId('stat-icon')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(
      <StatCard label="Test" value="0" className="my-class" />,
    );
    expect(container.firstChild).toHaveClass('my-class');
  });
});
