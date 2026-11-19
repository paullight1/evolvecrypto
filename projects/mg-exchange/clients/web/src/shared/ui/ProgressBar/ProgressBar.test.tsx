import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { ProgressBar } from './ProgressBar';

describe('ProgressBar', () => {
  it('renders with correct aria-valuenow', () => {
    render(<ProgressBar value={50} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
  });

  it('sets aria-valuemin to 0', () => {
    render(<ProgressBar value={50} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuemin', '0');
  });

  it('sets aria-valuemax to max prop', () => {
    render(<ProgressBar value={50} max={200} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuemax', '200');
  });

  it('defaults aria-valuemax to 100', () => {
    render(<ProgressBar value={50} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuemax', '100');
  });

  it('clamps value above max to max', () => {
    render(<ProgressBar value={150} max={100} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '100');
  });

  it('clamps negative value to 0', () => {
    render(<ProgressBar value={-10} />);
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '0');
  });

  it('shows label when provided', () => {
    render(<ProgressBar value={50} label="Margin usage" />);
    expect(screen.getByText('Margin usage')).toBeInTheDocument();
  });

  it('does not show label when not provided', () => {
    render(<ProgressBar value={50} />);
    expect(screen.queryByText('Margin usage')).not.toBeInTheDocument();
  });

  it('shows percentage value when showValue is true', () => {
    render(<ProgressBar value={65} showValue />);
    expect(screen.getByText('65%')).toBeInTheDocument();
  });

  it('does not show percentage when showValue is false', () => {
    render(<ProgressBar value={65} showValue={false} />);
    expect(screen.queryByText('65%')).not.toBeInTheDocument();
  });

  it('renders fill bar with correct inline width', () => {
    const { container } = render(<ProgressBar value={40} />);
    const fill = container.querySelector('[class*="fill"]') as HTMLElement;
    expect(fill.style.width).toBe('40%');
  });

  it('renders fill bar at 0% when value is 0', () => {
    const { container } = render(<ProgressBar value={0} />);
    const fill = container.querySelector('[class*="fill"]') as HTMLElement;
    expect(fill.style.width).toBe('0%');
  });

  it('renders fill bar at 100% when value equals max', () => {
    const { container } = render(<ProgressBar value={100} />);
    const fill = container.querySelector('[class*="fill"]') as HTMLElement;
    expect(fill.style.width).toBe('100%');
  });

  it('calculates fill width relative to custom max', () => {
    const { container } = render(<ProgressBar value={50} max={200} />);
    const fill = container.querySelector('[class*="fill"]') as HTMLElement;
    expect(fill.style.width).toBe('25%');
  });

  it('rounds displayed percentage to nearest integer', () => {
    render(<ProgressBar value={1} max={3} showValue />);
    // 1/3 * 100 = 33.33..., rounds to 33
    expect(screen.getByText('33%')).toBeInTheDocument();
  });

  it('renders without errors for all variants', () => {
    const variants = ['default', 'long', 'short', 'warning', 'accent'] as const;
    for (const variant of variants) {
      const { unmount } = render(<ProgressBar value={50} variant={variant} />);
      expect(screen.getByRole('progressbar')).toBeInTheDocument();
      unmount();
    }
  });
});
