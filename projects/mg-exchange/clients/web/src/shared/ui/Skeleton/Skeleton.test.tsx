import { render } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { Skeleton } from './Skeleton';

describe('Skeleton', () => {
  it('renders without error (default rect variant)', () => {
    const { container } = render(<Skeleton />);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('renders text variant without error', () => {
    const { container } = render(<Skeleton variant="text" />);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('renders circle variant without error', () => {
    const { container } = render(<Skeleton variant="circle" />);
    expect(container.firstChild).toBeInTheDocument();
  });

  it('applies numeric width as px', () => {
    const { container } = render(<Skeleton width={120} />);
    expect(container.firstChild).toHaveStyle({ width: '120px' });
  });

  it('applies string width', () => {
    const { container } = render(<Skeleton width="50%" />);
    expect(container.firstChild).toHaveStyle({ width: '50%' });
  });

  it('applies height', () => {
    const { container } = render(<Skeleton height={32} />);
    expect(container.firstChild).toHaveStyle({ height: '32px' });
  });

  it('is aria-hidden', () => {
    const { container } = render(<Skeleton />);
    expect(container.firstChild).toHaveAttribute('aria-hidden', 'true');
  });
});
