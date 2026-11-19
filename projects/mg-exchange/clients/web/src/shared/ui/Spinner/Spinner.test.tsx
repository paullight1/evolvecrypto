import { render, screen } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import { Spinner } from './Spinner';

describe('Spinner', () => {
  it('renders with default md size', () => {
    render(<Spinner />);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('renders sm size without error', () => {
    render(<Spinner size="sm" />);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('renders lg size without error', () => {
    render(<Spinner size="lg" />);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('applies custom className', () => {
    const { container } = render(<Spinner className="custom" />);
    expect(container.firstChild).toHaveClass('custom');
  });

  it('has accessible label', () => {
    render(<Spinner />);
    expect(screen.getByLabelText('Loading')).toBeInTheDocument();
  });
});
